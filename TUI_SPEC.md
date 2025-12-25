# agent-dashboard TUI Specification

## Overview

Rust-based TUI for managing multiple development environments across multiple hosts.
Connects to agent-dashboard servers via their HTTP API.

## CLI Options

```
agent-dashboard-tui [OPTIONS]

OPTIONS:
    -d, --dump    Fetch all hosts, print JSON, and exit (debug mode)
    -h, --help    Show help message
```

## Layout

Single-pane stacked view with collapsible host sections:

```
┌─ agent-dashboard ─────────────────────────────────────────────────────────────────────────────┐
│ ● ▼ C-5001 (5 repos, 2 active)                                                                │
│   ● agent-dashboard    main         #42   jekyll:4000     fix: update dashboard stuff  2h ago │
│   ● blog               feat/new     #15   vite:5173       feat: add new feature       1h ago │
│     settings           main               chore: update dependencies                   3h ago │
│   ▼ Stale with Servers (1)                                                                    │
│   ● old-project        main               next:3000       update                       3d ago │
│   ▶ Stale Repos (8)                                                                           │
│                                                                                               │
│ ● ▼ C-5002 (3 repos, 0 active)                                                                │
│     my-project         main                               init project                 1d ago │
│     other-repo         dev                                wip                          2d ago │
│   ▶ Stale Repos (5)                                                                           │
│                                                                                               │
│ ● ▶ C-5003 (disconnected)                                                                     │
│                                                                                               │
├───────────────────────────────────────────────────────────────────────────────────────────────┤
│ ↑↓:nav o:branch d:diff p:pr s:server e:editor r:refresh                                       │
└───────────────────────────────────────────────────────────────────────────────────────────────┘
```

## Hierarchy

1. **Host** (Level 1) - collapsible, shows connection status
2. **Fresh repos** - always visible under host
3. **Stale with Servers** - collapsible section (open by default)
4. **Stale Repos** - collapsible section (closed by default)

## Host Ordering

- Connected hosts first
- Connecting hosts in middle
- Disconnected hosts at bottom (collapsed by default)

## Configuration

```toml
# ~/.config/agent-dashboard/hosts.toml
[[hosts]]
name = "C-5001"
url = "http://c-5001.squeaker-teeth.ts.net:9999"

[[hosts]]
name = "C-5002"
url = "http://c-5002.squeaker-teeth.ts.net:9999"

[[hosts]]
name = "C-5003"
url = "http://c-5003.squeaker-teeth.ts.net:9999"

[[hosts]]
name = "C-5004"
url = "http://c-5004.squeaker-teeth.ts.net:9999"
```

## Display Columns

| Column | Width | Description |
|--------|-------|-------------|
| Name | dynamic (max 26) | Repo directory name |
| Branch | dynamic (max 28) | Current git branch |
| PR | dynamic (max 8) | PR number if exists (e.g., `#42`), strikethrough if merged |
| Server | dynamic (max 30) | Running server type:port, shows `+N` for overflow |
| Commit | fills remaining | Last commit message (uses available space) |
| Time | clipped last | Relative time of last commit |

## Status Indicators

| Indicator | Meaning |
|-----------|---------|
| `●` (green) | Connected host |
| `◐` (yellow) | Connecting |
| `●` (red) | Disconnected |
| `▼` | Expanded section |
| `▶` | Collapsed section |

## Navigation

| Key | Action |
|-----|--------|
| `↑` / `k` / `C-p` | Move up |
| `↓` / `j` / `C-n` | Move down |
| `Enter` | Expand/collapse if on section header |
| `Enter` | Open terminal if on repo |
| `g` `g` | Jump to first |
| `G` | Jump to last |
| `C-u` | Page up |
| `C-d` | Page down |

## Actions

| Key | Action | Description |
|-----|--------|-------------|
| `Enter` | Open terminal | cd to repo directory |
| `o` | Open branch | GitHub branch page |
| `d` | Open diff | GitHub diff vs main |
| `p` | Open PR | GitHub PR page (if exists) |
| `s` | Open server | Server URL (picker if multiple) |
| `e` | Open editor | $EDITOR or nvim |

## Host Actions

| Key | Action |
|-----|--------|
| `1-9` | Jump to host N |
| `Tab` | Next host |
| `S-Tab` | Previous host |

## Utility Actions

| Key | Action |
|-----|--------|
| `r` | Refresh current host |
| `R` | Refresh all hosts |
| `?` / `F1` | Help overlay |
| `q` / `Esc` / `C-c` | Quit |

## Filtering

- Press `/` to start filtering
- Type to filter repos by name or branch (live filtering)
- `Backspace` to delete characters
- `Enter` to stop filtering (keep filter active)
- `Esc` clears filter and exits filter mode
- Filter shown in header: `/query_`

## Polling

| Condition | Interval |
|-----------|----------|
| Host connected | 10 seconds |
| Host disconnected | 60 seconds |

## Server Picker

When pressing `s` on a repo with multiple servers:

- Shows popup with numbered list of servers
- `1-9` to select directly
- `↑` / `k` / `C-p` to move up
- `↓` / `j` / `C-n` to move down
- `Enter` to open selected
- `Esc` to cancel

Servers use Tailscale URLs when available for remote access.

## Terminal Integration

### Open Terminal (Enter)
- macOS: `open -a Terminal.app {path}` or iTerm AppleScript
- Linux: Detect terminal, spawn with cd

### Open Browser (o, d, p, s)
- Uses `open` crate for cross-platform URL opening
- Servers prefer tailscale_url for remote access

### Open Editor (e)
- `$EDITOR {path}` or `nvim {path}`

## Technical Notes

### Dependencies (Rust)

- `ratatui` - TUI framework
- `crossterm` - Terminal handling
- `reqwest` - HTTP client (async, rustls)
- `tokio` - Async runtime
- `serde` / `serde_json` - JSON parsing
- `toml` - TOML config parsing
- `dirs` - Config directory
- `open` - Cross-platform URL/path opening
- `anyhow` - Error handling

### Build Commands

```bash
just tui-build    # Build release binary
just tui-install  # Install to ~/.cargo/bin
just tui-dev      # Run in dev mode
just tui-dump     # Run dump mode for debugging
```

### API Contract

Expects JSON from `GET /api/agents`:

```json
{
  "agents": [
    {
      "id": "repo-name",
      "directory": "/path/to/repo",
      "repo": "owner/repo",
      "branch": "main",
      "servers": [{
        "type": "vite",
        "port": 5173,
        "pid": 12345,
        "url": "http://localhost:5173",
        "tailscaleUrl": "http://hostname.ts.net:5173"
      }],
      "lastCommit": "commit message",
      "lastCommitHash": "abc123",
      "lastCommitTime": "2 hours ago",
      "lastCommitTimestamp": 1234567890,
      "github": {
        "repoUrl": "https://github.com/...",
        "branchUrl": "https://github.com/.../tree/main",
        "diffUrl": "https://github.com/.../compare/main...branch",
        "commitsUrl": "https://github.com/.../commits/main",
        "lastCommitUrl": "https://github.com/.../commit/abc123"
      },
      "pr": {
        "number": 42,
        "url": "https://github.com/.../pull/42",
        "state": "OPEN|MERGED|CLOSED"
      },
      "status": "active|idle"
    }
  ]
}
```

## Help Overlay

```
┌─ Help ─────────────────────────────────────────────────────────────────────┐
│                                                                            │
│  agent-dashboard TUI                                                       │
│                                                                            │
│  NAVIGATION                        ACTIONS                                 │
│    ↑/↓, j/k, C-p/n  Move            o       Open branch on GitHub         │
│    1-9              Jump to host    d       Open diff vs main             │
│    Tab / S-Tab      Next/Prev host  p       Open PR on GitHub             │
│    gg / G           First / Last    s       Open server (picker)          │
│    Enter            Toggle/Open     e       Open in $EDITOR               │
│                                                                            │
│  SEARCH                            UTILITY                                 │
│    /                Start filter    r / R   Refresh host / all            │
│    Esc              Clear filter    ?       This help                      │
│                                     q       Quit                           │
│                                                                            │
│  Press any key to close...                                                 │
└────────────────────────────────────────────────────────────────────────────┘
```
