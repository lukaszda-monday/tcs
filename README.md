# tcs — tmux-claude-session

TUI for managing git worktree + tmux + Claude Code sessions.

Select a branch (fuzzy autocomplete), create a worktree, launch a tmux session with Claude in the left pane and nvim in the right. Cleanup prompt on exit.

## Install

```bash
# From source
cargo install --path .

# Or build and copy manually
cargo build --release
cp target/release/tcs ~/.local/bin/
```

Requires: `git`, `tmux`, `nvim`

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
# Command to run (default: "claude")
command: "~/.local/bin/axcli spawn"

# Default setup command for all repos
default_setup: "echo ready"

# Per-language setup (auto-detected via tokei)
languages:
  rust:
    setup: "cargo build"
  javascript:
    setup: "npm install"
  typescript:
    setup: "npm install"
  python:
    setup: "pip install -e ."

# Per-repo setup (takes priority over language)
repos:
  my-monorepo:
    setup: "make install && make build"
```

Priority: `repos.<name>` > `languages.<detected>` > `default_setup`

## TUI Flow

1. **Branch** — type to fuzzy-search, arrow keys to navigate, Enter to select, Esc to dismiss popup, Tab to reopen
2. **Base** (new branches only) — pre-filled with `master`/`main`, same controls
3. **Pull base** (new branches only) — toggle yes/no, defaults to yes
4. Worktree created, tmux launches with:
   - Left pane: setup command → Claude Code
   - Right pane: `nvim .`
5. After Claude exits: cleanup prompt to keep/remove worktree

## Worktree Layout

```
../worktrees/<repo-name>/<branch>/
```

Relative to the repo root, matching the original `claude-sesh` layout.
