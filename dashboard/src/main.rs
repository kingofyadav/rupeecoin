/// RupeeCoin Validator Dashboard — live-updating ratatui TUI.
///
/// Usage: rupeecoin-dashboard --rpc http://127.0.0.1:9944 [--addr <validator-addr>]
use std::io::{self, stdout};
use std::time::{Duration, Instant};

use clap::Parser;
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Gauge, Paragraph},
};
use rupeecoin_crypto::decode_address;
use serde::Deserialize;

// ── CLI ───────────────────────────────────────────────────────────────────────

#[derive(Parser)]
#[command(
    name = "rupeecoin-dashboard",
    about = "RupeeCoin validator dashboard TUI"
)]
struct Args {
    /// Node RPC base URL.
    #[arg(long, default_value = "http://127.0.0.1:9944")]
    rpc: String,

    /// Validator address (hex or Base58Check) — show personal stats when set.
    #[arg(long)]
    addr: Option<String>,

    /// Refresh interval in seconds.
    #[arg(long, default_value_t = 2)]
    interval: u64,
}

// ── RPC response types ────────────────────────────────────────────────────────

#[derive(Deserialize, Default, Clone)]
struct StatusResp {
    height: u64,
    slot: u64,
    epoch: u64,
    last_justified_epoch: u64,
    last_finalized_epoch: u64,
    active_validators: usize,
    #[serde(default)]
    total_stake: u64,
    mempool_size: usize,
    pending_privacy_txs: usize,
    commitment_count: u64,
    base_fee: u64,
}

#[derive(Deserialize, Default, Clone)]
struct ValidatorInfoResp {
    claimable_rewards: u64,
    stake: u64,
    uptime_score: f64,
    proposal_score: f64,
    total_score: f64,
    is_active: bool,
    status: String,
}

// ── App state ─────────────────────────────────────────────────────────────────

struct App {
    rpc: String,
    addr: Option<String>,
    interval: Duration,
    status: Option<StatusResp>,
    validator: Option<ValidatorInfoResp>,
    last_updated: Option<Instant>,
    last_attempted: Instant,
    error: Option<String>,
}

impl App {
    fn new(args: &Args) -> Self {
        App {
            rpc: args.rpc.clone(),
            addr: args.addr.clone(),
            interval: Duration::from_secs(args.interval),
            status: None,
            validator: None,
            last_updated: None,
            last_attempted: Instant::now() - Duration::from_secs(999),
            error: None,
        }
    }

    fn refresh(&mut self) {
        self.last_attempted = Instant::now();

        match ureq::get(&format!("{}/rpc/status", self.rpc))
            .timeout(Duration::from_secs(3))
            .call()
        {
            Ok(resp) => match resp.into_json::<StatusResp>() {
                Ok(s) => {
                    self.status = Some(s);
                    self.error = None;
                    self.last_updated = Some(Instant::now());
                }
                Err(e) => self.error = Some(format!("JSON error: {e}")),
            },
            Err(e) => {
                self.status = None;
                self.error = Some(format!("RPC unreachable: {e}"));
            }
        }

        if let Some(addr) = &self.addr.clone() {
            let is_valid = (addr.len() == 40 && addr.chars().all(|c| c.is_ascii_hexdigit()))
                || decode_address(addr).is_ok();
            if is_valid {
                let url = format!("{}/rpc/validator_info?addr={}", self.rpc, addr);
                if let Ok(resp) = ureq::get(&url).timeout(Duration::from_secs(3)).call() {
                    if let Ok(v) = resp.into_json::<ValidatorInfoResp>() {
                        self.validator = Some(v);
                    }
                }
            } else {
                self.error = Some(format!("invalid address format: {addr}"));
            }
        }
    }

    fn should_refresh(&self) -> bool {
        self.last_attempted.elapsed() >= self.interval
    }
}

// ── Rendering ─────────────────────────────────────────────────────────────────

fn format_num(n: u64) -> String {
    let s = n.to_string();
    let mut chars: Vec<char> = s.chars().collect();
    let mut i = chars.len() as isize - 3;
    while i > 0 {
        chars.insert(i as usize, ',');
        i -= 3;
    }
    chars.iter().collect()
}

fn rpc_str(micro: u64) -> String {
    format!("{:.4} RPC", micro as f64 / 1e8)
}

fn render_title(frame: &mut Frame, area: Rect, app: &App) {
    let connected = app.error.is_none() && app.status.is_some();
    let dot = if connected { "● " } else { "○ " };
    let conn_str = if connected {
        "Connected"
    } else {
        "Disconnected"
    };
    let update_str = match app.last_updated {
        Some(t) => format!("  Last update: {}s ago", t.elapsed().as_secs()),
        None => "  Waiting…".to_string(),
    };

    let line = Line::from(vec![
        Span::styled(
            "  ₹ RupeeCoin — Validator Dashboard  ",
            Style::new().bold().fg(Color::Yellow),
        ),
        Span::raw("│  "),
        Span::styled(
            dot,
            Style::new().fg(if connected { Color::Green } else { Color::Red }),
        ),
        Span::styled(
            conn_str,
            Style::new().fg(if connected { Color::Green } else { Color::Red }),
        ),
        Span::styled(update_str, Style::new().fg(Color::DarkGray)),
        Span::styled(
            "  │  [q] Quit  [r] Refresh",
            Style::new().fg(Color::DarkGray),
        ),
    ]);
    frame.render_widget(Paragraph::new(line), area);
}

fn render_chain(frame: &mut Frame, area: Rect, app: &App) {
    let block = Block::new()
        .borders(Borders::ALL)
        .title(Span::styled(
            " Chain Status ",
            Style::new().fg(Color::Cyan).bold(),
        ))
        .border_style(Style::new().fg(Color::Cyan));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let lines: Vec<Line> = if let Some(s) = &app.status {
        vec![
            kv_line("Height     ", &format_num(s.height), Color::White),
            kv_line("Slot       ", &format_num(s.slot), Color::White),
            kv_line("Epoch      ", &s.epoch.to_string(), Color::White),
            kv_line(
                "Justified  ",
                &s.last_justified_epoch.to_string(),
                Color::Green,
            ),
            kv_line(
                "Finalized  ",
                &s.last_finalized_epoch.to_string(),
                Color::Green,
            ),
            Line::raw(""),
            kv_line("Base Fee   ", &rpc_str(s.base_fee), Color::White),
            kv_line(
                "Validators ",
                &format!("{} active", s.active_validators),
                Color::White,
            ),
            kv_line(
                "Total Stake",
                &format!("{:.2} RPC", s.total_stake as f64 / 1e8),
                Color::White,
            ),
        ]
    } else if let Some(e) = &app.error {
        vec![
            Line::raw(""),
            Line::from(Span::styled(format!("  {e}"), Style::new().fg(Color::Red))),
        ]
    } else {
        vec![Line::from(Span::styled(
            "  Connecting…",
            Style::new().fg(Color::Yellow),
        ))]
    };

    frame.render_widget(Paragraph::new(lines), inner);
}

fn render_pool(frame: &mut Frame, area: Rect, app: &App) {
    let block = Block::new()
        .borders(Borders::ALL)
        .title(Span::styled(
            " Mempool & Privacy Pool ",
            Style::new().fg(Color::Cyan).bold(),
        ))
        .border_style(Style::new().fg(Color::Cyan));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let lines: Vec<Line> = if let Some(s) = &app.status {
        vec![
            kv_line("Mempool txs    ", &s.mempool_size.to_string(), Color::White),
            kv_line(
                "Privacy pending",
                &s.pending_privacy_txs.to_string(),
                Color::White,
            ),
            kv_line(
                "Shielded notes ",
                &format_num(s.commitment_count),
                Color::White,
            ),
        ]
    } else {
        vec![]
    };

    frame.render_widget(Paragraph::new(lines), inner);
}

fn render_validator(frame: &mut Frame, area: Rect, app: &App) {
    let title = if app.addr.is_some() {
        " Validator Status "
    } else {
        " Validator (no --addr) "
    };
    let block = Block::new()
        .borders(Borders::ALL)
        .title(Span::styled(title, Style::new().fg(Color::Magenta).bold()))
        .border_style(Style::new().fg(Color::Magenta));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if app.addr.is_none() {
        frame.render_widget(
            Paragraph::new(vec![
                Line::raw(""),
                Line::from(Span::styled(
                    "  Pass --addr <validator-address>",
                    Style::new().fg(Color::Yellow),
                )),
                Line::from(Span::styled(
                    "  to see personal validator stats.",
                    Style::new().fg(Color::DarkGray),
                )),
            ]),
            inner,
        );
        return;
    }

    let Some(v) = &app.validator else {
        frame.render_widget(
            Paragraph::new(Span::styled("  Loading…", Style::new().fg(Color::Yellow))),
            inner,
        );
        return;
    };

    // Split inner area: text info on top, gauge bars below.
    let sections = Layout::vertical([Constraint::Length(5), Constraint::Min(0)]).split(inner);

    let addr_str = app.addr.as_deref().unwrap_or("");
    let addr_disp = if addr_str.len() > 18 {
        format!("{}…{}", &addr_str[..10], &addr_str[addr_str.len() - 6..])
    } else {
        addr_str.to_string()
    };

    let status_color = if v.is_active {
        Color::Green
    } else {
        Color::Red
    };

    let info_lines = vec![
        kv_line("Address", &addr_disp, Color::Cyan),
        kv_line("Status ", &v.status, status_color),
        kv_line(
            "Stake  ",
            &format!("{:.2} RPC", v.stake as f64 / 1e8),
            Color::White,
        ),
        kv_line("Rewards", &rpc_str(v.claimable_rewards), Color::Yellow),
    ];
    frame.render_widget(Paragraph::new(info_lines), sections[0]);

    // Score gauges
    render_score_gauge(
        frame,
        sections[1],
        v.uptime_score,
        v.proposal_score,
        v.total_score,
    );
}

fn render_score_gauge(frame: &mut Frame, area: Rect, uptime: f64, proposal: f64, score: f64) {
    let rows = Layout::vertical([
        Constraint::Length(2),
        Constraint::Length(2),
        Constraint::Length(2),
        Constraint::Min(0),
    ])
    .split(area);

    let gauges = [
        ("  Uptime   ", uptime, Color::Green),
        ("  Proposal ", proposal, Color::Blue),
        ("  Score    ", score, Color::Yellow),
    ];

    for (i, (label, ratio, color)) in gauges.iter().enumerate() {
        let ratio = ratio.clamp(0.0, 1.0);
        let pct = (ratio * 100.0).round() as u16;
        let gauge = Gauge::default()
            .block(Block::new().title(*label))
            .gauge_style(Style::new().fg(*color).bg(Color::DarkGray))
            .ratio(ratio)
            .label(format!("{pct}%"));
        frame.render_widget(gauge, rows[i]);
    }
}

fn kv_line<'a>(key: &'a str, val: &str, val_color: Color) -> Line<'a> {
    Line::from(vec![
        Span::styled(format!("  {key}  "), Style::new().fg(Color::DarkGray)),
        Span::styled(val.to_string(), Style::new().fg(val_color)),
    ])
}

fn render_status_bar(frame: &mut Frame, area: Rect, app: &App) {
    let text = if let Some(e) = &app.error {
        format!(" ✖  {e}")
    } else if app.status.is_some() {
        format!(
            " ✓  Connected  │  RPC: {}  │  refresh every {}s",
            app.rpc,
            app.interval.as_secs()
        )
    } else {
        format!(" ○  Connecting to {} …", app.rpc)
    };

    let style = if app.error.is_some() {
        Style::new().fg(Color::Red)
    } else if app.status.is_some() {
        Style::new().fg(Color::Green)
    } else {
        Style::new().fg(Color::Yellow)
    };

    frame.render_widget(Paragraph::new(Span::styled(text, style)), area);
}

fn ui(frame: &mut Frame, app: &App) {
    let root = Layout::vertical([
        Constraint::Length(1), // title
        Constraint::Min(0),    // main
        Constraint::Length(1), // status bar
    ])
    .split(frame.area());

    render_title(frame, root[0], app);

    // Main area: two columns
    let columns =
        Layout::horizontal([Constraint::Percentage(56), Constraint::Percentage(44)]).split(root[1]);

    // Left column: chain info on top, pool stats on bottom
    let left = Layout::vertical([Constraint::Min(0), Constraint::Length(5)]).split(columns[0]);

    render_chain(frame, left[0], app);
    render_pool(frame, left[1], app);
    render_validator(frame, columns[1], app);
    render_status_bar(frame, root[2], app);
}

// ── Entry point ───────────────────────────────────────────────────────────────

fn main() -> io::Result<()> {
    let args = Args::parse();
    let mut app = App::new(&args);

    // Enter alternate screen and raw mode.
    enable_raw_mode()?;
    execute!(stdout(), EnterAlternateScreen)?;

    let mut terminal = Terminal::new(CrosstermBackend::new(stdout()))?;

    let result = run_loop(&mut terminal, &mut app);

    // Restore terminal on exit.
    disable_raw_mode()?;
    execute!(stdout(), LeaveAlternateScreen)?;

    result
}

fn run_loop<B: Backend>(terminal: &mut Terminal<B>, app: &mut App) -> io::Result<()> {
    // Do an initial refresh immediately.
    app.refresh();

    loop {
        terminal.draw(|frame| ui(frame, app))?;

        // Poll for keyboard events every 100ms.
        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    match key.code {
                        KeyCode::Char('q') | KeyCode::Char('Q') | KeyCode::Esc => break,
                        KeyCode::Char('r') | KeyCode::Char('R') => app.refresh(),
                        _ => {}
                    }
                }
            }
        }

        // Periodic refresh.
        if app.should_refresh() {
            app.refresh();
        }
    }

    Ok(())
}
