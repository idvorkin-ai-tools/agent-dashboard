import express from 'express';
import { join, dirname } from 'path';
import { fileURLToPath } from 'url';
import { watch } from 'fs';
import { scan, type ScanResult } from './scanner.js';

const __dirname = dirname(fileURLToPath(import.meta.url));

// Track connected SSE clients for live reload
const liveReloadClients: express.Response[] = [];

// Background scan cache
let cachedResult: ScanResult | null = null;
let scanInProgress = false;
const REFRESH_INTERVAL_ACTIVE_MS = 30000; // 30 seconds when changes detected
const REFRESH_INTERVAL_IDLE_MS = 300000; // 5 minutes when no changes
const HOURLY_RELOAD_MS = 60 * 60 * 1000; // 1 hour
const serverStartTime = Date.now();
let lastCommitHashes: Map<string, string> = new Map();
let currentRefreshInterval = REFRESH_INTERVAL_ACTIVE_MS;

async function backgroundScan(): Promise<void> {
  if (scanInProgress) return;
  scanInProgress = true;
  try {
    const result = await scan();
    cachedResult = result;

    // Check if any commit hashes changed
    const newHashes = new Map<string, string>();
    let hasChanges = false;

    for (const agent of result.agents) {
      newHashes.set(agent.id, agent.lastCommitHash);
      if (lastCommitHashes.get(agent.id) !== agent.lastCommitHash) {
        hasChanges = true;
      }
    }

    // Check for new or removed repos
    if (newHashes.size !== lastCommitHashes.size) {
      hasChanges = true;
    }

    lastCommitHashes = newHashes;

    // Adjust scan interval based on activity
    const oldInterval = currentRefreshInterval;
    currentRefreshInterval = hasChanges ? REFRESH_INTERVAL_ACTIVE_MS : REFRESH_INTERVAL_IDLE_MS;

    if (oldInterval !== currentRefreshInterval) {
      console.log(`[background] ${hasChanges ? 'Changes detected' : 'No changes'}, scan interval: ${currentRefreshInterval / 1000}s`);
    }

    console.log(`[background] Scan complete: ${result.agents.length} agents`);
  } catch (error) {
    console.error('[background] Scan error:', error);
  } finally {
    scanInProgress = false;
  }
}

export function startServer(port: number = 9999): void {
  const app = express();

  // CORS - allow cross-origin requests from any origin (for multi-host dashboard)
  app.use((_req, res, next) => {
    res.header('Access-Control-Allow-Origin', '*');
    res.header('Access-Control-Allow-Methods', 'GET, POST, OPTIONS');
    res.header('Access-Control-Allow-Headers', 'Content-Type');
    next();
  });

  // Serve static files from public directory
  const publicDir = join(__dirname, '..', 'public');
  app.use(express.static(publicDir));

  // Start background scanning with dynamic interval
  const scheduleNextScan = () => {
    setTimeout(async () => {
      await backgroundScan();
      scheduleNextScan();
    }, currentRefreshInterval);
  };

  backgroundScan(); // Initial scan
  scheduleNextScan();

  // Force page reload hourly to clean up any client-side memory leaks
  setInterval(() => {
    console.log('[hourly] Triggering client reload for memory cleanup');
    for (const client of liveReloadClients) {
      client.write('data: reload\n\n');
    }
  }, HOURLY_RELOAD_MS);

  // API endpoint - returns cached result immediately
  app.get('/api/agents', async (_req, res) => {
    try {
      if (cachedResult) {
        res.json(cachedResult);
      } else {
        // First request before initial scan completes - wait for scan
        const result = await scan();
        cachedResult = result;
        res.json(result);
      }
    } catch (error) {
      res.status(500).json({ error: String(error) });
    }
  });

  // Health check
  app.get('/api/health', (_req, res) => {
    res.json({ status: 'ok', timestamp: new Date().toISOString() });
  });

  // Host metadata
  app.get('/api/host', (_req, res) => {
    const hostname = cachedResult?.hostname || 'unknown';
    res.json({
      name: hostname,
      version: '1.0.0',
      uptime: Math.floor((Date.now() - serverStartTime) / 1000),
      lastScan: cachedResult?.scannedAt || null,
      scanInterval: currentRefreshInterval,
      repoCount: cachedResult?.agents?.length || 0
    });
  });

  // Force refresh
  app.post('/api/refresh', async (_req, res) => {
    const start = Date.now();
    try {
      const result = await scan();
      cachedResult = result;
      res.json({
        status: 'ok',
        scannedAt: result.scannedAt,
        duration: Date.now() - start
      });
    } catch (error) {
      res.status(500).json({ status: 'error', error: String(error) });
    }
  });

  // Live reload SSE endpoint
  app.get('/api/live-reload', (req, res) => {
    res.setHeader('Content-Type', 'text/event-stream');
    res.setHeader('Cache-Control', 'no-cache');
    res.setHeader('Connection', 'keep-alive');
    res.flushHeaders();

    liveReloadClients.push(res);

    req.on('close', () => {
      const idx = liveReloadClients.indexOf(res);
      if (idx !== -1) liveReloadClients.splice(idx, 1);
    });
  });

  app.listen(port, '0.0.0.0', () => {
    console.log(`Agent Dashboard running at:`);
    console.log(`  Local:     http://localhost:${port}`);

    // Try to get Tailscale hostname
    try {
      const { execSync } = require('child_process');
      const status = execSync('tailscale status --json 2>/dev/null', { encoding: 'utf-8' });
      const parsed = JSON.parse(status);
      const dnsName = parsed.Self?.DNSName?.replace(/\.$/, '');
      if (dnsName) {
        console.log(`  Tailscale: http://${dnsName}:${port}`);
      }
    } catch {
      // Tailscale not available
    }

    console.log(`\nAPI: GET /api/agents`);
    console.log(`Live reload enabled - watching for file changes`);

    // Watch for file changes in public and src directories
    const watchDirs = [
      join(__dirname, '..', 'public'),
      join(__dirname, '..', 'src')
    ];

    for (const dir of watchDirs) {
      try {
        watch(dir, { recursive: true }, (eventType, filename) => {
          if (filename && !filename.endsWith('.swp')) {
            console.log(`File changed: ${filename} - triggering reload`);
            for (const client of liveReloadClients) {
              client.write('data: reload\n\n');
            }
          }
        });
      } catch {
        // Directory might not exist
      }
    }
  });
}
