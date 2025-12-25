# agent-dashboard

Multi-host development environment dashboard with web UI and Rust TUI.

## Important

**When modifying agent-dashboard, update the relevant spec files:**
- `TUI_SPEC.md` - Rust TUI behavior and keybindings
- `MULTI_HOST_SPEC.md` - Multi-host web features

The specs document the *what* (behavior rules), not the *how* (implementation).

## Components

### Web Server (TypeScript/Node.js)
- `src/server.ts` - Express server with API endpoints
- `src/scanner.ts` - Git repo discovery and process scanning
- `src/cli.ts` - CLI entry point
- `public/index.html` - Web dashboard UI

### TUI (Rust) - To Be Implemented
- Location: `tui/` directory
- Uses ratatui for terminal UI
- Connects to multiple agent-dashboard servers via HTTP API

## Commands

```bash
# Development
just dev              # Start server with ts-node
just build            # Build TypeScript

# Production
just serve            # Start built server

# One-shot
just scan             # Output JSON scan to stdout
```

## API Endpoints

```
GET /api/agents       # List all discovered repos with status
GET /api/health       # Health check
GET /api/host         # Host metadata (name, version, uptime)
POST /api/refresh     # Force immediate re-scan
```

## Testing

```bash
# Test API
curl http://localhost:9999/api/agents | jq .

# Test health
curl http://localhost:9999/api/health
```

## Environment

- `GITS_DIR` - Directory to scan for git repos (default: ~/gits)
- Default port: 9999
