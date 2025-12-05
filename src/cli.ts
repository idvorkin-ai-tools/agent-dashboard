#!/usr/bin/env node

import { scan } from './scanner.js';
import { startServer } from './server.js';

const args = process.argv.slice(2);
const command = args[0] || 'serve';

switch (command) {
  case 'scan': {
    const result = scan();
    console.log(JSON.stringify(result, null, 2));
    break;
  }

  case 'serve': {
    const portArg = args.find(a => a.startsWith('--port='));
    const port = portArg ? parseInt(portArg.split('=')[1], 10) : 9999;
    startServer(port);
    break;
  }

  case 'help':
  default: {
    console.log(`
Agent Dashboard - Central portal for multi-agent dev sessions

Usage:
  agent-dashboard [command] [options]

Commands:
  serve [--port=9999]  Start the dashboard server (default)
  scan                 One-shot scan, output JSON to stdout
  help                 Show this help message

Environment:
  GITS_DIR             Directory to scan for agents (default: ~/gits)

Examples:
  agent-dashboard                    # Start server on port 9999
  agent-dashboard serve --port=8080  # Start server on port 8080
  agent-dashboard scan               # Scan and print JSON
`);
    break;
  }
}
