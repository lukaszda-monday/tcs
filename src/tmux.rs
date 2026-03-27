use anyhow::{bail, Result};
use std::path::Path;
use std::process::Command;

/// Mode for launching the tmux session.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Mode {
    Tab,
    Session,
    Ghostty,
}

/// Check if we're inside a tmux session.
pub fn inside_tmux() -> bool {
    std::env::var("TMUX").is_ok()
}

/// Get the current tmux session name.
pub fn current_session() -> Option<String> {
    let output = Command::new("tmux")
        .args(["display-message", "-p", "#S"])
        .output()
        .ok()?;
    if output.status.success() {
        Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        None
    }
}

/// Check if a tmux session already exists.
pub fn session_exists(name: &str) -> bool {
    Command::new("tmux")
        .args(["has-session", "-t", &format!("={name}")])
        .output()
        .is_ok_and(|o| o.status.success())
}

/// Check if a window with a given title exists in a session.
pub fn window_exists(session: &str, window_title: &str) -> Option<String> {
    let output = Command::new("tmux")
        .args([
            "list-windows",
            "-t",
            &format!("={session}"),
            "-F",
            "#I:#W",
        ])
        .output()
        .ok()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        if let Some((idx, name)) = line.split_once(':') {
            if name == window_title {
                return Some(idx.to_string());
            }
        }
    }
    None
}

/// Select an existing window.
pub fn select_window(session: &str, window_idx: &str) -> Result<()> {
    let status = Command::new("tmux")
        .args([
            "select-window",
            "-t",
            &format!("={session}:{window_idx}"),
        ])
        .status()?;
    if !status.success() {
        bail!("failed to select tmux window");
    }
    Ok(())
}

/// Attach to an existing session.
pub fn attach_session(name: &str) -> Result<()> {
    if inside_tmux() {
        Command::new("tmux")
            .args(["switch-client", "-t", &format!("={name}")])
            .status()?;
    } else {
        Command::new("tmux")
            .args(["attach-session", "-t", &format!("={name}")])
            .status()?;
    }
    Ok(())
}

/// Build the session name from repo + branch.
pub fn session_name(repo_name: &str, branch: &str) -> String {
    format!("tcs/{repo_name}/{branch}").replace(['.', ':'], "-")
}

/// Build the window title. Uses `/` as separator to avoid
/// conflicting with tmux's `session:window` target syntax.
pub fn window_title(repo_name: &str, branch: &str) -> String {
    format!("{repo_name}/{branch}")
}

/// Write the inner script that runs inside the tmux pane.
/// This script: runs setup, then launches claude, then shows cleanup TUI.
pub fn write_inner_script(
    worktree_dir: &Path,
    repo_dir: &Path,
    branch: &str,
    claude_cmd: &str,
    setup_cmd: Option<&str>,
    tcs_bin: &str,
) -> Result<std::path::PathBuf> {
    let tmp = std::env::temp_dir().join(format!(
        "tcs-inner-{}.sh",
        std::process::id()
    ));

    let worktree_str = worktree_dir.to_string_lossy();
    let repo_str = repo_dir.to_string_lossy();

    let setup_block = if let Some(cmd) = setup_cmd {
        format!(
            r#"
echo ""
echo -e "\033[0;36m▸\033[0m Running setup: {cmd}"
echo ""
{cmd}
SETUP_EXIT=$?
if [ $SETUP_EXIT -ne 0 ]; then
    echo ""
    echo -e "\033[0;31m✗ Setup failed (exit $SETUP_EXIT)\033[0m"
    echo ""
fi
"#
        )
    } else {
        String::new()
    };

    let script = format!(
        r#"#!/usr/bin/env bash
# Self-delete: the kernel holds the open fd; safe even after unlink.
rm -f "$0"

cd {worktree_quoted} || exit 1

echo ""
echo -e "\033[0;36m━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\033[0m"
echo -e " \033[1mWorktree\033[0m  {worktree_str}"
echo -e " \033[1mBranch\033[0m    $(git rev-parse --abbrev-ref HEAD 2>/dev/null || echo '???')"
echo -e "\033[0;36m━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\033[0m"
{setup_block}
echo ""

# Run Claude Code
{claude_cmd}

echo ""

# Run cleanup TUI
{tcs_bin} --cleanup --cleanup-worktree {worktree_quoted} --cleanup-repo {repo_quoted} --cleanup-branch {branch_quoted}
"#,
        worktree_quoted = shell_quote(&worktree_str),
        repo_quoted = shell_quote(&repo_str),
        branch_quoted = shell_quote(branch),
    );

    std::fs::write(&tmp, &script)?;

    // chmod +x
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755))?;
    }

    Ok(tmp)
}

pub(crate) fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// Launch the tmux session/tab with the inner script.
pub fn launch(
    mode: Mode,
    session_name: &str,
    window_title: &str,
    worktree_dir: &Path,
    inner_script: &Path,
) -> Result<()> {
    let wt = worktree_dir.to_string_lossy();
    let script = inner_script.to_string_lossy();
    let pane_title = format!("claude \u{25b8} {window_title}");

    match mode {
        Mode::Tab => {
            // Create new window with inner script in left pane
            let status = Command::new("tmux")
                .args([
                    "new-window",
                    "-n",
                    window_title,
                    "-c",
                    &wt,
                    &*script,
                ])
                .status()?;
            if !status.success() {
                bail!("failed to create tmux window");
            }
            // Set pane title for the claude pane
            Command::new("tmux")
                .args(["select-pane", "-T", &pane_title])
                .status()?;
            // Split right pane with nvim
            Command::new("tmux")
                .args(["split-window", "-h", "-c", &wt, &format!("nvim {}", shell_quote(&wt))])
                .status()?;
            // Select left pane (claude pane)
            Command::new("tmux")
                .args(["select-pane", "-t", "0"])
                .status()?;
            // Lock window name
            Command::new("tmux")
                .args(["set-option", "-w", "allow-rename", "off"])
                .status()?;
            Command::new("tmux")
                .args(["set-option", "-w", "automatic-rename", "off"])
                .status()?;
            // Show pane borders with titles
            Command::new("tmux")
                .args(["set-option", "-w", "pane-border-status", "top"])
                .status()?;
        }
        Mode::Ghostty => {
            // Create detached session
            Command::new("tmux")
                .args([
                    "new-session",
                    "-d",
                    "-s",
                    session_name,
                    "-c",
                    &wt,
                    "-n",
                    window_title,
                    &*script,
                ])
                .status()?;
            configure_session_panes(session_name, window_title, &wt, &pane_title)?;
            // Open ghostty
            let child = Command::new("ghostty")
                .args(["-e", "tmux", "attach-session", "-t", &format!("={session_name}")])
                .spawn()?;
            // Detach from child
            std::mem::forget(child);
        }
        Mode::Session => {
            Command::new("tmux")
                .args([
                    "new-session", "-d", "-s", session_name,
                    "-n", window_title, "-c", &wt, &*script,
                ])
                .status()?;
            configure_session_panes(session_name, window_title, &wt, &pane_title)?;
            let cmd = if inside_tmux() { "switch-client" } else { "attach-session" };
            Command::new("tmux")
                .args([cmd, "-t", &format!("={session_name}")])
                .status()?;
        }
    }

    Ok(())
}

/// Configure panes for session/ghostty mode (split, nvim, pane titles, etc.)
fn configure_session_panes(
    session_name: &str,
    window_title: &str,
    worktree_dir: &str,
    pane_title: &str,
) -> Result<()> {
    let target = format!("={session_name}:{window_title}");

    // Set pane title for claude pane
    Command::new("tmux")
        .args(["select-pane", "-t", &format!("{target}.0"), "-T", pane_title])
        .status()?;
    // Split right pane with nvim
    Command::new("tmux")
        .args([
            "split-window",
            "-h",
            "-c",
            worktree_dir,
            "-t",
            &target,
            &format!("nvim {}", shell_quote(worktree_dir)),
        ])
        .status()?;
    // Select left pane
    Command::new("tmux")
        .args(["select-pane", "-t", &format!("{target}.0")])
        .status()?;
    // Lock window name and show pane borders
    Command::new("tmux")
        .args(["set-option", "-w", "-t", &target, "allow-rename", "off"])
        .status()?;
    Command::new("tmux")
        .args([
            "set-option",
            "-w",
            "-t",
            &target,
            "automatic-rename",
            "off",
        ])
        .status()?;
    Command::new("tmux")
        .args([
            "set-option",
            "-w",
            "-t",
            &target,
            "pane-border-status",
            "top",
        ])
        .status()?;

    Ok(())
}
