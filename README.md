# tcs — tmux-claude-session

TUI for managing git worktree + tmux + Claude Code sessions.

Select a branch (fuzzy autocomplete with highlighting), create a worktree, launch a tmux session with Claude in the left pane and nvim in the right. Setup commands run in a separate pane. Cleanup prompt on exit.

## Install

Requires [Rust](https://rustup.rs/), `git`, `tmux`, `nvim`

```bash
cargo install --path .
```

## Usage

```bash
# Run from any git repo — launches TUI
tcs

# Force a specific tmux mode
tcs --tab        # new tab in current tmux session (default inside tmux)
tcs --session    # separate tmux session (default outside tmux)
tcs --ghostty    # new Ghostty window

# Pass args to the claude command
tcs -- --model sonnet
```

## Config

Create `~/.config/tcs.yml`:

```yaml
# Command to run in the claude pane (default: "claude")
command: "~/.local/bin/axcli spawn"

# Per-language setup command (auto-detected via tokei)
# Runs in a horizontal split pane alongside claude, closes on success,
# drops to a shell on failure so you can debug
languages:
  setup:
    tsx: "yarn install"
    jsx: "yarn install"
    javascript: "yarn install"
    typescript: "yarn install"

# Per-repo setup (takes priority over language detection)
# Repo name = directory name where tcs is invoked
repos:
  setup:
    my-monorepo: "make install && make build"
    api-service: "cargo build"

# Fallback if no repo or language match
# default_setup: "echo ready"
```

Priority: `repos.setup.<name>` > `languages.setup.<detected>` > `default_setup`

## TUI Flow

1. **Branch** — type to fuzzy-search (matched characters highlighted), arrow keys to navigate popup, Enter to select, Esc to dismiss, Tab to reopen
2. **Base** (new branches only) — pre-filled with `master` or `main` (auto-detected), same controls
3. **Pull base** (new branches only) — yes/no toggle, defaults to yes
4. Worktree created, tmux launches with three panes:
   - `claude-master-agent` — setup command, then Claude Code
   - `nvim` — nvim opened in the worktree
   - `setup` — setup command output (temporary, closes on success)
5. After Claude exits: cleanup prompt to keep/remove worktree

## Pane Layout

```
┌──────────────────────┬──────────────────────┐
│ claude-master-agent  │ nvim                 │
│                      │                      │
│                      │                      │
├──────────────────────┤                      │
│ setup (temporary)    │                      │
└──────────────────────┴──────────────────────┘
```

Pane names are locked — Claude Code cannot override them.

## Worktree Layout

```
../worktrees/<repo-name>/<branch>/
```

Relative to the repo root. Remote-only branches appear in autocomplete with a `(remote)` indicator.

## Key Bindings (TUI)

| Key | Action |
|-----|--------|
| Type | Filter/enter branch name |
| Arrow keys | Navigate autocomplete popup |
| Enter | Select from popup / submit field |
| Tab | Accept suggestion / reopen popup |
| Esc | Dismiss popup / cancel TUI |
| Ctrl+C | Cancel immediately |
| Alt+Backspace | Delete word |
| Space/y/n | Toggle pull option |
