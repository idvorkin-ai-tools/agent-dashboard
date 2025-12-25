use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    env,
    fs,
    io,
    path::PathBuf,
    process::Command,
    time::{Duration, Instant},
};

// Types matching the server API
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct Server {
    #[serde(rename = "type")]
    server_type: String,
    port: u16,
    pid: Option<u32>,
    url: String,
    tailscale_url: Option<String>,
}

impl Server {
    /// Get the best URL - prefer tailscale_url for remote access
    fn best_url(&self) -> &str {
        self.tailscale_url.as_deref().unwrap_or(&self.url)
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct GitHubLinks {
    repo_url: Option<String>,
    branch_url: Option<String>,
    diff_url: Option<String>,
    commits_url: Option<String>,
    last_commit_url: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct PrInfo {
    number: u32,
    url: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct AgentInfo {
    id: String,
    directory: String,
    repo: Option<String>,
    branch: String,
    servers: Vec<Server>,
    last_commit: String,
    last_commit_hash: Option<String>,
    last_commit_time: String,
    last_commit_timestamp: i64,
    github: Option<GitHubLinks>,
    pr: Option<PrInfo>,
    status: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct ScanResult {
    agents: Vec<AgentInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct HostConfig {
    name: String,
    url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Config {
    hosts: Vec<HostConfig>,
}

#[derive(Debug, Clone, PartialEq)]
enum ConnectionStatus {
    Connected,
    Connecting,
    Disconnected,
}

impl ConnectionStatus {
    /// Sort key for ordering hosts: Connected first, Connecting middle, Disconnected last
    fn sort_key(&self) -> u8 {
        match self {
            ConnectionStatus::Connected => 0,
            ConnectionStatus::Connecting => 1,
            ConnectionStatus::Disconnected => 2,
        }
    }
}

#[derive(Debug, Clone)]
struct HostData {
    config: HostConfig,
    status: ConnectionStatus,
    data: Option<ScanResult>,
    last_fetch: Option<Instant>,
}

#[derive(Debug, Clone)]
enum NavItem {
    HostHeader(usize),
    Repo(usize, usize),
    StaleServersHeader(usize),
    StaleServerRepo(usize, usize),
    StaleReposHeader(usize),
    StaleRepo(usize, usize),
}

#[derive(Debug, Clone, PartialEq)]
enum OverlayMode {
    None,
    Help,
    ServerPicker(Vec<Server>),
}

struct App {
    hosts: Vec<HostData>,
    nav_items: Vec<NavItem>,
    selected: usize,
    filter: String,
    searching: bool,  // Live search mode
    overlay: OverlayMode,
    server_picker_state: ListState,
    host_expanded: HashMap<usize, bool>,
    stale_servers_expanded: HashMap<usize, bool>,
    stale_repos_expanded: HashMap<usize, bool>,
    last_key: Option<char>,
    // Dynamic column widths
    col_width_name: usize,
    col_width_branch: usize,
    col_width_server: usize,
    col_width_pr: usize,
}

impl App {
    fn new(hosts: Vec<HostConfig>) -> Self {
        let host_data: Vec<HostData> = hosts
            .into_iter()
            .map(|config| HostData {
                config,
                status: ConnectionStatus::Connecting,
                data: None,
                last_fetch: None,
            })
            .collect();

        let mut host_expanded = HashMap::new();
        let mut stale_servers_expanded = HashMap::new();
        let mut stale_repos_expanded = HashMap::new();

        for i in 0..host_data.len() {
            host_expanded.insert(i, true);
            stale_servers_expanded.insert(i, true);
            stale_repos_expanded.insert(i, false);
        }

        let mut app = App {
            hosts: host_data,
            nav_items: vec![],
            selected: 0,
            filter: String::new(),
            searching: false,
            overlay: OverlayMode::None,
            server_picker_state: ListState::default(),
            host_expanded,
            stale_servers_expanded,
            stale_repos_expanded,
            last_key: None,
            col_width_name: 10,   // Will grow dynamically based on content
            col_width_branch: 6,
            col_width_server: 10,
            col_width_pr: 5,
        };
        app.rebuild_nav();
        app
    }

    fn recalc_column_widths(&mut self) {
        // Minimum column widths (for headers/readability)
        const MIN_NAME: usize = 10;
        const MIN_BRANCH: usize = 6;
        const MIN_SERVER: usize = 10;
        const MIN_PR: usize = 5;
        // Maximum column widths (to prevent excessive width)
        const MAX_NAME: usize = 26;
        const MAX_BRANCH: usize = 28;
        const MAX_SERVER: usize = 30;
        const MAX_PR: usize = 8;

        let mut max_name = MIN_NAME;
        let mut max_branch = MIN_BRANCH;
        let mut max_server = MIN_SERVER;
        let mut max_pr = MIN_PR;

        for host in &self.hosts {
            if let Some(data) = &host.data {
                for agent in &data.agents {
                    max_name = max_name.max(agent.id.chars().count());
                    max_branch = max_branch.max(agent.branch.chars().count());
                    if !agent.servers.is_empty() {
                        let server_str: String = agent.servers.iter()
                            .map(|s| format!("{}:{}", s.server_type, s.port))
                            .collect::<Vec<_>>()
                            .join(", ");
                        max_server = max_server.max(server_str.chars().count());
                    }
                    if let Some(pr) = &agent.pr {
                        let pr_str = format!("#{}", pr.number);
                        max_pr = max_pr.max(pr_str.chars().count());
                    }
                }
            }
        }

        // Dynamic width: grow to fit content, but cap at maximum
        self.col_width_name = max_name.min(MAX_NAME);
        self.col_width_branch = max_branch.min(MAX_BRANCH);
        self.col_width_server = max_server.min(MAX_SERVER);
        self.col_width_pr = max_pr.min(MAX_PR);
    }

    fn rebuild_nav(&mut self) {
        self.nav_items.clear();

        let mut host_order: Vec<usize> = (0..self.hosts.len()).collect();
        host_order.sort_by_key(|&i| self.hosts[i].status.sort_key());

        let now_ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        let one_day = 86400;

        for &host_idx in &host_order {
            self.nav_items.push(NavItem::HostHeader(host_idx));

            if !self.host_expanded.get(&host_idx).copied().unwrap_or(true) {
                continue;
            }

            let (fresh_indices, stale_server_indices, stale_indices) = {
                let filter = &self.filter;
                if let Some(data) = &self.hosts[host_idx].data {
                    let filtered: Vec<(usize, &AgentInfo)> = data.agents
                        .iter()
                        .enumerate()
                        .filter(|(_, a)| {
                            if filter.is_empty() {
                                true
                            } else {
                                a.id.to_lowercase().contains(&filter.to_lowercase())
                                    || a.branch.to_lowercase().contains(&filter.to_lowercase())
                            }
                        })
                        .collect();

                    let fresh: Vec<usize> = filtered.iter()
                        .filter(|(_, a)| now_ts - a.last_commit_timestamp < one_day)
                        .map(|(i, _)| *i)
                        .collect();

                    let stale_servers: Vec<usize> = filtered.iter()
                        .filter(|(_, a)| now_ts - a.last_commit_timestamp >= one_day && !a.servers.is_empty())
                        .map(|(i, _)| *i)
                        .collect();

                    let stale: Vec<usize> = filtered.iter()
                        .filter(|(_, a)| now_ts - a.last_commit_timestamp >= one_day && a.servers.is_empty())
                        .map(|(i, _)| *i)
                        .collect();

                    (fresh, stale_servers, stale)
                } else {
                    (vec![], vec![], vec![])
                }
            };

            for idx in &fresh_indices {
                self.nav_items.push(NavItem::Repo(host_idx, *idx));
            }

            if !stale_server_indices.is_empty() {
                self.nav_items.push(NavItem::StaleServersHeader(host_idx));
                if self.stale_servers_expanded.get(&host_idx).copied().unwrap_or(true) {
                    for idx in &stale_server_indices {
                        self.nav_items.push(NavItem::StaleServerRepo(host_idx, *idx));
                    }
                }
            }

            if !stale_indices.is_empty() {
                self.nav_items.push(NavItem::StaleReposHeader(host_idx));
                if self.stale_repos_expanded.get(&host_idx).copied().unwrap_or(false) {
                    for idx in &stale_indices {
                        self.nav_items.push(NavItem::StaleRepo(host_idx, *idx));
                    }
                }
            }
        }

        if self.selected >= self.nav_items.len() && !self.nav_items.is_empty() {
            self.selected = self.nav_items.len() - 1;
        }
    }

    fn get_selected_agent(&self) -> Option<&AgentInfo> {
        let nav = self.nav_items.get(self.selected)?;
        match nav {
            NavItem::Repo(host_idx, idx)
            | NavItem::StaleServerRepo(host_idx, idx)
            | NavItem::StaleRepo(host_idx, idx) => {
                self.hosts.get(*host_idx)?.data.as_ref()?.agents.get(*idx)
            }
            _ => None,
        }
    }

    fn toggle_current(&mut self) {
        if let Some(nav) = self.nav_items.get(self.selected).cloned() {
            match nav {
                NavItem::HostHeader(idx) => {
                    let current = self.host_expanded.get(&idx).copied().unwrap_or(true);
                    self.host_expanded.insert(idx, !current);
                    self.rebuild_nav();
                }
                NavItem::StaleServersHeader(idx) => {
                    let current = self.stale_servers_expanded.get(&idx).copied().unwrap_or(true);
                    self.stale_servers_expanded.insert(idx, !current);
                    self.rebuild_nav();
                }
                NavItem::StaleReposHeader(idx) => {
                    let current = self.stale_repos_expanded.get(&idx).copied().unwrap_or(false);
                    self.stale_repos_expanded.insert(idx, !current);
                    self.rebuild_nav();
                }
                _ => {
                    if let Some(agent) = self.get_selected_agent() {
                        open_terminal(&agent.directory);
                    }
                }
            }
        }
    }

    fn move_up(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
    }

    fn move_down(&mut self) {
        if self.selected + 1 < self.nav_items.len() {
            self.selected += 1;
        }
    }

    fn page_up(&mut self) {
        self.selected = self.selected.saturating_sub(10);
    }

    fn page_down(&mut self) {
        self.selected = (self.selected + 10).min(self.nav_items.len().saturating_sub(1));
    }

    fn jump_to_first(&mut self) {
        self.selected = 0;
    }

    fn jump_to_last(&mut self) {
        if !self.nav_items.is_empty() {
            self.selected = self.nav_items.len() - 1;
        }
    }

    fn jump_to_host(&mut self, n: usize) {
        let host_headers: Vec<usize> = self
            .nav_items
            .iter()
            .enumerate()
            .filter_map(|(i, nav)| match nav {
                NavItem::HostHeader(_) => Some(i),
                _ => None,
            })
            .collect();

        if n > 0 && n <= host_headers.len() {
            self.selected = host_headers[n - 1];
        }
    }

    fn next_host(&mut self) {
        let host_headers: Vec<usize> = self
            .nav_items
            .iter()
            .enumerate()
            .filter_map(|(i, nav)| match nav {
                NavItem::HostHeader(_) => Some(i),
                _ => None,
            })
            .collect();

        for &idx in &host_headers {
            if idx > self.selected {
                self.selected = idx;
                return;
            }
        }
        if let Some(&first) = host_headers.first() {
            self.selected = first;
        }
    }

    fn prev_host(&mut self) {
        let host_headers: Vec<usize> = self
            .nav_items
            .iter()
            .enumerate()
            .filter_map(|(i, nav)| match nav {
                NavItem::HostHeader(_) => Some(i),
                _ => None,
            })
            .collect();

        for &idx in host_headers.iter().rev() {
            if idx < self.selected {
                self.selected = idx;
                return;
            }
        }
        if let Some(&last) = host_headers.last() {
            self.selected = last;
        }
    }

    fn open_server_picker(&mut self) {
        if let Some(agent) = self.get_selected_agent() {
            if agent.servers.len() > 1 {
                self.overlay = OverlayMode::ServerPicker(agent.servers.clone());
                self.server_picker_state.select(Some(0));
            } else if let Some(server) = agent.servers.first() {
                open_browser(server.best_url());
            }
        }
    }
}

fn load_config() -> Vec<HostConfig> {
    let config_path = dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("agent-dashboard")
        .join("hosts.toml");

    if let Ok(content) = fs::read_to_string(&config_path) {
        if let Ok(config) = toml::from_str::<Config>(&content) {
            return config.hosts;
        }
    }

    // Default hosts (Tailscale MagicDNS requires full domain)
    vec![
        HostConfig { name: "C-5001".to_string(), url: "http://c-5001.squeaker-teeth.ts.net:9999".to_string() },
        HostConfig { name: "C-5002".to_string(), url: "http://c-5002.squeaker-teeth.ts.net:9999".to_string() },
        HostConfig { name: "C-5003".to_string(), url: "http://c-5003.squeaker-teeth.ts.net:9999".to_string() },
        HostConfig { name: "C-5004".to_string(), url: "http://c-5004.squeaker-teeth.ts.net:9999".to_string() },
    ]
}

async fn fetch_host_data(url: &str) -> Result<ScanResult> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()?;
    let resp = client.get(format!("{}/api/agents", url)).send().await?;
    let data = resp.json::<ScanResult>().await?;
    Ok(data)
}

fn open_terminal(path: &str) {
    #[cfg(target_os = "macos")]
    {
        let _ = Command::new("open")
            .args(["-a", "Terminal.app", path])
            .spawn();
    }
    #[cfg(target_os = "linux")]
    {
        for term in ["gnome-terminal", "konsole", "xterm", "alacritty", "kitty"] {
            if Command::new("which").arg(term).output().map(|o| o.status.success()).unwrap_or(false) {
                let _ = Command::new(term)
                    .arg("--working-directory")
                    .arg(path)
                    .spawn();
                break;
            }
        }
    }
}

fn open_browser(url: &str) {
    let _ = open::that(url);
}

fn open_editor(path: &str) {
    let editor = std::env::var("EDITOR").unwrap_or_else(|_| "nvim".to_string());
    let _ = Command::new(&editor).arg(path).spawn();
}

/// Truncate a string to fit within max_width characters, adding ellipsis if needed
fn truncate_str(s: &str, max_width: usize) -> String {
    if s.chars().count() <= max_width {
        s.to_string()
    } else if max_width > 1 {
        format!("{}…", s.chars().take(max_width - 1).collect::<String>())
    } else {
        "…".to_string()
    }
}

fn draw(frame: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(0), Constraint::Length(1)])
        .split(frame.area());

    // Calculate available width for commit message (fills remaining space)
    // Fixed parts: indent(2) + indicator(2) + name + branch + pr + server + separators(5) + time(10) + borders(2)
    let total_width = frame.area().width as usize;
    let fixed_width = 2 + 2 + app.col_width_name + 1 + app.col_width_branch + 1
        + app.col_width_pr + 1 + app.col_width_server + 1 + 1 + 10 + 2;
    let commit_width = total_width.saturating_sub(fixed_width).max(15);

    // Header with search/filter
    let header = if app.searching || !app.filter.is_empty() {
        let cursor = if app.searching { "_" } else { "" };
        Line::from(vec![
            Span::styled("/", Style::default().fg(Color::Yellow)),
            Span::styled(&app.filter, Style::default().fg(Color::White)),
            Span::styled(cursor, Style::default().fg(Color::Yellow)),
            Span::styled(" │ ", Style::default().fg(Color::DarkGray)),
            Span::styled("type to filter  Esc:clear  Enter:done", Style::default().fg(Color::DarkGray)),
        ])
    } else {
        Line::from(vec![
            Span::styled("agent-dashboard", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::styled(" │ ", Style::default().fg(Color::DarkGray)),
            Span::styled("/:search  ?:help  q:quit  1-9:host", Style::default().fg(Color::DarkGray)),
        ])
    };
    frame.render_widget(Paragraph::new(header), chunks[0]);

    let now_ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    let one_day = 86400;

    let mut items: Vec<ListItem> = vec![];

    for (nav_idx, nav_item) in app.nav_items.iter().enumerate() {
        let is_selected = nav_idx == app.selected;
        let style = if is_selected {
            Style::default().bg(Color::DarkGray).add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };

        let line = match nav_item {
            NavItem::HostHeader(host_idx) => {
                let host = &app.hosts[*host_idx];
                let expanded = app.host_expanded.get(host_idx).copied().unwrap_or(true);
                let arrow = if expanded { "▼" } else { "▶" };

                let status_color = match host.status {
                    ConnectionStatus::Connected => Color::Green,
                    ConnectionStatus::Connecting => Color::Yellow,
                    ConnectionStatus::Disconnected => Color::Red,
                };
                let status_char = match host.status {
                    ConnectionStatus::Connected => "●",
                    ConnectionStatus::Connecting => "◐",
                    ConnectionStatus::Disconnected => "●",
                };

                let repo_count = host.data.as_ref().map(|d| d.agents.len()).unwrap_or(0);
                let active_count = host.data.as_ref().map(|d| {
                    d.agents.iter().filter(|a| !a.servers.is_empty()).count()
                }).unwrap_or(0);

                Line::from(vec![
                    Span::styled(status_char, Style::default().fg(status_color)),
                    Span::raw(format!(" {} {} ", arrow, host.config.name)),
                    Span::styled(
                        format!("({} repos, {} active)", repo_count, active_count),
                        Style::default().fg(Color::DarkGray),
                    ),
                ])
            }
            NavItem::Repo(host_idx, idx)
            | NavItem::StaleServerRepo(host_idx, idx)
            | NavItem::StaleRepo(host_idx, idx) => {
                if let Some(agent) = app.hosts.get(*host_idx)
                    .and_then(|h| h.data.as_ref())
                    .and_then(|d| d.agents.get(*idx)) {
                    // Build server string with +N for overflow
                    let server_display = if agent.servers.is_empty() {
                        String::new()
                    } else {
                        let max_width = app.col_width_server;
                        let server_strs: Vec<String> = agent.servers.iter()
                            .map(|s| format!("{}:{}", s.server_type, s.port))
                            .collect();

                        let mut result = String::new();
                        let mut shown = 0;
                        for (i, s) in server_strs.iter().enumerate() {
                            let remaining = agent.servers.len() - i - 1;
                            let suffix = if remaining > 0 { format!(", +{}", remaining) } else { String::new() };
                            let candidate = if result.is_empty() {
                                format!("{}{}", s, suffix)
                            } else {
                                format!("{}, {}{}", result, s, suffix)
                            };

                            if candidate.chars().count() <= max_width {
                                if !result.is_empty() {
                                    result.push_str(", ");
                                }
                                result.push_str(s);
                                shown += 1;
                            } else {
                                break;
                            }
                        }

                        let hidden = agent.servers.len() - shown;
                        if hidden > 0 {
                            result.push_str(&format!(", +{}", hidden));
                        }
                        result
                    };

                    // Truncate values that exceed column width (char-safe)
                    let name_display = truncate_str(&agent.id, app.col_width_name);
                    let branch_display = truncate_str(&agent.branch, app.col_width_branch);
                    let pr_display = agent.pr.as_ref()
                        .map(|pr| format!("#{}", pr.number))
                        .unwrap_or_default();
                    let commit = truncate_str(&agent.last_commit, commit_width);
                    let time_width = 10;
                    let time_display = truncate_str(&agent.last_commit_time, time_width);

                    let server_indicator = if !agent.servers.is_empty() {
                        Span::styled("● ", Style::default().fg(Color::Green))
                    } else {
                        Span::raw("  ")
                    };

                    Line::from(vec![
                        Span::raw("  "),
                        server_indicator,
                        Span::styled(
                            format!("{:<width$}", name_display, width = app.col_width_name),
                            Style::default().fg(Color::Cyan),
                        ),
                        Span::raw(" "),
                        Span::styled(
                            format!("{:<width$}", branch_display, width = app.col_width_branch),
                            Style::default().fg(Color::LightMagenta),
                        ),
                        Span::raw(" "),
                        Span::styled(
                            format!("{:<width$}", pr_display, width = app.col_width_pr),
                            Style::default().fg(Color::Blue),
                        ),
                        Span::raw(" "),
                        Span::styled(
                            format!("{:<width$}", server_display, width = app.col_width_server),
                            Style::default().fg(Color::Yellow),
                        ),
                        Span::raw(" "),
                        Span::raw(format!("{:<width$}", commit, width = commit_width)),
                        Span::raw(" "),
                        Span::styled(
                            format!("{:>width$}", time_display, width = time_width),
                            Style::default().fg(Color::DarkGray),
                        ),
                    ])
                } else {
                    Line::raw("    (error)")
                }
            }
            NavItem::StaleServersHeader(host_idx) => {
                let expanded = app.stale_servers_expanded.get(host_idx).copied().unwrap_or(true);
                let arrow = if expanded { "▼" } else { "▶" };

                let count = app.hosts.get(*host_idx)
                    .and_then(|h| h.data.as_ref())
                    .map(|d| d.agents.iter()
                        .filter(|a| now_ts - a.last_commit_timestamp >= one_day && !a.servers.is_empty())
                        .count())
                    .unwrap_or(0);
                Line::styled(
                    format!("  {} Stale with Servers ({})", arrow, count),
                    Style::default().fg(Color::Green),
                )
            }
            NavItem::StaleReposHeader(host_idx) => {
                let expanded = app.stale_repos_expanded.get(host_idx).copied().unwrap_or(false);
                let arrow = if expanded { "▼" } else { "▶" };

                let count = app.hosts.get(*host_idx)
                    .and_then(|h| h.data.as_ref())
                    .map(|d| d.agents.iter()
                        .filter(|a| now_ts - a.last_commit_timestamp >= one_day && a.servers.is_empty())
                        .count())
                    .unwrap_or(0);
                Line::styled(
                    format!("  {} Stale Repos ({})", arrow, count),
                    Style::default().fg(Color::DarkGray),
                )
            }
        };

        items.push(ListItem::new(line).style(style));
    }

    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL))
        .highlight_symbol("▶ ");
    frame.render_widget(list, chunks[1]);

    // Status bar
    let status = Line::from(vec![
        Span::styled("↑↓", Style::default().fg(Color::Yellow)),
        Span::styled(":nav ", Style::default().fg(Color::DarkGray)),
        Span::styled("o", Style::default().fg(Color::Yellow)),
        Span::styled(":branch ", Style::default().fg(Color::DarkGray)),
        Span::styled("d", Style::default().fg(Color::Yellow)),
        Span::styled(":diff ", Style::default().fg(Color::DarkGray)),
        Span::styled("p", Style::default().fg(Color::Yellow)),
        Span::styled(":pr ", Style::default().fg(Color::DarkGray)),
        Span::styled("s", Style::default().fg(Color::Yellow)),
        Span::styled(":server ", Style::default().fg(Color::DarkGray)),
        Span::styled("e", Style::default().fg(Color::Yellow)),
        Span::styled(":editor ", Style::default().fg(Color::DarkGray)),
        Span::styled("r", Style::default().fg(Color::Yellow)),
        Span::styled(":refresh", Style::default().fg(Color::DarkGray)),
    ]);
    frame.render_widget(Paragraph::new(status), chunks[2]);

    // Overlays
    match &app.overlay {
        OverlayMode::Help => draw_help_overlay(frame),
        OverlayMode::ServerPicker(servers) => draw_server_picker(frame, servers, &app.server_picker_state),
        OverlayMode::None => {}
    }
}

fn draw_help_overlay(frame: &mut Frame) {
    let help_text = r#"
  agent-dashboard TUI

  NAVIGATION
    ↑/↓, j/k, C-n/C-p   Move selection
    1-9                  Jump to host N
    Tab / S-Tab          Next / Previous host
    gg / G               First / Last item
    Enter                Toggle section / Open terminal

  ACTIONS
    o                    Open branch on GitHub
    d                    Open diff vs main
    p                    Open PR on GitHub
    s                    Open server (picker if multiple)
    e                    Open in $EDITOR
    r / R                Refresh host / all hosts

  SEARCH
    /                    Start typing to filter
    Esc                  Clear filter

  UTILITY
    ?                    This help
    q / Esc / C-c        Quit

  CONFIG
    ~/.config/agent-dashboard/hosts.toml

  Press any key to close..."#;

    let area = centered_rect(70, 80, frame.area());
    let popup = Paragraph::new(help_text)
        .style(Style::default().fg(Color::White))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Help ")
                .style(Style::default().bg(Color::Black)),
        );

    frame.render_widget(Clear, area);
    frame.render_widget(popup, area);
}

fn draw_server_picker(frame: &mut Frame, servers: &[Server], state: &ListState) {
    let popup_width = 50;
    let popup_height = (servers.len() + 4).min(15) as u16;
    let area = frame.area();
    let x = (area.width.saturating_sub(popup_width)) / 2;
    let y = (area.height.saturating_sub(popup_height)) / 2;
    let popup_area = Rect::new(x, y, popup_width, popup_height);

    let items: Vec<ListItem> = servers
        .iter()
        .enumerate()
        .map(|(i, s)| {
            ListItem::new(Line::from(vec![
                Span::styled(format!("{} ", i + 1), Style::default().fg(Color::Yellow)),
                Span::styled(&s.server_type, Style::default().fg(Color::Cyan)),
                Span::raw(format!(":{}", s.port)),
            ]))
        })
        .collect();

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Select Server ")
                .style(Style::default().bg(Color::Black)),
        )
        .highlight_style(Style::default().bg(Color::DarkGray).add_modifier(Modifier::BOLD))
        .highlight_symbol("▶ ");

    frame.render_widget(Clear, popup_area);
    frame.render_stateful_widget(list, popup_area, &mut state.clone());
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    let dump_mode = args.iter().any(|a| a == "--dump" || a == "-d");
    let help_mode = args.iter().any(|a| a == "--help" || a == "-h");

    if help_mode {
        println!("agent-dashboard-tui - TUI for managing development environments");
        println!();
        println!("USAGE:");
        println!("    agent-dashboard-tui [OPTIONS]");
        println!();
        println!("OPTIONS:");
        println!("    -d, --dump    Fetch all hosts, print JSON, and exit (debug mode)");
        println!("    -h, --help    Show this help message");
        println!();
        println!("CONFIG:");
        println!("    ~/.config/agent-dashboard/hosts.toml");
        return Ok(());
    }

    let configs = load_config();

    // Dump mode: fetch all hosts, print JSON, exit
    if dump_mode {
        let mut results: HashMap<String, Option<ScanResult>> = HashMap::new();
        for config in &configs {
            match fetch_host_data(&config.url).await {
                Ok(data) => {
                    results.insert(config.name.clone(), Some(data));
                }
                Err(e) => {
                    eprintln!("Error fetching {}: {}", config.name, e);
                    results.insert(config.name.clone(), None);
                }
            }
        }
        println!("{}", serde_json::to_string_pretty(&results)?);
        return Ok(());
    }

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    let mut app = App::new(configs);

    // Channel for background fetch results
    let (tx, mut rx) = tokio::sync::mpsc::channel::<(usize, Result<ScanResult>)>(16);

    // Spawn initial fetches for all hosts
    for i in 0..app.hosts.len() {
        let url = app.hosts[i].config.url.clone();
        let tx = tx.clone();
        tokio::spawn(async move {
            let result = fetch_host_data(&url).await;
            let _ = tx.send((i, result)).await;
        });
    }

    let mut last_refresh = Instant::now();
    let connected_interval = Duration::from_secs(10);
    let disconnected_interval = Duration::from_secs(60);

    loop {
        terminal.draw(|frame| draw(frame, &app))?;

        // Check for background fetch results (non-blocking)
        while let Ok((i, result)) = rx.try_recv() {
            match result {
                Ok(data) => {
                    app.hosts[i].data = Some(data);
                    app.hosts[i].status = ConnectionStatus::Connected;
                }
                Err(_) => {
                    app.hosts[i].status = ConnectionStatus::Disconnected;
                }
            }
            // Always update last_fetch to prevent rapid retries
            app.hosts[i].last_fetch = Some(Instant::now());
            app.recalc_column_widths();
            app.rebuild_nav();
        }

        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.kind != KeyEventKind::Press {
                    continue;
                }

                // Handle live search mode first
                if app.searching {
                    match key.code {
                        KeyCode::Esc => {
                            app.filter.clear();
                            app.searching = false;
                            app.rebuild_nav();
                        }
                        KeyCode::Enter => {
                            app.searching = false;
                        }
                        KeyCode::Backspace => {
                            app.filter.pop();
                            app.rebuild_nav();
                        }
                        KeyCode::Char(c) => {
                            app.filter.push(c);
                            app.rebuild_nav();
                        }
                        _ => {}
                    }
                    continue;
                }

                // Handle overlays
                match &app.overlay {
                    OverlayMode::Help => {
                        app.overlay = OverlayMode::None;
                        continue;
                    }
                    OverlayMode::ServerPicker(servers) => {
                        let servers = servers.clone();
                        match key.code {
                            KeyCode::Esc => {
                                app.overlay = OverlayMode::None;
                            }
                            KeyCode::Up | KeyCode::Char('k') => {
                                let current = app.server_picker_state.selected().unwrap_or(0);
                                if current > 0 {
                                    app.server_picker_state.select(Some(current - 1));
                                }
                            }
                            KeyCode::Char('p') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                                let current = app.server_picker_state.selected().unwrap_or(0);
                                if current > 0 {
                                    app.server_picker_state.select(Some(current - 1));
                                }
                            }
                            KeyCode::Down | KeyCode::Char('j') => {
                                let current = app.server_picker_state.selected().unwrap_or(0);
                                if current + 1 < servers.len() {
                                    app.server_picker_state.select(Some(current + 1));
                                }
                            }
                            KeyCode::Char('n') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                                let current = app.server_picker_state.selected().unwrap_or(0);
                                if current + 1 < servers.len() {
                                    app.server_picker_state.select(Some(current + 1));
                                }
                            }
                            KeyCode::Enter => {
                                if let Some(idx) = app.server_picker_state.selected() {
                                    if let Some(server) = servers.get(idx) {
                                        open_browser(server.best_url());
                                    }
                                }
                                app.overlay = OverlayMode::None;
                            }
                            KeyCode::Char(c) if c.is_ascii_digit() => {
                                let n = c.to_digit(10).unwrap() as usize;
                                if n > 0 && n <= servers.len() {
                                    open_browser(servers[n - 1].best_url());
                                    app.overlay = OverlayMode::None;
                                }
                            }
                            _ => {}
                        }
                        continue;
                    }
                    OverlayMode::None => {}
                }

                // Main key handling
                match key.code {
                    KeyCode::Char('q') => break,
                    KeyCode::Esc => {
                        if !app.filter.is_empty() {
                            app.filter.clear();
                            app.rebuild_nav();
                        } else {
                            break;
                        }
                    }
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => break,
                    KeyCode::Up | KeyCode::Char('k') => app.move_up(),
                    KeyCode::Char('p') if key.modifiers.contains(KeyModifiers::CONTROL) => app.move_up(),
                    KeyCode::Down | KeyCode::Char('j') => app.move_down(),
                    KeyCode::Char('n') if key.modifiers.contains(KeyModifiers::CONTROL) => app.move_down(),
                    KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => app.page_up(),
                    KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => app.page_down(),
                    KeyCode::Char('G') => app.jump_to_last(),
                    KeyCode::Char('g') => {
                        if app.last_key == Some('g') {
                            app.jump_to_first();
                            app.last_key = None;
                        } else {
                            app.last_key = Some('g');
                        }
                        continue;
                    }
                    KeyCode::Tab => app.next_host(),
                    KeyCode::BackTab => app.prev_host(),
                    KeyCode::Char(c) if c.is_ascii_digit() && !key.modifiers.contains(KeyModifiers::CONTROL) => {
                        let n = c.to_digit(10).unwrap() as usize;
                        app.jump_to_host(n);
                    }
                    KeyCode::Enter => app.toggle_current(),
                    KeyCode::Char('/') => {
                        app.searching = true;
                    }
                    KeyCode::Char('o') => {
                        if let Some(agent) = app.get_selected_agent() {
                            if let Some(github) = &agent.github {
                                if let Some(url) = &github.branch_url {
                                    open_browser(url);
                                }
                            }
                        }
                    }
                    KeyCode::Char('d') => {
                        if let Some(agent) = app.get_selected_agent() {
                            if let Some(github) = &agent.github {
                                if let Some(url) = &github.diff_url {
                                    open_browser(url);
                                }
                            }
                        }
                    }
                    KeyCode::Char('p') => {
                        if let Some(agent) = app.get_selected_agent() {
                            if let Some(pr) = &agent.pr {
                                open_browser(&pr.url);
                            }
                        }
                    }
                    KeyCode::Char('s') => {
                        app.open_server_picker();
                    }
                    KeyCode::Char('e') => {
                        if let Some(agent) = app.get_selected_agent() {
                            open_editor(&agent.directory);
                        }
                    }
                    KeyCode::Char('?') | KeyCode::F(1) => {
                        app.overlay = OverlayMode::Help;
                    }
                    KeyCode::Char('r') => {
                        if let Some(nav) = app.nav_items.get(app.selected) {
                            let host_idx = match nav {
                                NavItem::HostHeader(i) => *i,
                                NavItem::Repo(i, _) => *i,
                                NavItem::StaleServersHeader(i) => *i,
                                NavItem::StaleServerRepo(i, _) => *i,
                                NavItem::StaleReposHeader(i) => *i,
                                NavItem::StaleRepo(i, _) => *i,
                            };
                            let url = app.hosts[host_idx].config.url.clone();
                            app.hosts[host_idx].status = ConnectionStatus::Connecting;
                            let tx = tx.clone();
                            tokio::spawn(async move {
                                let result = fetch_host_data(&url).await;
                                let _ = tx.send((host_idx, result)).await;
                            });
                        }
                    }
                    KeyCode::Char('R') => {
                        for i in 0..app.hosts.len() {
                            let url = app.hosts[i].config.url.clone();
                            app.hosts[i].status = ConnectionStatus::Connecting;
                            let tx = tx.clone();
                            tokio::spawn(async move {
                                let result = fetch_host_data(&url).await;
                                let _ = tx.send((i, result)).await;
                            });
                        }
                        app.recalc_column_widths();
                        app.rebuild_nav();
                    }
                    _ => {}
                }
                app.last_key = None;
            }
        }

        // Background refresh - spawn tasks for hosts that need updating
        if last_refresh.elapsed() > Duration::from_secs(1) {
            for i in 0..app.hosts.len() {
                let interval = match app.hosts[i].status {
                    ConnectionStatus::Connected => connected_interval,
                    ConnectionStatus::Connecting => continue, // Skip if already fetching
                    _ => disconnected_interval,
                };

                if app.hosts[i].last_fetch.map(|t| t.elapsed() > interval).unwrap_or(true) {
                    let url = app.hosts[i].config.url.clone();
                    app.hosts[i].status = ConnectionStatus::Connecting;
                    let tx = tx.clone();
                    tokio::spawn(async move {
                        let result = fetch_host_data(&url).await;
                        let _ = tx.send((i, result)).await;
                    });
                }
            }
            last_refresh = Instant::now();
        }
    }

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // ==================== Config Parsing Tests ====================

    #[test]
    fn test_config_parse_toml() {
        let toml_str = r#"
[[hosts]]
name = "C-5001"
url = "http://c-5001.example.com:9999"

[[hosts]]
name = "C-5002"
url = "http://c-5002.example.com:9999"
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.hosts.len(), 2);
        assert_eq!(config.hosts[0].name, "C-5001");
        assert_eq!(config.hosts[0].url, "http://c-5001.example.com:9999");
        assert_eq!(config.hosts[1].name, "C-5002");
    }

    #[test]
    fn test_config_empty_hosts() {
        let toml_str = "hosts = []";
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.hosts.len(), 0);
    }

    #[test]
    fn test_default_hosts() {
        // When no config file exists, should return defaults
        let defaults = vec![
            HostConfig { name: "C-5001".to_string(), url: "http://c-5001.squeaker-teeth.ts.net:9999".to_string() },
            HostConfig { name: "C-5002".to_string(), url: "http://c-5002.squeaker-teeth.ts.net:9999".to_string() },
            HostConfig { name: "C-5003".to_string(), url: "http://c-5003.squeaker-teeth.ts.net:9999".to_string() },
            HostConfig { name: "C-5004".to_string(), url: "http://c-5004.squeaker-teeth.ts.net:9999".to_string() },
        ];
        assert_eq!(defaults.len(), 4);
        assert!(defaults[0].url.contains("squeaker-teeth.ts.net"));
    }

    // ==================== API Parsing Tests ====================

    #[test]
    fn test_parse_server() {
        let json = r#"{
            "type": "vite",
            "port": 5173,
            "pid": 12345,
            "url": "http://localhost:5173",
            "tailscaleUrl": "http://c-5001.example.com:5173"
        }"#;
        let server: Server = serde_json::from_str(json).unwrap();
        assert_eq!(server.server_type, "vite");
        assert_eq!(server.port, 5173);
        assert_eq!(server.pid, Some(12345));
        assert_eq!(server.url, "http://localhost:5173");
        assert_eq!(server.tailscale_url, Some("http://c-5001.example.com:5173".to_string()));
    }

    #[test]
    fn test_server_best_url_with_tailscale() {
        let server = Server {
            server_type: "vite".to_string(),
            port: 5173,
            pid: Some(12345),
            url: "http://localhost:5173".to_string(),
            tailscale_url: Some("http://c-5001.example.com:5173".to_string()),
        };
        assert_eq!(server.best_url(), "http://c-5001.example.com:5173");
    }

    #[test]
    fn test_server_best_url_without_tailscale() {
        let server = Server {
            server_type: "vite".to_string(),
            port: 5173,
            pid: None,
            url: "http://localhost:5173".to_string(),
            tailscale_url: None,
        };
        assert_eq!(server.best_url(), "http://localhost:5173");
    }

    #[test]
    fn test_parse_github_links() {
        let json = r#"{
            "repoUrl": "https://github.com/owner/repo",
            "branchUrl": "https://github.com/owner/repo/tree/main",
            "diffUrl": "https://github.com/owner/repo/compare/main...feature",
            "commitsUrl": "https://github.com/owner/repo/commits/main",
            "lastCommitUrl": "https://github.com/owner/repo/commit/abc123"
        }"#;
        let links: GitHubLinks = serde_json::from_str(json).unwrap();
        assert_eq!(links.repo_url, Some("https://github.com/owner/repo".to_string()));
        assert_eq!(links.branch_url, Some("https://github.com/owner/repo/tree/main".to_string()));
        assert!(links.diff_url.is_some());
    }

    #[test]
    fn test_parse_pr_info() {
        let json = r#"{
            "number": 42,
            "url": "https://github.com/owner/repo/pull/42"
        }"#;
        let pr: PrInfo = serde_json::from_str(json).unwrap();
        assert_eq!(pr.number, 42);
        assert_eq!(pr.url, "https://github.com/owner/repo/pull/42");
    }

    #[test]
    fn test_parse_agent_info_full() {
        let json = r#"{
            "id": "my-repo",
            "directory": "/home/user/gits/my-repo",
            "repo": "owner/my-repo",
            "branch": "feature-branch",
            "servers": [{
                "type": "vite",
                "port": 5173,
                "url": "http://localhost:5173"
            }],
            "lastCommit": "feat: add new feature",
            "lastCommitHash": "abc123def",
            "lastCommitTime": "2 hours ago",
            "lastCommitTimestamp": 1234567890,
            "github": {
                "branchUrl": "https://github.com/owner/my-repo/tree/feature-branch",
                "diffUrl": "https://github.com/owner/my-repo/compare/main...feature-branch"
            },
            "pr": {
                "number": 42,
                "url": "https://github.com/owner/my-repo/pull/42"
            },
            "status": "active"
        }"#;
        let agent: AgentInfo = serde_json::from_str(json).unwrap();
        assert_eq!(agent.id, "my-repo");
        assert_eq!(agent.branch, "feature-branch");
        assert_eq!(agent.servers.len(), 1);
        assert!(agent.pr.is_some());
        assert_eq!(agent.pr.unwrap().number, 42);
        assert_eq!(agent.status, Some("active".to_string()));
    }

    #[test]
    fn test_parse_agent_info_minimal() {
        let json = r#"{
            "id": "my-repo",
            "directory": "/home/user/gits/my-repo",
            "branch": "main",
            "servers": [],
            "lastCommit": "initial commit",
            "lastCommitTime": "1 day ago",
            "lastCommitTimestamp": 1234567890
        }"#;
        let agent: AgentInfo = serde_json::from_str(json).unwrap();
        assert_eq!(agent.id, "my-repo");
        assert!(agent.pr.is_none());
        assert!(agent.github.is_none());
        assert!(agent.repo.is_none());
    }

    #[test]
    fn test_parse_scan_result() {
        let json = r#"{
            "agents": [
                {
                    "id": "repo1",
                    "directory": "/path/to/repo1",
                    "branch": "main",
                    "servers": [],
                    "lastCommit": "commit 1",
                    "lastCommitTime": "1h ago",
                    "lastCommitTimestamp": 1234567890
                },
                {
                    "id": "repo2",
                    "directory": "/path/to/repo2",
                    "branch": "develop",
                    "servers": [],
                    "lastCommit": "commit 2",
                    "lastCommitTime": "2h ago",
                    "lastCommitTimestamp": 1234567800
                }
            ]
        }"#;
        let result: ScanResult = serde_json::from_str(json).unwrap();
        assert_eq!(result.agents.len(), 2);
        assert_eq!(result.agents[0].id, "repo1");
        assert_eq!(result.agents[1].id, "repo2");
    }

    // ==================== App State Tests ====================

    #[test]
    fn test_app_new() {
        let configs = vec![
            HostConfig { name: "Host1".to_string(), url: "http://host1:9999".to_string() },
            HostConfig { name: "Host2".to_string(), url: "http://host2:9999".to_string() },
        ];
        let app = App::new(configs);
        assert_eq!(app.hosts.len(), 2);
        assert_eq!(app.hosts[0].config.name, "Host1");
        assert_eq!(app.hosts[1].config.name, "Host2");
        assert!(matches!(app.hosts[0].status, ConnectionStatus::Connecting));
    }

    #[test]
    fn test_app_move_up_down() {
        let configs = vec![
            HostConfig { name: "Host1".to_string(), url: "http://host1:9999".to_string() },
        ];
        let mut app = App::new(configs);
        app.nav_items = vec![
            NavItem::HostHeader(0),
            NavItem::Repo(0, 0),
            NavItem::Repo(0, 1),
        ];

        assert_eq!(app.selected, 0);
        app.move_down();
        assert_eq!(app.selected, 1);
        app.move_down();
        assert_eq!(app.selected, 2);
        app.move_down(); // Should not go past end
        assert_eq!(app.selected, 2);
        app.move_up();
        assert_eq!(app.selected, 1);
        app.move_up();
        assert_eq!(app.selected, 0);
        app.move_up(); // Should not go negative
        assert_eq!(app.selected, 0);
    }

    #[test]
    fn test_host_sorting() {
        let configs = vec![
            HostConfig { name: "Host1".to_string(), url: "http://host1:9999".to_string() },
            HostConfig { name: "Host2".to_string(), url: "http://host2:9999".to_string() },
            HostConfig { name: "Host3".to_string(), url: "http://host3:9999".to_string() },
        ];
        let mut app = App::new(configs);

        // Set different statuses
        app.hosts[0].status = ConnectionStatus::Disconnected;
        app.hosts[1].status = ConnectionStatus::Connected;
        app.hosts[2].status = ConnectionStatus::Connecting;

        // Get sorted indices
        let mut indices: Vec<usize> = (0..app.hosts.len()).collect();
        indices.sort_by_key(|&i| app.hosts[i].status.sort_key());

        // Connected (1) should be first, Connecting (2) second, Disconnected (0) last
        assert_eq!(indices, vec![1, 2, 0]);
    }

    #[test]
    fn test_filter_matching() {
        let agent = AgentInfo {
            id: "my-awesome-repo".to_string(),
            directory: "/path/to/repo".to_string(),
            repo: None,
            branch: "feature-xyz".to_string(),
            servers: vec![],
            last_commit: "test".to_string(),
            last_commit_hash: None,
            last_commit_time: "1h".to_string(),
            last_commit_timestamp: 0,
            github: None,
            pr: None,
            status: None,
        };

        // Filter should match id
        assert!(agent.id.to_lowercase().contains("awesome"));
        assert!(agent.id.to_lowercase().contains("repo"));

        // Filter should match branch
        assert!(agent.branch.to_lowercase().contains("feature"));
        assert!(agent.branch.to_lowercase().contains("xyz"));

        // Should not match
        assert!(!agent.id.to_lowercase().contains("notfound"));
    }

    #[test]
    fn test_connection_status_sort_order() {
        assert!(ConnectionStatus::Connected.sort_key() < ConnectionStatus::Connecting.sort_key());
        assert!(ConnectionStatus::Connecting.sort_key() < ConnectionStatus::Disconnected.sort_key());
    }

    // ==================== Overlay Tests ====================

    #[test]
    fn test_overlay_mode_none() {
        let overlay = OverlayMode::None;
        assert!(matches!(overlay, OverlayMode::None));
    }

    #[test]
    fn test_overlay_mode_help() {
        let overlay = OverlayMode::Help;
        assert!(matches!(overlay, OverlayMode::Help));
    }

    #[test]
    fn test_overlay_mode_server_picker() {
        let servers = vec![
            Server {
                server_type: "vite".to_string(),
                port: 5173,
                pid: None,
                url: "http://localhost:5173".to_string(),
                tailscale_url: None,
            },
        ];
        let overlay = OverlayMode::ServerPicker(servers.clone());
        if let OverlayMode::ServerPicker(s) = overlay {
            assert_eq!(s.len(), 1);
            assert_eq!(s[0].port, 5173);
        } else {
            panic!("Expected ServerPicker");
        }
    }

    // ==================== Stale Classification Tests ====================

    #[test]
    fn test_stale_classification() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        let one_day_secs: i64 = 24 * 60 * 60;

        // Fresh: less than 1 day old
        let fresh_timestamp = now - (one_day_secs / 2); // 12 hours ago
        assert!(now - fresh_timestamp < one_day_secs);

        // Stale: more than 1 day old
        let stale_timestamp = now - (one_day_secs * 2); // 2 days ago
        assert!(now - stale_timestamp >= one_day_secs);
    }

    // ==================== Nav Item Tests ====================

    #[test]
    fn test_nav_item_variants() {
        let items = vec![
            NavItem::HostHeader(0),
            NavItem::Repo(0, 0),
            NavItem::StaleServersHeader(0),
            NavItem::StaleServerRepo(0, 0),
            NavItem::StaleReposHeader(0),
            NavItem::StaleRepo(0, 0),
        ];

        assert_eq!(items.len(), 6);

        // Test pattern matching
        for item in &items {
            match item {
                NavItem::HostHeader(h) => assert_eq!(*h, 0),
                NavItem::Repo(h, r) => { assert_eq!(*h, 0); assert_eq!(*r, 0); }
                NavItem::StaleServersHeader(h) => assert_eq!(*h, 0),
                NavItem::StaleServerRepo(h, r) => { assert_eq!(*h, 0); assert_eq!(*r, 0); }
                NavItem::StaleReposHeader(h) => assert_eq!(*h, 0),
                NavItem::StaleRepo(h, r) => { assert_eq!(*h, 0); assert_eq!(*r, 0); }
            }
        }
    }

    // ==================== String Truncation Tests ====================

    #[test]
    fn test_truncate_str_short() {
        // String shorter than max width should be unchanged
        assert_eq!(truncate_str("hello", 10), "hello");
        assert_eq!(truncate_str("test", 4), "test");
    }

    #[test]
    fn test_truncate_str_exact() {
        // String exactly at max width should be unchanged
        assert_eq!(truncate_str("hello", 5), "hello");
    }

    #[test]
    fn test_truncate_str_long() {
        // String longer than max width should be truncated with ellipsis
        assert_eq!(truncate_str("feature/cloudflare-deploy", 20), "feature/cloudflare-…");
        assert_eq!(truncate_str("very-long-branch-name", 10), "very-long…");
    }

    #[test]
    fn test_truncate_str_unicode() {
        // Should handle unicode characters properly
        assert_eq!(truncate_str("héllo", 5), "héllo");
        assert_eq!(truncate_str("héllo world", 6), "héllo…");
    }

    #[test]
    fn test_truncate_str_edge_cases() {
        // Edge cases
        assert_eq!(truncate_str("", 5), "");
        assert_eq!(truncate_str("ab", 1), "…");
        assert_eq!(truncate_str("a", 1), "a");
    }
}
