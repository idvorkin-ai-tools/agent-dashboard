# agent-dashboard Multi-Host Web Specification

## Overview

Web dashboard that connects to multiple agent-dashboard instances running on different hosts, displaying each as a collapsible section.

## Architecture

```
┌─────────────┐     ┌─────────────┐     ┌─────────────┐
│   Host 1    │     │   Host 2    │     │   Host 3    │
│  (c-5001)   │     │  (c-5002)   │     │  (c-5003)   │
│  :9999      │     │  :9999      │     │  :9999      │
└─────┬───────┘     └─────┬───────┘     └─────┬───────┘
      │                   │                   │
      └───────────────────┼───────────────────┘
                          │
                    ┌─────▼─────┐
                    │  Browser  │
                    │    UI     │
                    └───────────┘
```

The browser connects directly to each host's agent-dashboard API.

## Configuration

### LocalStorage Persistence

```javascript
// Key: 'agent-dashboard-hosts'
[
  { name: 'c-5001', url: 'http://c-5001:9999' },
  { name: 'c-5002', url: 'http://c-5002:9999' },
  { name: 'c-5003', url: 'http://c-5003:9999' },
  { name: 'c-5004', url: 'http://c-5004:9999' }
]
```

### Default Hosts

If no localStorage config exists, defaults to `c-5001` through `c-5004` on port 9999.

## Web UI Layout

### Header

```
┌────────────────────────────────────────────────────────────────────────────────┐
│ Agent Dashboard    [Expand All] [Collapse All] [Refresh All] [Settings]        │
└────────────────────────────────────────────────────────────────────────────────┘
```

### Host Sections (Stacked Vertically)

Each host is a collapsible section:

```
┌─ c-5001 (http://c-5001:9999) ────────────── 5 repos, 2 active │ Last: 5s ago ● Connected ─┐
│ ┌─────────────────────────────────────────────────────────────────────────────────────────┐ │
│ │ Agent              Branch       PR    Servers           Beads         Last Commit       │ │
│ │ agent-dashboard    main         -     jekyll:4000       -             fix: stuff (2h)   │ │
│ │ blog               feat/new     #42   vite:5173         3 open        feat: new (1h)    │ │
│ └─────────────────────────────────────────────────────────────────────────────────────────┘ │
│ ┌─ Stale with Servers (1) ────────────────────────────────────────────────────────────────┐ │
│ │ old-project        main         -     next:3000         -             update (3d)       │ │
│ └─────────────────────────────────────────────────────────────────────────────────────────┘ │
│ ▶ Stale Repos (12)                                                                          │
└─────────────────────────────────────────────────────────────────────────────────────────────┘
```

### Host Status Indicators

| Indicator | Meaning |
|-----------|---------|
| `●` (green) | Connected |
| `●` (yellow, pulsing) | Connecting |
| `●` (red) | Disconnected |

### Repository Sections (per host)

1. **Fresh repos** - Commits < 1 day old (always shown)
2. **Stale with Servers** - Commits >= 1 day BUT has running servers (expanded by default, green tint)
3. **Stale Repos** - Commits >= 1 day, no servers (collapsed by default)

### Table Columns

| Column | Description |
|--------|-------------|
| Agent | Repo name with status dot (green=active, gray=idle) |
| Branch | Clickable link to GitHub diff (branch vs main) |
| PR | PR number if exists, links to GitHub PR |
| Servers | Server type and port, clickable links |
| Beads | Open/WIP count if beads enabled |
| Last Commit | Commit message + relative time |

### Host Ordering

- Connected hosts appear first
- Connecting hosts in middle
- Disconnected hosts pushed to bottom

## API Endpoints

### Existing

```
GET /api/agents     → ScanResult with all repos
GET /api/health     → { status: 'ok', timestamp: '...' }
```

### Host Metadata

```
GET /api/host

Response:
{
  "name": "hostname",
  "version": "1.0.0",
  "uptime": 3600,
  "lastScan": "ISO8601",
  "scanInterval": 30000,    // Current interval (30s or 300s)
  "repoCount": 15
}
```

### Force Refresh

```
POST /api/refresh

Response:
{
  "status": "ok",
  "scannedAt": "ISO8601",
  "duration": 1234
}
```

## Polling Strategy

### Client → Host Polling

| Condition | Interval |
|-----------|----------|
| Host connected | 10 seconds |
| Host disconnected | 60 seconds |

### Server Background Scan

| Condition | Interval |
|-----------|----------|
| Changes detected (new commits) | 30 seconds |
| No changes | 5 minutes |

### Rate Limit Optimization

- Skip `gh pr view` for repos on main/master branches
- Use `ss` command as fallback when `lsof` unavailable

## Keyboard Shortcuts

| Key | Action |
|-----|--------|
| `e` | Expand all hosts |
| `c` | Collapse all hosts |
| `r` | Refresh all hosts |
| `s` | Open settings modal |
| `Esc` | Close settings modal |

## Settings Modal

```
┌─ Configure Hosts ───────────────────────────────────────────────────┐
│                                                                     │
│ [Name input] [URL input] [Remove]                                   │
│ [Name input] [URL input] [Remove]                                   │
│ [Name input] [URL input] [Remove]                                   │
│                                                                     │
│ [+ Add Host]                                                        │
│                                                                     │
│ Enter host URLs like http://hostname:9999                           │
│                                                                     │
│ [Reset to Defaults]                    [Cancel] [Save]              │
└─────────────────────────────────────────────────────────────────────┘
```

## CORS

Server includes CORS headers for cross-origin requests:

```
Access-Control-Allow-Origin: *
Access-Control-Allow-Methods: GET, POST, OPTIONS
Access-Control-Allow-Headers: Content-Type
```

## Server Detection

Detects running dev servers via:
1. `lsof -i -P -n` (primary)
2. `ss -tlnp` (fallback when lsof unavailable)

Maps process working directory to agent directories.

Detected server types:
- vite
- next
- playwright
- jekyll (via ruby/bundle process)
