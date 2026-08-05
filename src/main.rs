mod app;
mod capture;
mod recorder;
mod store;
mod ui;

use std::{
    io::{self, Stdout},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use anyhow::{Context, Result, bail};
use app::{App, Tab};
use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};
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
        Command::Recorder => recorder::run(),
        Command::Stop => {
            if recorder::stop()? {
                println!("keypulse recorder stopped");
            } else {
                println!("keypulse recorder is not running");
            }
            Ok(())
        }
    }
}

fn run_dashboard() -> Result<()> {
    // Complete any one-time migration before the recorder and dashboard access SQLite together.
    let mut database = Database::open(&default_database_path()?)?;
    database.migrate_json(&legacy_json_path()?)?;
    drop(database);

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

fn run(tui: &mut Tui, app: &mut App, interrupted: &AtomicBool) -> Result<()> {
    while !interrupted.load(Ordering::Relaxed) {
        app.refresh_if_due();
        tui.draw(|frame| ui::render(frame, app))?;

        if event::poll(Duration::from_millis(80))?
            && let Event::Key(key) = event::read()?
            && key.kind == KeyEventKind::Press
            && handle_key(app, key)
        {
            return Ok(());
        }
    }
    Ok(())
}

fn handle_key(app: &mut App, key: KeyEvent) -> bool {
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
        _ => false,
    }
}

fn start_terminal() -> Result<TerminalSession> {
    enable_raw_mode().context("failed to enable terminal raw mode")?;
    let mut stdout = io::stdout();
    if let Err(error) = execute!(stdout, EnterAlternateScreen) {
        let _ = disable_raw_mode();
        return Err(error).context("failed to enter alternate screen");
    }
    let mut tui = match Terminal::new(CrosstermBackend::new(stdout)) {
        Ok(tui) => tui,
        Err(error) => {
            let _ = disable_raw_mode();
            let _ = execute!(io::stdout(), LeaveAlternateScreen);
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
    let screen_result = execute!(tui.backend_mut(), LeaveAlternateScreen)
        .context("failed to leave alternate screen");
    let cursor_result = tui.show_cursor().context("failed to show terminal cursor");
    raw_mode_result.and(screen_result).and(cursor_result)
}

enum Command {
    Dashboard,
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
                "keypulse {}\n\nPrivate keyboard activity dashboard\n\nUSAGE:\n    keypulse          Open the dashboard and start recording\n    keypulse --stop   Stop the background recorder\n\nThe recorder continues after the dashboard closes.\n\nKEYS:\n    1/2/3/4     Select a tab\n    Left/Right  Change tabs\n    r           Refresh now\n    q, Esc      Close the dashboard",
                env!("CARGO_PKG_VERSION")
            );
            std::process::exit(0);
        }
        "-V" | "--version" => {
            println!("keypulse {}", env!("CARGO_PKG_VERSION"));
            std::process::exit(0);
        }
        "--recorder" if std::env::var_os("KEYPULSE_RECORDER_CHILD").is_some() => {
            Ok(Command::Recorder)
        }
        "--recorder" => bail!("--recorder is an internal option; run keypulse instead"),
        "--stop" => Ok(Command::Stop),
        _ => bail!("unknown argument: {argument}; try --help"),
    }
}
