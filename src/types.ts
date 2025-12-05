export interface Server {
  type: 'vite' | 'playwright' | 'next' | 'unknown';
  port: number;
  pid: number;
  url: string;
  tailscaleUrl?: string;
}

export interface BeadsStatus {
  open: number;
  inProgress: number;
  closed: number;
  inProgressIssues: string[];
}

export interface PullRequest {
  number: number;
  url: string;
  title: string;
  state: string;
}

export interface AgentInfo {
  id: string;
  directory: string;
  repo: string;
  branch: string;
  pr?: PullRequest;
  servers: Server[];
  beads?: BeadsStatus;
  lastCommit: string;
  lastCommitTime: string;
  status: 'active' | 'idle';
}

export interface ScanResult {
  agents: AgentInfo[];
  scannedAt: string;
  hostname: string;
  tailscaleHostname?: string;
}
