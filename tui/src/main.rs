use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, List, ListItem, Paragraph},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs,
    io,
    path::PathBuf,
    process::Command,
    time::{Duration, Instant},
};

// Types matching the server API
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Server {
    #[serde(rename = "type")]
    server_type: String,
    port: u16,
    url: String,
    #[serde(default)]
    tailscale_url: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GitHubLinks {
    repo_url: Option<String>,
    branch_url: Option<String>,
    diff_url: Option<String>,
    commits_url: Option<String>,
    last_commit_url: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
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
    status: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ScanResult {
    agents: Vec<AgentInfo>,
    hostname: String,
    scanned_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct HostConfig {
    name: String,
    url: String,
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
    error: Option<String>,
}

// Navigation items
#[derive(Debug, Clone)]
enum NavItem {
    HostHeader(usize),
    Repo(usize, usize),
    StaleServersHeader(usize),
    StaleServerRepo(usize, usize),
    StaleReposHeader(usize),
    StaleRepo(usize, usize),
}

struct App {
    hosts: Vec<HostData>,
    nav_items: Vec<NavItem>,
    selected: usize,
    filter: String,
    show_help: bool,
    host_expanded: HashMap<usize, bool>,
    stale_servers_expanded: HashMap<usize, bool>,
    stale_repos_expanded: HashMap<usize, bool>,
    last_key: Option<char>,
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
                error: None,
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
            show_help: false,
            host_expanded,
            stale_servers_expanded,
            stale_repos_expanded,
            last_key: None,
        };
        app.rebuild_nav();
        app
    }

    fn rebuild_nav(&mut self) {
        self.nav_items.clear();

        // Sort hosts: connected first, then connecting, then disconnected
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

            // Collect indices upfront to avoid borrow issues
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

            // Fresh repos
            for idx in &fresh_indices {
                self.nav_items.push(NavItem::Repo(host_idx, *idx));
            }

            // Stale with servers
            if !stale_server_indices.is_empty() {
                self.nav_items.push(NavItem::StaleServersHeader(host_idx));
                if self.stale_servers_expanded.get(&host_idx).copied().unwrap_or(true) {
                    for idx in &stale_server_indices {
                        self.nav_items.push(NavItem::StaleServerRepo(host_idx, *idx));
                    }
                }
            }

            // Stale repos
            if !stale_indices.is_empty() {
                self.nav_items.push(NavItem::StaleReposHeader(host_idx));
                if self.stale_repos_expanded.get(&host_idx).copied().unwrap_or(false) {
                    for idx in &stale_indices {
                        self.nav_items.push(NavItem::StaleRepo(host_idx, *idx));
                    }
                }
            }
        }

        // Clamp selection
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
                // idx is the original index into agents array
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
}

fn load_config() -> Vec<HostConfig> {
    let config_path = dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("agent-dashboard")
        .join("hosts.json");

    if let Ok(content) = fs::read_to_string(&config_path) {
        if let Ok(hosts) = serde_json::from_str::<Vec<HostConfig>>(&content) {
            return hosts;
        }
    }

    // Default hosts
    vec![
        HostConfig { name: "c-5001".to_string(), url: "http://c-5001:9999".to_string() },
        HostConfig { name: "c-5002".to_string(), url: "http://c-5002:9999".to_string() },
        HostConfig { name: "c-5003".to_string(), url: "http://c-5003:9999".to_string() },
        HostConfig { name: "c-5004".to_string(), url: "http://c-5004:9999".to_string() },
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
        .constraints([Constraint::Min(0), Constraint::Length(1)])
        .split(frame.area());

    let now_ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    let one_day = 86400;

    let mut items: Vec<ListItem> = vec![];

    for (nav_idx, nav_item) in app.nav_items.iter().enumerate() {
        let is_selected = nav_idx == app.selected;
        let style = if is_selected {
            Style::default().bg(Color::Blue).fg(Color::White)
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
                    Span::raw(format!("{} {} ", arrow, host.config.name)),
                    Span::raw(format!("({} repos, {} active) ", repo_count, active_count)),
                    Span::styled(status_char, Style::default().fg(status_color)),
                ])
            }
            NavItem::Repo(host_idx, idx)
            | NavItem::StaleServerRepo(host_idx, idx)
            | NavItem::StaleRepo(host_idx, idx) => {
                // idx is original index into agents array
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

                    let commit = if agent.last_commit.len() > 30 {
                        format!("{}...", &agent.last_commit[..27])
                    } else {
                        agent.last_commit.clone()
                    };

                    let server_indicator = if !agent.servers.is_empty() {
                        Span::styled("● ", Style::default().fg(Color::Green))
                    } else {
                        Span::raw("  ")
                    };

                    Line::from(vec![
                        Span::raw("    "),
                        server_indicator,
                        Span::styled(
                            format!("{:<18}", agent.id),
                            Style::default().fg(Color::Cyan),
                        ),
                        Span::raw(format!("{:<14}", agent.branch)),
                        Span::styled(
                            format!("{:<18}", server_str),
                            Style::default().fg(Color::Yellow),
                        ),
                        Span::raw(format!("{:<25}", commit)),
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
        .block(Block::default().borders(Borders::ALL).title(" agent-dashboard "));

    frame.render_widget(list, chunks[0]);

    // Status bar
    let filter_text = if app.filter.is_empty() {
        String::new()
    } else {
        format!("filter> {} ", app.filter)
    };
    let status = format!(
        "{}↑↓:nav  Enter:open  o:browser  s:server  e:editor  ?:help  q:quit",
        filter_text
    );
    let status_bar = Paragraph::new(status).style(Style::default().fg(Color::DarkGray));
    frame.render_widget(status_bar, chunks[1]);

    // Help overlay
    if app.show_help {
        let help_text = r#"
  agent-dashboard TUI

  NAVIGATION                        ACTIONS
    ↑/↓, j/k     Move selection       Enter   Open terminal
    1-9          Jump to host         o       Open in browser
    Tab          Next host            s       Open server
    gg / G       First / Last         e       Open in editor
    Type         Filter repos

  UTILITY
    r / R        Refresh / Refresh all
    ?            This help
    q            Quit

  Press any key to close...
"#;
        let help_area = centered_rect(60, 60, frame.area());
        let help = Paragraph::new(help_text)
            .block(Block::default().borders(Borders::ALL).title(" Help "))
            .style(Style::default().bg(Color::Black));
        frame.render_widget(ratatui::widgets::Clear, help_area);
        frame.render_widget(help, help_area);
    }
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
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let configs = load_config();
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
            Err(e) => {
                app.hosts[i].status = ConnectionStatus::Disconnected;
                app.hosts[i].error = Some(e.to_string());
            }
        }
    }
    app.rebuild_nav();

    let mut last_refresh = Instant::now();
    let connected_interval = Duration::from_secs(10);
    let disconnected_interval = Duration::from_secs(60);

    loop {
        terminal.draw(|frame| draw(frame, &app))?;

        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if app.show_help {
                    app.show_help = false;
                    continue;
                }

                match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => break,
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
                    KeyCode::Char(c) if c.is_ascii_digit() => {
                        let n = c.to_digit(10).unwrap() as usize;
                        app.jump_to_host(n);
                    }
                    KeyCode::Enter => app.toggle_current(),
                    KeyCode::Char('o') => {
                        if let Some(agent) = app.get_selected_agent() {
                            if let Some(github) = &agent.github {
                                if let Some(url) = &github.branch_url {
                                    open_browser(url);
                                }
                            }
                        }
                    }
                    KeyCode::Char('s') => {
                        if let Some(agent) = app.get_selected_agent() {
                            if let Some(server) = agent.servers.first() {
                                open_browser(&server.url);
                            }
                        }
                    }
                    KeyCode::Char('e') => {
                        if let Some(agent) = app.get_selected_agent() {
                            open_editor(&agent.directory);
                        }
                    }
                    KeyCode::Char('?') | KeyCode::F(1) => {
                        app.show_help = true;
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
                                Err(e) => {
                                    app.hosts[host_idx].status = ConnectionStatus::Disconnected;
                                    app.hosts[host_idx].error = Some(e.to_string());
                                }
                            }
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
                                Err(e) => {
                                    app.hosts[i].status = ConnectionStatus::Disconnected;
                                    app.hosts[i].error = Some(e.to_string());
                                }
                            }
                        }
                        app.rebuild_nav();
                    }
                    KeyCode::Backspace => {
                        app.filter.pop();
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
                        Err(e) => {
                            app.hosts[i].status = ConnectionStatus::Disconnected;
                            app.hosts[i].error = Some(e.to_string());
                        }
                    }
                }
            }
            app.rebuild_nav();
            last_refresh = Instant::now();
        }
    }

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;

    Ok(())
}
