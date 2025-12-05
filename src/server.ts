import express from 'express';
import { join, dirname } from 'path';
import { fileURLToPath } from 'url';
import { scan } from './scanner.js';

const __dirname = dirname(fileURLToPath(import.meta.url));

export function startServer(port: number = 9999): void {
  const app = express();

  // Serve static files from public directory
  const publicDir = join(__dirname, '..', 'public');
  app.use(express.static(publicDir));

  // API endpoint
  app.get('/api/agents', (_req, res) => {
    try {
      const result = scan();
      res.json(result);
    } catch (error) {
      res.status(500).json({ error: String(error) });
    }
  });

  // Health check
  app.get('/api/health', (_req, res) => {
    res.json({ status: 'ok', timestamp: new Date().toISOString() });
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
  });
}
