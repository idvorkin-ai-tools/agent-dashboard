# Agent Dashboard

Central portal for monitoring multi-agent dev sessions.

## Features

- **Auto-discovers** agent directories in `~/gits/` (pattern: `*-N`, e.g., `swing-1`, `swing-2`)
- **Scans running servers** (Vite, Playwright, Next.js) by process inspection
- **Shows git status**: branch, last commit, PR info
- **Beads integration**: open issues, in-progress work
- **Tailscale-friendly**: binds to 0.0.0.0, generates Tailscale URLs

## Usage

```bash
# Start the dashboard (default port 9999)
npm run dev

# Start on different port
npm run dev -- --port=8080

# One-shot scan (output JSON)
npm run scan
```

## API

- `GET /api/agents` - Returns JSON with all discovered agents
- `GET /api/health` - Health check

## Configuration

Environment variables:
- `GITS_DIR` - Directory to scan (default: `~/gits`)

## How It Works

1. Scans `~/gits/` for directories matching `*-N` pattern
2. For each directory:
   - Reads git branch and remote
   - Checks for open PRs via `gh pr view`
   - Scans `/proc` for running servers with matching cwd
   - Reads beads status if `.beads/` exists
3. Serves dashboard on port 9999 with auto-refresh

## Dashboard

The web UI shows:
- Agent ID and status (active/idle based on running servers)
- Current branch
- PR link (if any)
- Running servers with clickable links
- Beads summary (open issues, in-progress)
- Last commit message and time
