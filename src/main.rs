mod app;
mod capture;
mod recorder;
mod store;
mod ui;

use std::{
    io::{self, Stdout, Write},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use anyhow::{Context, Result, bail};
use app::{App, Tab};
use crossterm::{
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyEventKind,
        KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
    },
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Layout, Rect},
};
use serde::Serialize;
use store::{Database, default_database_path, legacy_json_path};

type Tui = Terminal<CrosstermBackend<Stdout>>;

struct TerminalSession {
    tui: Tui,
    restored: bool,
}

impl TerminalSession {
    fn restore(&mut self) -> Result<()> {
        if self.restored {
            return Ok(());
        }
        self.restored = true;
        restore_terminal(&mut self.tui)
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        if !self.restored {
            let _ = restore_terminal(&mut self.tui);
        }
    }
}

fn main() -> Result<()> {
    match parse_arguments()? {
        Command::Dashboard => run_dashboard(),
        Command::Start => {
            drop(prepare_database()?);
            recorder::ensure_running()?;
            println!("speedy recorder started");
            Ok(())
        }
        Command::Status => print_status(),
        Command::Recorder => recorder::run(),
        Command::Stop => {
            if recorder::stop()? {
                println!("speedy recorder stopped");
            } else {
                println!("speedy recorder is not running");
            }
            Ok(())
        }
    }
}

fn run_dashboard() -> Result<()> {
    // Complete any one-time migration before the recorder and dashboard access SQLite together.
    drop(prepare_database()?);

    recorder::ensure_running()?;
    let database = Database::open(&default_database_path()?)?;
    let stats = database.load_stats()?;
    let mut app = App::new(stats, database);
    app.refresh_if_due();
    let interrupted = Arc::new(AtomicBool::new(false));
    signal_hook::flag::register(signal_hook::consts::SIGTERM, Arc::clone(&interrupted))
        .context("failed to install dashboard stop handler")?;
    signal_hook::flag::register(signal_hook::consts::SIGINT, Arc::clone(&interrupted))
        .context("failed to install dashboard interrupt handler")?;

    let mut terminal = start_terminal()?;
    let run_result = run(&mut terminal.tui, &mut app, &interrupted);
    let restore_result = terminal.restore();
    run_result.and(restore_result)
}

fn prepare_database() -> Result<Database> {
    let mut database = Database::open(&default_database_path()?)?;
    database.migrate_json(&legacy_json_path()?)?;
    Ok(database)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Status {
    today: u64,
    keys_per_minute: usize,
    active: bool,
    device_count: usize,
    daily_target: u64,
}

fn print_status() -> Result<()> {
    let database = prepare_database()?;
    let recorder = database.load_recorder_status()?;
    let today = database
        .load_stats()?
        .total_on(chrono::Local::now().date_naive());
    let status = Status {
        today,
        keys_per_minute: recorder.keys_per_minute,
        active: recorder.active,
        device_count: recorder.device_count,
        daily_target: database.get_daily_target().unwrap_or(app::DAILY_TARGET),
    };
    let stdout = io::stdout();
    let mut output = stdout.lock();
    serde_json::to_writer(&mut output, &status).context("failed to serialize speedy status")?;
    writeln!(output).context("failed to write speedy status")
}

fn run(tui: &mut Tui, app: &mut App, interrupted: &AtomicBool) -> Result<()> {
    while !interrupted.load(Ordering::Relaxed) {
        app.refresh_if_due();
        tui.draw(|frame| ui::render(frame, app))?;

        if event::poll(Duration::from_millis(80))? {
            match event::read()? {
                Event::Key(key) if key.kind == KeyEventKind::Press && handle_key(app, key) => {
                    return Ok(());
                }
                Event::Mouse(mouse) if mouse.kind == MouseEventKind::Down(MouseButton::Left) => {
                    if let Ok(size) = tui.size() {
                        let area = Rect {
                            x: 0,
                            y: 0,
                            width: size.width,
                            height: size.height,
                        };
                        handle_mouse(app, mouse, area);
                    }
                }
                _ => {}
            }
        }
    }
    Ok(())
}

fn handle_mouse(app: &mut App, mouse: MouseEvent, area: Rect) {
    if area.width < 60 || area.height < 32 {
        return;
    }
    let inner = inset(area, 2, 1);
    let page = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(3),
        Constraint::Length(1),
        Constraint::Min(23),
        Constraint::Length(1),
    ])
    .split(inner);
    let tabs = page[2];
    if mouse.column < tabs.x
        || mouse.column >= tabs.x + tabs.width
        || mouse.row < tabs.y
        || mouse.row >= tabs.y + tabs.height
    {
        return;
    }
    let relative_x = mouse.column.saturating_sub(tabs.x);
    let index = (u32::from(relative_x) * 4 / u32::from(tabs.width.max(1))) as usize;
    app.tab = match index {
        0 => Tab::Live,
        1 => Tab::Daily,
        2 => Tab::Hourly,
        _ => Tab::Records,
    };
}

fn inset(area: Rect, horizontal: u16, vertical: u16) -> Rect {
    Rect {
        x: area.x.saturating_add(horizontal),
        y: area.y.saturating_add(vertical),
        width: area.width.saturating_sub(horizontal.saturating_mul(2)),
        height: area.height.saturating_sub(vertical.saturating_mul(2)),
    }
}

fn handle_key(app: &mut App, key: KeyEvent) -> bool {
    if app.editing_target {
        return handle_target_editor_key(app, key);
    }
    match (key.code, key.modifiers) {
        (KeyCode::Char('c'), modifiers) if modifiers.contains(KeyModifiers::CONTROL) => true,
        (KeyCode::Char('q') | KeyCode::Esc, _) => true,
        (KeyCode::Right | KeyCode::Char('l') | KeyCode::Tab, _) => {
            app.tab = app.tab.next();
            false
        }
        (KeyCode::Left | KeyCode::Char('h') | KeyCode::BackTab, _) => {
            app.tab = app.tab.previous();
            false
        }
        (KeyCode::Char('1'), _) => {
            app.tab = Tab::Live;
            false
        }
        (KeyCode::Char('2'), _) => {
            app.tab = Tab::Daily;
            false
        }
        (KeyCode::Char('3'), _) => {
            app.tab = Tab::Hourly;
            false
        }
        (KeyCode::Char('4'), _) => {
            app.tab = Tab::Records;
            false
        }
        (KeyCode::Char('r'), _) => {
            app.refresh_now();
            false
        }
        (KeyCode::Char('t'), _) => {
            app.start_editing_target();
            false
        }
        _ => false,
    }
}

fn handle_target_editor_key(app: &mut App, key: KeyEvent) -> bool {
    match key.code {
        KeyCode::Esc => {
            app.cancel_editing_target();
            false
        }
        KeyCode::Enter => {
            app.confirm_target();
            false
        }
        KeyCode::Backspace => {
            app.target_input.pop();
            app.target_error = None;
            false
        }
        KeyCode::Char(c) if c.is_ascii_digit() => {
            // Keep the input within a reasonable length for a u64 plus separators.
            let digits = app
                .target_input
                .chars()
                .filter(|c| c.is_ascii_digit())
                .count();
            if digits < 10 {
                app.target_input.push(c);
                app.target_error = None;
            }
            false
        }
        KeyCode::Char(',') | KeyCode::Char('_') | KeyCode::Char(' ') => {
            // Allow separators for readability (e.g. "10,000"); they are stripped on save.
            if app.target_input.len() < 14 {
                app.target_input.push(',');
                app.target_error = None;
            }
            false
        }
        _ => false,
    }
}

fn start_terminal() -> Result<TerminalSession> {
    enable_raw_mode().context("failed to enable terminal raw mode")?;
    let mut stdout = io::stdout();
    if let Err(error) = execute!(stdout, EnterAlternateScreen, EnableMouseCapture) {
        let _ = disable_raw_mode();
        return Err(error).context("failed to enter alternate screen");
    }
    let mut tui = match Terminal::new(CrosstermBackend::new(stdout)) {
        Ok(tui) => tui,
        Err(error) => {
            let _ = disable_raw_mode();
            let _ = execute!(io::stdout(), LeaveAlternateScreen, DisableMouseCapture);
            return Err(error).context("failed to initialize terminal");
        }
    };
    if let Err(error) = tui.hide_cursor() {
        let _ = restore_terminal(&mut tui);
        return Err(error).context("failed to hide terminal cursor");
    }
    Ok(TerminalSession {
        tui,
        restored: false,
    })
}

fn restore_terminal(tui: &mut Tui) -> Result<()> {
    let raw_mode_result = disable_raw_mode().context("failed to disable terminal raw mode");
    let screen_result = execute!(tui.backend_mut(), DisableMouseCapture, LeaveAlternateScreen)
        .context("failed to leave alternate screen");
    let cursor_result = tui.show_cursor().context("failed to show terminal cursor");
    raw_mode_result.and(screen_result).and(cursor_result)
}

enum Command {
    Dashboard,
    Start,
    Status,
    Recorder,
    Stop,
}

fn parse_arguments() -> Result<Command> {
    let mut arguments = std::env::args().skip(1);
    let Some(argument) = arguments.next() else {
        return Ok(Command::Dashboard);
    };
    if arguments.next().is_some() {
        bail!("too many arguments; try --help");
    }

    match argument.as_str() {
        "-h" | "--help" => {
            println!(
                "speedy {}\n\nPrivate keyboard activity dashboard\n\nUSAGE:\n    speedy           Open the dashboard and start recording\n    speedy --start   Start the background recorder\n    speedy --status  Print machine-readable recorder status\n    speedy --stop    Stop the background recorder\n\nThe recorder continues after the dashboard closes.\n\nKEYS:\n    1/2/3/4     Select a tab (or click the heading)\n    Left/Right  Change tabs\n    r           Refresh now\n    t           Set daily keypress target\n    q, Esc      Close the dashboard",
                env!("CARGO_PKG_VERSION")
            );
            std::process::exit(0);
        }
        "-V" | "--version" => {
            println!("speedy {}", env!("CARGO_PKG_VERSION"));
            std::process::exit(0);
        }
        "--recorder" if std::env::var_os("SPEEDY_RECORDER_CHILD").is_some() => {
            Ok(Command::Recorder)
        }
        "--recorder" => bail!("--recorder is an internal option; run speedy instead"),
        "--start" => Ok(Command::Start),
        "--status" => Ok(Command::Status),
        "--stop" => Ok(Command::Stop),
        _ => bail!("unknown argument: {argument}; try --help"),
    }
}
