mod config;
mod git;
mod lang;
mod tmux;
mod tui;
mod ui;

use anyhow::{bail, Context, Result};
use clap::Parser;
use std::path::PathBuf;

fn init_logging() {
    let paths = [
        PathBuf::from("/var/log/tcs.log"),
        dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("/tmp"))
            .join("tcs.log"),
        PathBuf::from("/tmp/tcs.log"),
    ];
    let log_file = paths.iter().find_map(|p| {
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(p)
            .ok()
    });
    if let Some(file) = log_file {
        tracing_subscriber::fmt()
            .with_writer(std::sync::Mutex::new(file))
            .with_ansi(false)
            .with_target(false)
            .with_max_level(tracing::Level::ERROR)
            .init();
    }
}

#[derive(Parser)]
#[command(name = "tcs", version, about = "tmux-claude-session: TUI for git worktree + tmux + Claude Code")]
struct Cli {
    /// Open as a new tab in the current tmux session
    #[arg(long)]
    tab: bool,

    /// Create a separate tmux session (default outside tmux)
    #[arg(long)]
    session: bool,

    /// Spawn a new Ghostty window attached to the session
    #[arg(long)]
    ghostty: bool,

    /// Extra arguments to pass to the claude command
    #[arg(last = true)]
    claude_args: Vec<String>,

    #[arg(long, hide = true)]
    cleanup: bool,

    #[arg(long, hide = true)]
    cleanup_worktree: Option<String>,

    #[arg(long, hide = true)]
    cleanup_repo: Option<String>,

    #[arg(long, hide = true)]
    cleanup_branch: Option<String>,
}

fn main() {
    init_logging();
    if let Err(e) = run() {
        tracing::error!("{e:#}");
        if let Some(mut out) = ui::Output::new() {
            out.error(&format!("{e:#}"));
        } else {
            eprintln!("ERROR: {e:#}");
        }
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();

    if cli.cleanup {
        let worktree = cli.cleanup_worktree.as_deref().unwrap_or("");
        let repo = cli.cleanup_repo.as_deref().unwrap_or("");
        let branch = cli.cleanup_branch.as_deref().unwrap_or("");
        if worktree.is_empty() || repo.is_empty() || branch.is_empty() {
            bail!("cleanup requires --cleanup-worktree, --cleanup-repo, and --cleanup-branch");
        }
        return run_cleanup(worktree, repo, branch);
    }

    run_main(cli)
}

fn run_cleanup(worktree: &str, repo: &str, branch: &str) -> Result<()> {
    let worktree_dir = PathBuf::from(worktree);
    let repo_dir = PathBuf::from(repo);

    let action = tui::cleanup_prompt(&worktree_dir, branch)?;
    let mut out = ui::Output::new().context("failed to init output")?;

    match action {
        tui::cleanup::CleanupAction::Keep => {
            out.done_label("Git", &format!("Worktree kept at: {}", worktree_dir.display()));
        }
        tui::cleanup::CleanupAction::Remove => {
            kill_other_panes();
            out.done_label("Git", &format!("Removing worktree at {} ...", worktree_dir.display()));
            git::remove_worktree(&repo_dir, &worktree_dir)
                .context("failed to remove worktree")?;
            out.done_label("Git", "Worktree removed.");
        }
        tui::cleanup::CleanupAction::RemoveAndDeleteBranch => {
            kill_other_panes();
            out.done_label("Git", &format!("Removing worktree at {} ...", worktree_dir.display()));
            git::remove_worktree(&repo_dir, &worktree_dir)
                .context("failed to remove worktree")?;
            out.done_label("Git", "Worktree removed.");
            if git::delete_branch(&repo_dir, branch)? {
                out.done_label("Git", &format!("Branch '{branch}' deleted."));
            } else {
                out.error("Could not delete branch.");
            }
        }
    }

    Ok(())
}

fn run_main(cli: Cli) -> Result<()> {
    let mode = if cli.ghostty {
        tmux::Mode::Ghostty
    } else if cli.tab {
        tmux::Mode::Tab
    } else if cli.session {
        tmux::Mode::Session
    } else if tmux::inside_tmux() {
        tmux::Mode::Tab
    } else {
        tmux::Mode::Session
    };

    if mode == tmux::Mode::Tab && !tmux::inside_tmux() {
        bail!("--tab requires being inside a tmux session");
    }

    let cwd = std::env::current_dir()?;
    let repo = git::repo_root(&cwd).context("not a git repository")?;
    let repo_name = repo
        .file_name()
        .unwrap()
        .to_string_lossy()
        .to_string();

    let cfg = config::Config::load()?;
    let branches = git::list_all_branches(&repo)?;
    let default_base = git::detect_default_branch(&repo);

    let selector = tui::BranchSelector::new(branches, &default_base);
    let selection = match selector.run()? {
        Some(s) => s,
        None => {
            if let Some(mut out) = ui::Output::new() {
                out.warn("Cancelled.");
            }
            return Ok(());
        }
    };

    if !git::validate_branch_name(&selection.branch)? {
        bail!("invalid branch name: '{}'", selection.branch);
    }

    let worktree_dir = git::worktree_path(&repo, &selection.branch);
    let session_name = tmux::session_name(&repo_name, &selection.branch);
    let window_title = tmux::window_title(&repo_name, &selection.branch);

    let mut out = ui::Output::new().context("failed to init output")?;
    out.sep();

    // Check for existing session/window
    if mode == tmux::Mode::Tab {
        if let Some(current) = tmux::current_session() {
            if let Some(win_idx) = tmux::window_exists(&current, &window_title) {
                out.warn(&format!("Window '{window_title}' already exists. Selecting it."));
                tmux::select_window(&current, &win_idx)?;
                return Ok(());
            }
        }
    } else if tmux::session_exists(&session_name) {
        out.warn(&format!("Session '{session_name}' already exists. Attaching."));
        if mode == tmux::Mode::Ghostty {
            std::process::Command::new("ghostty")
                .args(["-e", "tmux", "attach-session", "-t", &format!("={session_name}")])
                .spawn()?;
            return Ok(());
        }
        tmux::attach_session(&session_name)?;
        return Ok(());
    }

    // Pull base branch if requested (before creating worktree)
    if selection.pull_base {
        out.done_label("Git", &format!("Pulling {} ...", selection.base));
        git::pull_ref(&repo).context("failed to pull base branch")?;
    }

    // Create worktree
    let wt_msg = git::create_worktree(&repo, &worktree_dir, &selection.branch, &selection.base)
        .context("failed to create worktree")?;
    out.done_label("Git", &wt_msg);

    // Detect language only if it could influence the setup command
    let detected_lang = if cfg.repos.setup.contains_key(&repo_name) || cfg.languages.setup.is_empty() {
        None
    } else {
        lang::detect(&worktree_dir)
    };
    let setup_cmd = cfg.setup_command(&repo_name, detected_lang.as_deref());

    // Check if nvm is needed: node-ish language + .nvmrc present
    const NODE_LANGS: &[&str] = &["tsx", "jsx", "javascript", "typescript"];
    let is_node = detected_lang.as_deref().is_some_and(|l| NODE_LANGS.contains(&l));
    let nvmrc_path = worktree_dir.join(".nvmrc");
    let use_nvm = is_node && nvmrc_path.exists();

    if use_nvm {
        let required_version = std::fs::read_to_string(&nvmrc_path)
            .context("failed to read .nvmrc")?
            .trim()
            .to_string();
        let nvm_check = std::process::Command::new("bash")
            .args(["-c", &format!(
                "export NVM_DIR=\"$HOME/.nvm\" && [ -s \"$NVM_DIR/nvm.sh\" ] && . \"$NVM_DIR/nvm.sh\" && nvm ls {} >/dev/null 2>&1",
                shell_quote(&required_version)
            )])
            .status();
        if !nvm_check.is_ok_and(|s| s.success()) {
            out.error(&format!(
                "Node {} (from .nvmrc) is not installed. Run: nvm install {}",
                required_version, required_version
            ));
            return Ok(());
        }
        out.done_label("nvm", &format!("Node {required_version} (from .nvmrc)"));
    }

    if let Some(ref cmd) = setup_cmd {
        out.done_val("Setup", &format!("{cmd} (in tmux)"));
    }

    // Build claude command
    let mut claude_cmd = if cli.claude_args.is_empty() {
        cfg.command.clone()
    } else {
        format!(
            "{} {}",
            cfg.command,
            cli.claude_args
                .iter()
                .map(|a| shell_quote(a))
                .collect::<Vec<_>>()
                .join(" ")
        )
    };
    if selection.skip_permissions {
        claude_cmd.push_str(" --dangerously-skip-permissions");
    }

    let tcs_bin = std::env::current_exe()?
        .to_string_lossy()
        .to_string();

    let inner_script = tmux::write_inner_script(
        &worktree_dir,
        &repo,
        &selection.branch,
        &claude_cmd,
        setup_cmd.as_deref(),
        &tcs_bin,
        use_nvm,
    )?;

    let mode_str = match mode {
        tmux::Mode::Tab => "tab",
        tmux::Mode::Session => "session",
        tmux::Mode::Ghostty => "session + Ghostty",
    };
    out.done_val(&format!("Launching tmux {mode_str}"), &window_title);

    drop(out);

    tmux::launch(mode, &session_name, &window_title, &worktree_dir, &inner_script, use_nvm)
        .context("failed to launch tmux")?;

    Ok(())
}

fn kill_other_panes() {
    let _ = std::process::Command::new("tmux")
        .args(["kill-pane", "-a"])
        .output();
}

use tmux::shell_quote;
