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
const REFRESH_INTERVAL_MS = 30000; // 30 seconds

async function backgroundScan(): Promise<void> {
  if (scanInProgress) return;
  scanInProgress = true;
  try {
    const result = await scan();
    cachedResult = result;
    console.log(`[background] Scan complete: ${result.agents.length} agents`);
  } catch (error) {
    console.error('[background] Scan error:', error);
  } finally {
    scanInProgress = false;
  }
}

export function startServer(port: number = 9999): void {
  const app = express();

  // Serve static files from public directory
  const publicDir = join(__dirname, '..', 'public');
  app.use(express.static(publicDir));

  // Start background scanning
  backgroundScan(); // Initial scan
  setInterval(backgroundScan, REFRESH_INTERVAL_MS);

  // API endpoint - returns cached result immediately
  app.get('/api/agents', (_req, res) => {
    try {
      if (cachedResult) {
        res.json(cachedResult);
      } else {
        // First request before initial scan completes - do sync scan
        const result = scan();
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
