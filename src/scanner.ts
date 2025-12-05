import { execSync } from 'child_process';
import { existsSync, readdirSync } from 'fs';
import { join } from 'path';
import type { AgentInfo, BeadsStatus, GitHubLinks, PullRequest, ScanResult, Server } from './types.js';

const GITS_DIR = process.env.GITS_DIR || join(process.env.HOME || '', 'gits');

function exec(cmd: string, cwd?: string): string {
  try {
    return execSync(cmd, {
      cwd,
      encoding: 'utf-8',
      timeout: 5000,
      stdio: ['pipe', 'pipe', 'pipe']
    }).trim();
  } catch {
    return '';
  }
}

function findAgentDirectories(): string[] {
  if (!existsSync(GITS_DIR)) return [];

  const entries = readdirSync(GITS_DIR, { withFileTypes: true });
  const agentDirs: string[] = [];

  for (const entry of entries) {
    if (!entry.isDirectory()) continue;

    // Match patterns like: project-1, project-2, swing-3, etc.
    if (/-\d+$/.test(entry.name)) {
      const fullPath = join(GITS_DIR, entry.name);
      // Verify it's a git repo
      if (existsSync(join(fullPath, '.git'))) {
        agentDirs.push(fullPath);
      }
    }
  }

  return agentDirs.sort();
}

function getRunningServers(): Map<string, Server[]> {
  const serversByDir = new Map<string, Server[]>();

  // Get all listening TCP ports with their PIDs
  const lsofOutput = exec('lsof -i -P -n 2>/dev/null | grep LISTEN || true');
  if (!lsofOutput) return serversByDir;

  const portToPid = new Map<number, number>();

  for (const line of lsofOutput.split('\n')) {
    const parts = line.split(/\s+/);
    if (parts.length < 9) continue;

    const pid = parseInt(parts[1], 10);
    const portMatch = parts[8]?.match(/:(\d+)$/);
    if (portMatch) {
      const port = parseInt(portMatch[1], 10);
      portToPid.set(port, pid);
    }
  }

  // For each PID, find its working directory
  for (const [port, pid] of portToPid) {
    const cwd = exec(`readlink -f /proc/${pid}/cwd 2>/dev/null`);
    if (!cwd || !cwd.startsWith(GITS_DIR)) continue;

    // Find the agent directory (top-level under gits)
    const relativePath = cwd.slice(GITS_DIR.length + 1);
    const agentName = relativePath.split('/')[0];
    if (!agentName || !/-\d+$/.test(agentName)) continue;

    const agentDir = join(GITS_DIR, agentName);

    // Determine server type from process name
    const cmdline = exec(`cat /proc/${pid}/cmdline 2>/dev/null | tr '\\0' ' '`);
    let type: Server['type'] = 'unknown';
    if (cmdline.includes('vite')) type = 'vite';
    else if (cmdline.includes('playwright')) type = 'playwright';
    else if (cmdline.includes('next')) type = 'next';

    const server: Server = {
      type,
      port,
      pid,
      url: `http://localhost:${port}`
    };

    const existing = serversByDir.get(agentDir) || [];
    existing.push(server);
    serversByDir.set(agentDir, existing);
  }

  return serversByDir;
}

function getGitInfo(dir: string): {
  branch: string;
  repo: string;
  lastCommit: string;
  lastCommitHash: string;
  lastCommitTime: string;
  defaultBranch: string;
} {
  const branch = exec('git branch --show-current', dir) || 'unknown';
  const remoteUrl = exec('git remote get-url origin 2>/dev/null', dir) || '';

  // Parse repo from URL (https://github.com/user/repo.git or git@github.com:user/repo.git)
  let repo = remoteUrl;
  const httpsMatch = remoteUrl.match(/github\.com[/:]([^/]+\/[^/]+?)(?:\.git)?$/);
  if (httpsMatch) repo = httpsMatch[1];

  const lastCommit = exec('git log -1 --format="%s" 2>/dev/null', dir) || '';
  const lastCommitHash = exec('git log -1 --format="%H" 2>/dev/null', dir) || '';
  const lastCommitTime = exec('git log -1 --format="%cr" 2>/dev/null', dir) || '';

  // Try to detect default branch (main or dev)
  const defaultBranch = exec('git symbolic-ref refs/remotes/origin/HEAD 2>/dev/null', dir)
    ?.replace('refs/remotes/origin/', '') || 'main';

  return { branch, repo, lastCommit, lastCommitHash, lastCommitTime, defaultBranch };
}

function getGitHubLinks(repo: string, branch: string, defaultBranch: string, commitHash: string): GitHubLinks | undefined {
  if (!repo || !repo.includes('/')) return undefined;

  const baseUrl = `https://github.com/${repo}`;

  return {
    repoUrl: baseUrl,
    branchUrl: `${baseUrl}/tree/${branch}`,
    diffUrl: `${baseUrl}/compare/${defaultBranch}...${branch}`,
    commitsUrl: `${baseUrl}/commits/${branch}`,
    lastCommitUrl: commitHash ? `${baseUrl}/commit/${commitHash}` : `${baseUrl}/commits/${branch}`
  };
}

function getPRInfo(dir: string): PullRequest | undefined {
  const prJson = exec('gh pr view --json number,url,title,state 2>/dev/null', dir);
  if (!prJson) return undefined;

  try {
    const pr = JSON.parse(prJson);
    return {
      number: pr.number,
      url: pr.url,
      title: pr.title,
      state: pr.state
    };
  } catch {
    return undefined;
  }
}

function getBeadsStatus(dir: string): BeadsStatus | undefined {
  if (!existsSync(join(dir, '.beads'))) return undefined;

  // Get stats
  const statsOutput = exec('bd stats 2>/dev/null', dir);
  if (!statsOutput) return undefined;

  const openMatch = statsOutput.match(/Open:\s+(\d+)/);
  const inProgressMatch = statsOutput.match(/In Progress:\s+(\d+)/);
  const closedMatch = statsOutput.match(/Closed:\s+(\d+)/);

  // Get in-progress issue IDs
  const listOutput = exec('bd list --status=in_progress 2>/dev/null', dir);
  const inProgressIssues: string[] = [];
  if (listOutput) {
    const matches = listOutput.matchAll(/^([\w-]+)\s/gm);
    for (const match of matches) {
      inProgressIssues.push(match[1]);
    }
  }

  return {
    open: parseInt(openMatch?.[1] || '0', 10),
    inProgress: parseInt(inProgressMatch?.[1] || '0', 10),
    closed: parseInt(closedMatch?.[1] || '0', 10),
    inProgressIssues
  };
}

function getTailscaleHostname(): string | undefined {
  const status = exec('tailscale status --json 2>/dev/null');
  if (!status) return undefined;

  try {
    const parsed = JSON.parse(status);
    return parsed.Self?.DNSName?.replace(/\.$/, '');
  } catch {
    return undefined;
  }
}

export function scan(): ScanResult {
  const agentDirs = findAgentDirectories();
  const runningServers = getRunningServers();
  const tailscaleHostname = getTailscaleHostname();
  const hostname = exec('hostname') || 'localhost';

  const agents: AgentInfo[] = [];

  for (const dir of agentDirs) {
    const id = dir.split('/').pop() || '';
    const { branch, repo, lastCommit, lastCommitHash, lastCommitTime, defaultBranch } = getGitInfo(dir);
    const servers = runningServers.get(dir) || [];

    // Add Tailscale URLs to servers
    if (tailscaleHostname) {
      for (const server of servers) {
        server.tailscaleUrl = `http://${tailscaleHostname}:${server.port}`;
      }
    }

    const agent: AgentInfo = {
      id,
      directory: dir,
      repo,
      branch,
      pr: getPRInfo(dir),
      servers,
      beads: getBeadsStatus(dir),
      lastCommit,
      lastCommitHash,
      lastCommitTime,
      github: getGitHubLinks(repo, branch, defaultBranch, lastCommitHash),
      status: servers.length > 0 ? 'active' : 'idle'
    };

    agents.push(agent);
  }

  return {
    agents,
    scannedAt: new Date().toISOString(),
    hostname,
    tailscaleHostname
  };
}
