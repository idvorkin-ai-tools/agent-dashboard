# agent-dashboard TUI Specification

## Overview

Rust-based TUI for managing multiple development environments across multiple hosts.
Connects to agent-dashboard servers via their HTTP API.

## Layout

Single-pane stacked view with collapsible host sections:

```
┌─ agent-dashboard ────────────────────────────────────────────────────────────┐
│ ▼ c-5001 (5 repos, 2 active) ●                                               │
│     agent-dashboard    main         ● jekyll:4000     fix: stuff      2h ago │
│   ▶ blog               feat/new     ● vite:5173       feat: add       1h ago │
│     settings           main                           chore: update   3h ago │
│   ▼ Stale with Servers (1)                                                   │
│       old-project      main         ● next:3000       update          3d ago │
│   ▶ Stale Repos (8)                                                          │
│                                                                              │
│ ▼ c-5002 (3 repos, 0 active) ●                                               │
│     my-project         main                           init            1d ago │
│     other-repo         dev                            wip             2d ago │
│   ▶ Stale Repos (5)                                                          │
│                                                                              │
│ ▶ c-5003 (disconnected) ✗                                                    │
│                                                                              │
├──────────────────────────────────────────────────────────────────────────────┤
│ filter> _                               ↑↓:nav  Enter:term  o:open  ?:help   │
└──────────────────────────────────────────────────────────────────────────────┘
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

```json
// ~/.config/agent-dashboard/hosts.json
[
  { "name": "c-5001", "url": "http://c-5001:9999" },
  { "name": "c-5002", "url": "http://c-5002:9999" },
  { "name": "c-5003", "url": "http://c-5003:9999" },
  { "name": "c-5004", "url": "http://c-5004:9999" }
]
```

## Display Columns

| Column | Description |
|--------|-------------|
| Name | Repo directory name |
| Branch | Current git branch |
| Server | Running server type:port (if any) |
| Commit | Last commit message (truncated) |
| Time | Relative time of last commit |

## Status Indicators

| Indicator | Meaning |
|-----------|---------|
| `●` (green) | Connected host / has running server |
| `●` (yellow) | Connecting |
| `✗` (red) | Disconnected |
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
| `o` | Open in browser | GitHub branch page |
| `s` | Open server | First server URL in browser |
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

- Type to filter repos by name or branch
- `Backspace` to delete
- `Esc` clears filter (or quits if empty)
- Filter shown in status bar: `filter> query_`

## Polling

| Condition | Interval |
|-----------|----------|
| Host connected | 10 seconds |
| Host disconnected | 60 seconds |

## Terminal Integration

### Open Terminal (Enter)
- macOS: `open -a Terminal.app {path}` or iTerm AppleScript
- Linux: Detect terminal, spawn with cd

### Open Browser (o, s)
- macOS: `open {url}`
- Linux: `xdg-open {url}`

### Open Editor (e)
- `$EDITOR {path}` or `nvim {path}`

## Technical Notes

### Dependencies (Rust)

- `ratatui` - TUI framework
- `crossterm` - Terminal handling
- `reqwest` - HTTP client (async)
- `tokio` - Async runtime
- `serde` / `serde_json` - JSON parsing
- `dirs` - Config directory
- `open` - Cross-platform URL/path opening

### API Contract

Expects JSON from `GET /api/agents`:

```json
{
  "agents": [
    {
      "id": "repo-name",
      "directory": "/path/to/repo",
      "branch": "main",
      "servers": [{ "type": "vite", "port": 5173, "url": "http://..." }],
      "lastCommit": "commit message",
      "lastCommitTime": "2 hours ago",
      "lastCommitTimestamp": 1234567890,
      "github": { "branchUrl": "https://github.com/..." },
      "status": "active|idle"
    }
  ],
  "hostname": "string",
  "scannedAt": "ISO8601"
}
```

## Help Overlay

```
┌─ Help ─────────────────────────────────────────────────────────────────────┐
│                                                                            │
│  agent-dashboard TUI                                                       │
│                                                                            │
│  NAVIGATION                        ACTIONS                                 │
│    ↑/↓, j/k     Move selection       Enter   Open terminal                │
│    1-9          Jump to host         o       Open in browser              │
│    Tab          Next host            s       Open server                  │
│    gg / G       First / Last         e       Open in editor               │
│    Type         Filter repos                                              │
│                                                                            │
│  UTILITY                                                                   │
│    r / R        Refresh / Refresh all                                     │
│    ?            This help                                                  │
│    q            Quit                                                       │
│                                                                            │
│  Press any key to close...                                                 │
└────────────────────────────────────────────────────────────────────────────┘
```
