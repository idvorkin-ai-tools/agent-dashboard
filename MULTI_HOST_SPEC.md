# agent-dashboard Multi-Host Web Specification

## Overview

Extend the web dashboard to aggregate and display repositories from multiple agent-dashboard instances running on different hosts (e.g., local machine, macbook, devbox).

## Architecture

```
┌─────────────┐     ┌─────────────┐     ┌─────────────┐
│   Host 1    │     │   Host 2    │     │   Host 3    │
│  (local)    │     │  (macbook)  │     │  (devbox)   │
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

### URL Parameters

Configure hosts via URL query parameters:

```
http://localhost:9999/?hosts=local:9999,macbook.ts.net:9999,devbox.ts.net:9999
```

Or with explicit names:

```
http://localhost:9999/?hosts=local|localhost:9999,mac|macbook.ts.net:9999,dev|devbox.ts.net:9999
```

### LocalStorage Persistence

```javascript
// Saved automatically when hosts configured
localStorage.setItem('agent-dashboard-hosts', JSON.stringify([
  { name: 'local', url: 'http://localhost:9999' },
  { name: 'macbook', url: 'http://macbook.ts.net:9999' },
  { name: 'devbox', url: 'http://devbox.ts.net:9999' }
]));
```

## Web UI Layout

### Header

```
┌────────────────────────────────────────────────────────────────────────────────┐
│ Agent Dashboard    [local ▼] [macbook ●] [devbox ✗]    [+ Add Host] [Settings] │
└────────────────────────────────────────────────────────────────────────────────┘
```

- Tabs/buttons for each host
- `●` = active/connected, `✗` = disconnected
- Current host highlighted
- "All" view option to show aggregated

### Host Status Indicators

| Icon | Meaning |
|------|---------|
| `●` (green) | Connected, data fresh |
| `○` (yellow) | Connected, data stale (>60s) |
| `✗` (red) | Disconnected |
| `⟳` (blue) | Currently refreshing |

### Repository List

When viewing single host:
```
┌─────────────────────────────────────────────────────────────────────────────┐
│ Name              Branch          Status    Servers          Actions        │
├─────────────────────────────────────────────────────────────────────────────┤
│ agent-dashboard   main            idle      -                [↗] [⚙]       │
│ blog              feat/new        ● active  vite:5173        [↗] [🌐] [⚙]  │
│ settings          main            idle      -                [↗] [⚙]       │
└─────────────────────────────────────────────────────────────────────────────┘
```

When viewing "All Hosts":
```
┌─────────────────────────────────────────────────────────────────────────────┐
│ Host      Name              Branch      Status    Servers      Actions      │
├─────────────────────────────────────────────────────────────────────────────┤
│ local     agent-dashboard   main        idle      -            [↗] [⚙]     │
│ local     blog              feat/new    ● active  vite:5173    [↗] [🌐]    │
│ macbook   settings          main        idle      -            [↗] [⚙]     │
│ macbook   my-project        dev         ● active  next:3000    [↗] [🌐]    │
│ devbox    ml-experiment     train       ● active  jupyter:8888 [↗] [🌐]    │
└─────────────────────────────────────────────────────────────────────────────┘
```

### Action Buttons

| Icon | Action | Description |
|------|--------|-------------|
| `↗` | GitHub | Open repository on GitHub |
| `🌐` | Server | Open dev server URL |
| `⚙` | Details | Expand details panel |
| `📋` | PR | Open pull request |

## API Endpoints

### Existing (No Changes)

```
GET /api/agents     → ScanResult for this host
GET /api/health     → { status: 'ok', timestamp: '...' }
```

### New: Host Metadata

```
GET /api/host

Response:
{
  "name": "macbook",           // Hostname or custom name
  "version": "1.0.0",          // agent-dashboard version
  "uptime": 3600,              // Seconds since start
  "lastScan": "ISO8601",       // Last scan timestamp
  "scanInterval": 30000,       // Scan interval in ms
  "repoCount": 15              // Number of repos tracked
}
```

### New: Force Refresh

```
POST /api/refresh

Response:
{
  "status": "ok",
  "scannedAt": "ISO8601",
  "duration": 1234            // Scan duration in ms
}
```

## Client-Side Implementation

### Host Manager Class

```typescript
interface HostConfig {
  name: string;
  url: string;
}

interface HostState {
  config: HostConfig;
  status: 'connected' | 'disconnected' | 'refreshing';
  lastSeen: Date | null;
  data: ScanResult | null;
  error: string | null;
}

class HostManager {
  private hosts: Map<string, HostState>;

  addHost(config: HostConfig): void;
  removeHost(name: string): void;
  refreshHost(name: string): Promise<void>;
  refreshAll(): Promise<void>;
  getAggregatedAgents(): AgentInfo[];  // All hosts combined
}
```

### Polling Strategy

1. Poll each host independently every 30 seconds
2. Stagger requests to avoid thundering herd
3. Mark host as disconnected after 3 consecutive failures
4. Exponential backoff on failures (30s → 60s → 120s)
5. Reset backoff on successful connection

### CORS Handling

Each agent-dashboard server needs CORS headers:

```typescript
// Add to server.ts
app.use((req, res, next) => {
  res.header('Access-Control-Allow-Origin', '*');
  res.header('Access-Control-Allow-Methods', 'GET, POST');
  res.header('Access-Control-Allow-Headers', 'Content-Type');
  next();
});
```

## UI Features

### Host Switcher

- Dropdown or tab bar for switching between hosts
- Keyboard shortcuts: `1`, `2`, `3` to switch
- `A` for "All Hosts" aggregate view

### Settings Modal

```
┌─ Settings ──────────────────────────────────────────────────────────────────┐
│                                                                             │
│ Hosts:                                                                      │
│ ┌─────────────────────────────────────────────────────────────────────────┐ │
│ │ Name         URL                              Status        Actions     │ │
│ │ local        http://localhost:9999            ● Connected   [Test] [✗]  │ │
│ │ macbook      http://macbook.ts.net:9999       ● Connected   [Test] [✗]  │ │
│ │ devbox       http://devbox.ts.net:9999        ✗ Disconnected[Test] [✗]  │ │
│ └─────────────────────────────────────────────────────────────────────────┘ │
│                                                                             │
│ [+ Add Host]                                                                │
│                                                                             │
│ Refresh interval: [30s ▼]                                                   │
│                                                                             │
│                                           [Cancel]  [Save]                  │
└─────────────────────────────────────────────────────────────────────────────┘
```

### Add Host Modal

```
┌─ Add Host ──────────────────────────────────────────────────────────────────┐
│                                                                             │
│ Name:  [devbox_______________]                                              │
│                                                                             │
│ URL:   [http://devbox.ts.net:9999_______]                                   │
│                                                                             │
│ [Test Connection]   Status: ● Connected (15 repos)                          │
│                                                                             │
│                                           [Cancel]  [Add]                   │
└─────────────────────────────────────────────────────────────────────────────┘
```

### Keyboard Shortcuts

| Key | Action |
|-----|--------|
| `1-9` | Switch to host N |
| `a` | Show all hosts aggregate |
| `r` | Refresh current host |
| `R` | Refresh all hosts |
| `/` | Focus search/filter |
| `?` | Show keyboard shortcuts help |
| `s` | Open settings |

## Data Aggregation

### All Hosts View

When showing all hosts:
1. Combine all agents from all connected hosts
2. Add `hostName` field to each agent for display
3. Sort by: host name, then repo name (default)
4. Allow sorting by any column

### Conflict Resolution

If same repo name exists on multiple hosts:
- Show both, distinguished by host column
- In aggregate view, group by host first

## Error States

### Host Disconnected

```
┌─────────────────────────────────────────────────────────────────────────────┐
│ ⚠ Cannot connect to macbook (http://macbook.ts.net:9999)                   │
│   Last seen: 5 minutes ago                                                  │
│   Error: Connection refused                                                 │
│   [Retry Now]  [Remove Host]                                               │
└─────────────────────────────────────────────────────────────────────────────┘
```

### Partial Data

If some hosts connected, some not:
- Show connected hosts' data normally
- Show warning banner for disconnected hosts
- Still allow interaction with available data

## URL Scheme

### Share Configuration

Generate shareable URL with host config:

```
https://localhost:9999/?config=eyJob3N0cyI6W3sibmFtZSI6ImxvY2FsIi...
```

Base64-encoded JSON config for easy sharing.

### Deep Links

Link directly to a specific repo on a specific host:

```
https://localhost:9999/?host=macbook&repo=blog
```

## Mobile Considerations

- Responsive layout: single column on narrow screens
- Touch-friendly action buttons
- Swipe between hosts
- Pull-to-refresh on each host view
