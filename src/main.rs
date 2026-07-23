mod app;
mod capture;
mod store;
mod ui;

use std::{
    io::{self, Stdout},
    sync::mpsc::Receiver,
    thread,
    time::Duration,
};

use anyhow::{Context, Result, bail};
use app::{App, Tab};
use capture::{CaptureEvent, CaptureHandle};
use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};
use store::{Database, default_database_path, legacy_json_path};

type Tui = Terminal<CrosstermBackend<Stdout>>;

fn main() -> Result<()> {
    parse_arguments()?;

    let mut database = Database::open(&default_database_path()?)?;
    database.migrate_json(&legacy_json_path()?)?;
    let stats = database.load_stats()?;
    let mut app = App::new(stats, database);
    let (capture_sender, capture_receiver) = std::sync::mpsc::channel();
    let _capture = CaptureHandle::start(capture_sender);

    let mut terminal = start_terminal()?;
    let run_result = run(&mut terminal, &mut app, &capture_receiver);
    let restore_result = restore_terminal(&mut terminal);

    // Give the input worker a moment to deliver the key used to quit.
    thread::sleep(Duration::from_millis(12));
    drain_capture(&mut app, &capture_receiver);
    let save_result = if app.is_dirty() { app.save() } else { Ok(()) };

    run_result.and(restore_result).and(save_result)
}

fn run(tui: &mut Tui, app: &mut App, receiver: &Receiver<CaptureEvent>) -> Result<()> {
    loop {
        drain_capture(app, receiver);
        app.save_if_due();
        tui.draw(|frame| ui::render(frame, app))?;

        if event::poll(Duration::from_millis(80))?
            && let Event::Key(key) = event::read()?
            && key.kind == KeyEventKind::Press
            && handle_key(app, key)
        {
            return Ok(());
        }
    }
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
            app.tab = Tab::Today;
            false
        }
        (KeyCode::Char('2'), _) => {
            app.tab = Tab::Week;
            false
        }
        (KeyCode::Char('3'), _) => {
            app.tab = Tab::Report;
            false
        }
        _ => false,
    }
}

fn drain_capture(app: &mut App, receiver: &Receiver<CaptureEvent>) {
    while let Ok(event) = receiver.try_recv() {
        app.handle_capture(event);
    }
}

fn start_terminal() -> Result<Tui> {
    enable_raw_mode().context("failed to enable terminal raw mode")?;
    let mut stdout = io::stdout();
    if let Err(error) = execute!(stdout, EnterAlternateScreen) {
        let _ = disable_raw_mode();
        return Err(error).context("failed to enter alternate screen");
    }
    Terminal::new(CrosstermBackend::new(stdout)).context("failed to initialize terminal")
}

fn restore_terminal(tui: &mut Tui) -> Result<()> {
    let raw_mode_result = disable_raw_mode().context("failed to disable terminal raw mode");
    let screen_result = execute!(tui.backend_mut(), LeaveAlternateScreen)
        .context("failed to leave alternate screen");
    let cursor_result = tui.show_cursor().context("failed to show terminal cursor");
    raw_mode_result.and(screen_result).and(cursor_result)
}

fn parse_arguments() -> Result<()> {
    let mut arguments = std::env::args().skip(1);
    let Some(argument) = arguments.next() else {
        return Ok(());
    };

    match argument.as_str() {
        "-h" | "--help" => {
            println!(
                "keypulse {}\n\nPrivate keyboard activity dashboard\n\nUSAGE:\n    keypulse\n\nKEYS:\n    1/2/3       Select a tab\n    Left/Right  Change tabs\n    q, Esc      Quit",
                env!("CARGO_PKG_VERSION")
            );
            std::process::exit(0);
        }
        "-V" | "--version" => {
            println!("keypulse {}", env!("CARGO_PKG_VERSION"));
            std::process::exit(0);
        }
        _ => bail!("unknown argument: {argument}; try --help"),
    }
}
