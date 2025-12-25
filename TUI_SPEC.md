# agent-dashboard TUI Specification

## Overview

Rust-based TUI for managing multiple development environments across multiple hosts.
Connects to agent-dashboard servers via their HTTP API.

## Multi-Host Architecture

### Configuration

```toml
# ~/.config/agent-dashboard/hosts.toml
[[hosts]]
name = "local"
url = "http://localhost:9999"
shortcut = "1"

[[hosts]]
name = "macbook"
url = "http://macbook.tail1234.ts.net:9999"
shortcut = "2"

[[hosts]]
name = "devbox"
url = "http://devbox.tail1234.ts.net:9999"
shortcut = "3"
```

### Host Switching

- `1`, `2`, `3` - Switch between configured hosts
- `Tab` - Cycle through hosts
- Current host shown in title bar

## Layout

### Horizontal (Wide Terminal)

```
┌─ Repositories [macbook] ───────────────┬─ Details ─────────────────────────────┐
│ ▶ agent-dashboard      main      idle  │ Branch: main                          │
│   blog                 feat/x  ● active│ Commit: 2h ago "feat: Add feature"    │
│   settings             main      idle  │                                       │
│   claude-code          dev     ● active│ Servers:                              │
│                                        │   ● vite  http://localhost:5173       │
│                                        │   ● next  http://localhost:3000       │
│                                        │                                       │
│                                        │ PR #42: "Add dark mode"               │
│                                        │                                       │
│                                        │ Beads: 3 open, 1 in-progress          │
│                                        │   → WIP: issue-123, issue-456         │
│                                        │                                       │
│                                        │ Links:                                │
│                                        │   [r]epo [d]iff [c]ommits [p]r        │
└────────────────────────────────────────┴───────────────────────────────────────┘
 filter> _    1:local 2:macbook 3:devbox │ ↑↓:nav Enter:term s:server ?:help
```

### Vertical (Narrow Terminal)

```
┌─ Repositories [macbook] ───────────────────────────────────────────────────────┐
│ ▶ agent-dashboard      main      idle                                          │
│   blog                 feat/x  ● active                                        │
│   settings             main      idle                                          │
└────────────────────────────────────────────────────────────────────────────────┘
┌─ Details ──────────────────────────────────────────────────────────────────────┐
│ Branch: feat/x  │  Commit: 2h ago "feat: Add feature"                          │
│ Servers: ● vite :5173  ● next :3000  │  PR #42: "Add dark mode"                │
└────────────────────────────────────────────────────────────────────────────────┘
 filter> _    1:local 2:macbook 3:devbox │ ↑↓:nav Enter:term s:server ?:help
```

Auto-switch to vertical when `terminal_width / 2 < list_width_needed`.
Manual toggle via `C-l`.

## Display Format

### List Columns

| Column | Color | Description |
|--------|-------|-------------|
| Marker | White | `▶` = selected |
| Name | Light Green | Repository directory name |
| Branch | Light Magenta | Current git branch |
| Status | Cyan/Gray | `● active` (has servers) or `idle` |

### Status Indicators

- `●` (cyan, bold) = Has running servers
- No indicator = Idle repository

### Row Highlighting

- **Selected**: Dark gray background + bold
- **Active (has servers)**: Subtle green background `rgb(30, 50, 30)`
- **With PR**: Subtle blue background `rgb(30, 30, 50)`

## Navigation

| Key | Action |
|-----|--------|
| `↑` / `C-p` | Move selection up |
| `↓` / `C-n` | Move selection down |
| `Home` / `g` `g` | Jump to first |
| `End` / `G` | Jump to last |
| `C-u` | Page up |
| `C-d` | Page down |

## Actions

### Primary Actions

| Key | Action | Description |
|-----|--------|-------------|
| `Enter` | Open terminal | Open terminal cd'd to repo directory |
| `e` | Open editor | Open repo in $EDITOR (nvim) |
| `s` | Open server | Open first server URL in browser |
| `S` | Server menu | Show server picker if multiple |

### GitHub Actions

| Key | Action | URL |
|-----|--------|-----|
| `o` | Open repo | `github.com/{repo}` |
| `d` | Open diff | `github.com/{repo}/compare/main...{branch}` |
| `c` | Open commits | `github.com/{repo}/commits/{branch}` |
| `p` | Open PR | PR URL (if exists) |
| `C` | Open last commit | Direct link to HEAD commit |

### Host Actions

| Key | Action |
|-----|--------|
| `1` | Switch to host 1 |
| `2` | Switch to host 2 |
| `3` | Switch to host 3 |
| `Tab` | Cycle to next host |
| `S-Tab` | Cycle to previous host |

### Utility Actions

| Key | Action |
|-----|--------|
| `r` | Refresh | Force re-scan on current host |
| `R` | Refresh all | Force re-scan on all hosts |
| `C-l` | Toggle layout | Horizontal ↔ Vertical |
| `?` / `F1` | Help | Show help overlay |
| `Esc` / `C-c` / `q` | Quit | Exit TUI |

## Filtering

- Type printable characters to filter by repo name or branch
- `Backspace` to delete characters
- `Esc` clears filter (if filter active) or quits (if empty)
- Filter shown as `filter> query_` in chrome line

## Details Pane

For selected repository, display:

### Section 1: Git Info
```
Branch: {branch}
Commit: {relative_time} "{commit_message}"
```

### Section 2: Servers (if any)
```
Servers:
  ● {type}  {url}
  ● {type}  {url}
```

### Section 3: PR (if exists)
```
PR #{number}: "{title}"
  State: {state}
```

### Section 4: Beads (if exists)
```
Beads: {open} open, {in_progress} in-progress
  → WIP: {issue_ids...}
```

### Section 5: Quick Links
```
Links: [r]epo [d]iff [c]ommits [p]r
```

## Chrome (UI Frame)

### Title Bar
```
─ Repositories [{host_name}] ─
```

### Status Line
```
filter> {query}_    1:{host1} 2:{host2} 3:{host3} │ ↑↓:nav Enter:term s:server ?:help
```

- Host shortcuts shown with current host highlighted
- If host disconnected, show `✗` marker

## Terminal Integration

### Opening Terminal (Enter)

Execute via system:
- macOS: `open -a Terminal.app {path}` or iTerm2 AppleScript
- Linux: Detect terminal emulator, use appropriate command

### Opening Browser

Execute via system:
- macOS: `open {url}`
- Linux: `xdg-open {url}`

### Opening Editor

Execute: `$EDITOR {path}` or fallback to `nvim {path}`

## Error Handling

- Host unreachable: Show `✗` in host list, gray out if selected
- Retry connection every 30 seconds
- Show last successful scan time in details pane

## Help Overlay

```
┌─ Help ─────────────────────────────────────────────────────────────────────────┐
│                                                                                │
│  agent-dashboard TUI - Multi-host Development Environment Manager              │
│                                                                                │
│  NAVIGATION                          GITHUB                                    │
│    ↑/↓, C-p/C-n   Move selection       o    Open repository                   │
│    Enter          Open terminal        d    Open diff (branch vs main)        │
│    e              Open in editor       c    Open commits                       │
│    s              Open server          p    Open pull request                  │
│    Type           Filter repos         C    Open last commit                   │
│                                                                                │
│  HOSTS                               UTILITY                                   │
│    1/2/3          Switch host          r    Refresh current host              │
│    Tab            Cycle hosts          R    Refresh all hosts                  │
│                                        C-l  Toggle layout                      │
│                                        ?    This help                          │
│                                        q    Quit                               │
│                                                                                │
│  Press any key to close...                                                     │
└────────────────────────────────────────────────────────────────────────────────┘
```

## Technical Notes

### Dependencies (Rust)

- `ratatui` - TUI framework
- `crossterm` - Terminal handling
- `reqwest` - HTTP client (async)
- `tokio` - Async runtime
- `serde` / `serde_json` - JSON parsing
- `toml` - Config parsing
- `dirs` - Config directory location
- `open` - Cross-platform URL/path opening

### Data Flow

1. TUI starts, loads `~/.config/agent-dashboard/hosts.toml`
2. Connects to first configured host via `GET /api/agents`
3. Polls each host every 30 seconds for updates
4. User actions trigger immediate refresh or browser/terminal opens

### API Contract

Expects JSON from `/api/agents`:
```json
{
  "agents": [...],
  "scannedAt": "ISO8601",
  "hostname": "string",
  "tailscaleHostname": "string|null"
}
```

Each agent:
```json
{
  "id": "repo-name",
  "directory": "/path/to/repo",
  "repo": "owner/repo",
  "branch": "main",
  "pr": { "number": 42, "url": "...", "title": "...", "state": "OPEN" },
  "servers": [{ "type": "vite", "port": 5173, "url": "http://...", "tailscaleUrl": "..." }],
  "beads": { "open": 3, "inProgress": 1, "inProgressIssues": ["id1", "id2"] },
  "lastCommit": "commit message",
  "lastCommitTime": "2 hours ago",
  "github": { "repoUrl": "...", "diffUrl": "...", "commitsUrl": "...", "lastCommitUrl": "..." },
  "status": "active|idle"
}
```
