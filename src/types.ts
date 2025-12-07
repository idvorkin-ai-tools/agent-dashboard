export interface Server {
  type: 'vite' | 'playwright' | 'next' | 'jekyll' | 'unknown';
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

export interface GitHubLinks {
  repoUrl: string;
  branchUrl: string;
  diffUrl: string;        // Compare branch to default branch
  commitsUrl: string;     // Recent commits on branch
  lastCommitUrl: string;  // Direct link to last commit
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
  lastCommitHash: string;
  lastCommitTime: string;
  github?: GitHubLinks;
  status: 'active' | 'idle';
}

export interface ScanResult {
  agents: AgentInfo[];
  scannedAt: string;
  hostname: string;
  tailscaleHostname?: string;
}
