use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Run a git command, capturing stderr. On failure, include stderr in the error.
fn git_run(args: &[&str]) -> Result<()> {
    let output = Command::new("git")
        .args(args)
        .output()
        .context("failed to run git")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("git {} failed: {}", args.iter().take(3).copied().collect::<Vec<_>>().join(" "), stderr.trim());
    }
    Ok(())
}

/// Run a git command, return stdout.
fn git_output(args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .args(args)
        .output()
        .context("failed to run git")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("git {} failed: {}", args.iter().take(3).copied().collect::<Vec<_>>().join(" "), stderr.trim());
    }
    Ok(String::from_utf8(output.stdout)?.trim().to_string())
}

/// Run a git command, return whether it succeeded (no error on failure).
fn git_check(args: &[&str]) -> Result<bool> {
    let output = Command::new("git")
        .args(args)
        .output()
        .context("failed to run git")?;
    Ok(output.status.success())
}

/// Resolve the git repo root from a directory.
pub fn repo_root(dir: &Path) -> Result<PathBuf> {
    let root = git_output(&["-C", &dir.to_string_lossy(), "rev-parse", "--show-toplevel"])
        .with_context(|| format!("not a git repository: {}", dir.display()))?;
    Ok(PathBuf::from(root))
}

/// A branch with its source (local or remote).
#[derive(Clone)]
pub struct BranchInfo {
    pub name: String,
    pub is_remote: bool,
}

/// List all branches: local first, then remote-only (deduped).
pub fn list_all_branches(repo: &Path) -> Result<Vec<BranchInfo>> {
    let repo_str = repo.to_string_lossy();

    // Local branches
    let local_stdout = git_output(&[
        "-C", &repo_str,
        "for-each-ref", "--format=%(refname:short)", "refs/heads/",
    ])?;
    let local: Vec<String> = local_stdout.lines().filter(|s| !s.is_empty()).map(|s| s.to_string()).collect();
    let local_set: std::collections::HashSet<&str> = local.iter().map(|s| s.as_str()).collect();

    // Remote branches — strip "origin/" or any remote prefix
    let remote_stdout = git_output(&[
        "-C", &repo_str,
        "for-each-ref", "--format=%(refname:short)", "refs/remotes/",
    ])?;

    let mut branches: Vec<BranchInfo> = local
        .iter()
        .map(|name| BranchInfo { name: name.clone(), is_remote: false })
        .collect();

    for line in remote_stdout.lines().filter(|s| !s.is_empty()) {
        // Skip HEAD pointers like "origin/HEAD"
        if line.ends_with("/HEAD") {
            continue;
        }
        // Strip remote prefix: "origin/feat/foo" -> "feat/foo"
        let branch_name = if let Some(pos) = line.find('/') {
            &line[pos + 1..]
        } else {
            line
        };
        // Only add if not already local
        if !local_set.contains(branch_name) {
            branches.push(BranchInfo {
                name: branch_name.to_string(),
                is_remote: true,
            });
        }
    }

    Ok(branches)
}

/// Detect the default branch (master or main) with a single git call.
pub fn detect_default_branch(repo: &Path) -> String {
    let repo_str = repo.to_string_lossy();
    let refs = git_output(&[
        "-C", &repo_str, "for-each-ref", "--format=%(refname)",
        "refs/heads/main", "refs/heads/master",
        "refs/remotes/origin/main", "refs/remotes/origin/master",
    ])
    .unwrap_or_default();

    // Priority: local main > local master > origin/main > origin/master
    for candidate in [
        ("refs/heads/main", "main"),
        ("refs/heads/master", "master"),
        ("refs/remotes/origin/main", "origin/main"),
        ("refs/remotes/origin/master", "origin/master"),
    ] {
        if refs.lines().any(|l| l == candidate.0) {
            return candidate.1.to_string();
        }
    }
    "master".to_string()
}

/// Pull (fetch + merge) a ref in the repo.
pub fn pull_ref(repo: &Path) -> Result<()> {
    let repo_str = repo.to_string_lossy();
    git_run(&["-C", &repo_str, "pull"])
}

/// Check if a local branch exists.
pub fn branch_exists(repo: &Path, branch: &str) -> Result<bool> {
    git_check(&[
        "-C", &repo.to_string_lossy(),
        "show-ref", "--verify", "--quiet", &format!("refs/heads/{branch}"),
    ])
}

/// Check if a remote tracking branch exists, return the remote ref if found.
pub fn find_remote_branch(repo: &Path, branch: &str) -> Result<Option<String>> {
    let stdout = git_output(&[
        "-C", &repo.to_string_lossy(),
        "for-each-ref", "--format=%(refname:short)", &format!("refs/remotes/*/{branch}"),
    ])?;
    Ok(stdout.lines().next().map(|s| s.to_string()))
}

/// Validate a branch name.
pub fn validate_branch_name(branch: &str) -> Result<bool> {
    git_check(&["check-ref-format", "--allow-onelevel", &format!("refs/heads/{branch}")])
}

/// Create a worktree. Returns the action taken.
pub fn create_worktree(
    repo: &Path,
    worktree_dir: &Path,
    branch: &str,
    base: &str,
) -> Result<String> {
    if worktree_dir.exists() {
        return Ok(format!("Reusing existing worktree at {}", worktree_dir.display()));
    }

    if let Some(parent) = worktree_dir.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let repo_str = repo.to_string_lossy();
    let wt_str = worktree_dir.to_string_lossy();

    if branch_exists(repo, branch)? {
        git_run(&["-C", &repo_str, "worktree", "add", &wt_str, branch])?;
        Ok(format!("Checked out existing branch '{branch}'"))
    } else if let Some(remote_ref) = find_remote_branch(repo, branch)? {
        git_run(&["-C", &repo_str, "worktree", "add", "--track", "-b", branch, &wt_str, &remote_ref])?;
        Ok(format!("Tracking remote branch '{remote_ref}'"))
    } else {
        git_run(&["-C", &repo_str, "worktree", "add", "--no-track", "-b", branch, &wt_str, base])?;
        Ok(format!("Created new branch '{branch}' from {base}"))
    }
}

/// Remove a worktree.
pub fn remove_worktree(repo: &Path, worktree_dir: &Path) -> Result<()> {
    let repo_str = repo.to_string_lossy();
    let wt_str = worktree_dir.to_string_lossy();

    if let Err(e) = git_run(&["-C", &repo_str, "worktree", "remove", "--force", &wt_str]) {
        tracing::error!("git worktree remove failed: {e:#}, falling back to rm -rf");
        std::fs::remove_dir_all(worktree_dir).ok();
    }

    // Prune
    let _ = git_run(&["-C", &repo_str, "worktree", "prune"]);
    cleanup_empty_parents(worktree_dir, repo);
    Ok(())
}

/// Delete a local branch.
pub fn delete_branch(repo: &Path, branch: &str) -> Result<bool> {
    git_check(&["-C", &repo.to_string_lossy(), "branch", "-D", branch])
}

/// Clean up empty parent directories up to (and including) the worktrees base.
fn cleanup_empty_parents(worktree_dir: &Path, repo: &Path) {
    let worktrees_root = repo.parent().unwrap().join("worktrees");
    let mut dir = worktree_dir.parent();
    while let Some(d) = dir {
        if d == worktrees_root.parent().unwrap_or(Path::new("/")) {
            break;
        }
        if std::fs::remove_dir(d).is_err() {
            break;
        }
        dir = d.parent();
    }
}

/// Compute the worktree directory path.
/// Branch names with `/` (e.g. `feat/login`) produce nested dirs,
/// matching the original bash script behavior.
pub fn worktree_path(repo: &Path, branch: &str) -> PathBuf {
    let repo_name = repo.file_name().unwrap().to_string_lossy();
    let worktree_base = repo.parent().unwrap().join("worktrees").join(&*repo_name);
    worktree_base.join(branch)
}
