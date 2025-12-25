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
    url: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct GitHubLinks {
    branch_url: Option<String>,
    diff_url: Option<String>,
    last_commit_url: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct AgentInfo {
    id: String,
    directory: String,
    branch: String,
    servers: Vec<Server>,
    last_commit: String,
    last_commit_time: String,
    last_commit_timestamp: i64,
    github: Option<GitHubLinks>,
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

#[derive(Debug, Clone)]
enum ConnectionStatus {
    Connected,
    Connecting,
    Disconnected,
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
            col_width_name: 18,
            col_width_branch: 14,
            col_width_server: 18,
        };
        app.rebuild_nav();
        app
    }

    fn recalc_column_widths(&mut self) {
        let mut max_name = 8usize;
        let mut max_branch = 6usize;
        let mut max_server = 6usize;

        for host in &self.hosts {
            if let Some(data) = &host.data {
                for agent in &data.agents {
                    max_name = max_name.max(agent.id.len());
                    max_branch = max_branch.max(agent.branch.len());
                    if !agent.servers.is_empty() {
                        let server_str: String = agent.servers.iter()
                            .map(|s| format!("{}:{}", s.server_type, s.port))
                            .collect::<Vec<_>>()
                            .join(", ");
                        max_server = max_server.max(server_str.len());
                    }
                }
            }
        }

        self.col_width_name = max_name.min(25) + 1;
        self.col_width_branch = max_branch.min(20) + 1;
        self.col_width_server = max_server.min(25) + 1;
    }

    fn rebuild_nav(&mut self) {
        self.nav_items.clear();

        let mut host_order: Vec<usize> = (0..self.hosts.len()).collect();
        host_order.sort_by_key(|&i| match self.hosts[i].status {
            ConnectionStatus::Connected => 0,
            ConnectionStatus::Connecting => 1,
            ConnectionStatus::Disconnected => 2,
        });

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
                open_browser(&server.url);
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

    // Default hosts (Tailscale hostnames use capital C)
    vec![
        HostConfig { name: "C-5001".to_string(), url: "http://C-5001:9999".to_string() },
        HostConfig { name: "C-5002".to_string(), url: "http://C-5002:9999".to_string() },
        HostConfig { name: "C-5003".to_string(), url: "http://C-5003:9999".to_string() },
        HostConfig { name: "C-5004".to_string(), url: "http://C-5004:9999".to_string() },
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

fn draw(frame: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(0), Constraint::Length(1)])
        .split(frame.area());

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
                    ConnectionStatus::Connecting => "●",
                    ConnectionStatus::Disconnected => "✗",
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
                    let server_str = if agent.servers.is_empty() {
                        String::new()
                    } else {
                        agent.servers.iter()
                            .map(|s| format!("{}:{}", s.server_type, s.port))
                            .collect::<Vec<_>>()
                            .join(", ")
                    };

                    let commit = if agent.last_commit.len() > 25 {
                        format!("{}...", &agent.last_commit[..22])
                    } else {
                        agent.last_commit.clone()
                    };

                    let server_indicator = if !agent.servers.is_empty() {
                        Span::styled("● ", Style::default().fg(Color::Green))
                    } else {
                        Span::raw("  ")
                    };

                    Line::from(vec![
                        Span::raw("  "),
                        server_indicator,
                        Span::styled(
                            format!("{:<width$}", agent.id, width = app.col_width_name),
                            Style::default().fg(Color::Cyan),
                        ),
                        Span::styled(
                            format!("{:<width$}", agent.branch, width = app.col_width_branch),
                            Style::default().fg(Color::LightMagenta),
                        ),
                        Span::styled(
                            format!("{:<width$}", server_str, width = app.col_width_server),
                            Style::default().fg(Color::Yellow),
                        ),
                        Span::raw(format!("{:<20}", commit)),
                        Span::styled(
                            agent.last_commit_time.clone(),
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
        Span::styled("Enter", Style::default().fg(Color::Yellow)),
        Span::styled(":term ", Style::default().fg(Color::DarkGray)),
        Span::styled("o", Style::default().fg(Color::Yellow)),
        Span::styled(":branch ", Style::default().fg(Color::DarkGray)),
        Span::styled("d", Style::default().fg(Color::Yellow)),
        Span::styled(":diff ", Style::default().fg(Color::DarkGray)),
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

    // Initial fetch
    for i in 0..app.hosts.len() {
        let url = app.hosts[i].config.url.clone();
        match fetch_host_data(&url).await {
            Ok(data) => {
                app.hosts[i].data = Some(data);
                app.hosts[i].status = ConnectionStatus::Connected;
                app.hosts[i].last_fetch = Some(Instant::now());
            }
            Err(_) => {
                app.hosts[i].status = ConnectionStatus::Disconnected;
            }
        }
    }
    app.recalc_column_widths();
    app.rebuild_nav();

    let mut last_refresh = Instant::now();
    let connected_interval = Duration::from_secs(10);
    let disconnected_interval = Duration::from_secs(60);

    loop {
        terminal.draw(|frame| draw(frame, &app))?;

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
                            KeyCode::Down | KeyCode::Char('j') => {
                                let current = app.server_picker_state.selected().unwrap_or(0);
                                if current + 1 < servers.len() {
                                    app.server_picker_state.select(Some(current + 1));
                                }
                            }
                            KeyCode::Enter => {
                                if let Some(idx) = app.server_picker_state.selected() {
                                    if let Some(server) = servers.get(idx) {
                                        open_browser(&server.url);
                                    }
                                }
                                app.overlay = OverlayMode::None;
                            }
                            KeyCode::Char(c) if c.is_ascii_digit() => {
                                let n = c.to_digit(10).unwrap() as usize;
                                if n > 0 && n <= servers.len() {
                                    open_browser(&servers[n - 1].url);
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
                            match fetch_host_data(&url).await {
                                Ok(data) => {
                                    app.hosts[host_idx].data = Some(data);
                                    app.hosts[host_idx].status = ConnectionStatus::Connected;
                                    app.hosts[host_idx].last_fetch = Some(Instant::now());
                                }
                                Err(_) => {
                                    app.hosts[host_idx].status = ConnectionStatus::Disconnected;
                                }
                            }
                            app.recalc_column_widths();
                            app.rebuild_nav();
                        }
                    }
                    KeyCode::Char('R') => {
                        for i in 0..app.hosts.len() {
                            let url = app.hosts[i].config.url.clone();
                            app.hosts[i].status = ConnectionStatus::Connecting;
                            match fetch_host_data(&url).await {
                                Ok(data) => {
                                    app.hosts[i].data = Some(data);
                                    app.hosts[i].status = ConnectionStatus::Connected;
                                    app.hosts[i].last_fetch = Some(Instant::now());
                                }
                                Err(_) => {
                                    app.hosts[i].status = ConnectionStatus::Disconnected;
                                }
                            }
                        }
                        app.recalc_column_widths();
                        app.rebuild_nav();
                    }
                    _ => {}
                }
                app.last_key = None;
            }
        }

        // Background refresh
        if last_refresh.elapsed() > connected_interval {
            for i in 0..app.hosts.len() {
                let interval = match app.hosts[i].status {
                    ConnectionStatus::Connected => connected_interval,
                    _ => disconnected_interval,
                };

                if app.hosts[i].last_fetch.map(|t| t.elapsed() > interval).unwrap_or(true) {
                    let url = app.hosts[i].config.url.clone();
                    match fetch_host_data(&url).await {
                        Ok(data) => {
                            app.hosts[i].data = Some(data);
                            app.hosts[i].status = ConnectionStatus::Connected;
                            app.hosts[i].last_fetch = Some(Instant::now());
                        }
                        Err(_) => {
                            app.hosts[i].status = ConnectionStatus::Disconnected;
                        }
                    }
                }
            }
            app.recalc_column_widths();
            app.rebuild_nav();
            last_refresh = Instant::now();
        }
    }

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;

    Ok(())
}
