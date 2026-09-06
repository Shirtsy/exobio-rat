mod state;
mod ui;

use std::io::Stdout;
use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::Arc;
use std::time::Duration;

use crossterm::event::{self, Event, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{self, EnterAlternateScreen, LeaveAlternateScreen};
use ed_journals::fs::{
    auto_detect_journal_path, common::NewestFile, DirWatcher, LogDir, SyncBlocker, Unblocker,
};
use ed_journals::logs::LogEvent;
use ed_state::state::EventSink;
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;

use state::SystemsState;
use ui::{KeyAction, Ui};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let journal_dir = journal_dir_from_args()?;
    if !journal_dir.is_dir() {
        return Err(format!(
            "journal directory does not exist: {}",
            journal_dir.display()
        )
        .into());
    }
    use std::io::IsTerminal;
    if !std::io::stdout().is_terminal() {
        return Err("stdout is not a terminal, run this from an interactive terminal".into());
    }

    let (tx, rx) = mpsc::channel::<LogEvent>();
    let reader_dir = journal_dir.clone();
    std::thread::spawn(move || reader(reader_dir, tx));

    // Restore the terminal even if we panic in the middle of the TUI.
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore_terminal();
        original_hook(info);
    }));

    let mut ui = Ui {
        journal_dir: journal_dir.display().to_string(),
        ..Default::default()
    };
    let mut systems = SystemsState::default();

    terminal::enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout))?;

    let result = app_loop(&mut terminal, &mut systems, &mut ui, &rx);

    restore_terminal();

    result
}

fn app_loop(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    systems: &mut SystemsState,
    ui: &mut Ui,
    rx: &mpsc::Receiver<LogEvent>,
) -> Result<(), Box<dyn std::error::Error>> {
    loop {
        // Drain everything the reader has produced, then wait for input with
        // a short timeout that doubles as the idle redraw tick.
        while let Ok(event) = rx.try_recv() {
            systems.sink_log(&event);
        }

        if event::poll(Duration::from_millis(250))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    if ui.handle_key(key.code, key.modifiers) == KeyAction::Quit {
                        return Ok(());
                    }
                }
            }
        }

        terminal.draw(|frame| ui::draw(frame, systems, ui))?;
    }
}

/// Reads the journal live in the background: watches the directory, follows
/// the newest log file, and sends every parsed event over the channel.
fn reader(journal_dir: PathBuf, tx: mpsc::Sender<LogEvent>) {
    let mut dir = LogDir::new(journal_dir);
    let mut blocker = SyncBlocker::new();
    let unblocker: Arc<dyn Unblocker> = (&blocker).into();

    let _watcher = match DirWatcher::new(&dir, unblocker.clone()) {
        Ok(watcher) => watcher,
        Err(error) => {
            eprintln!("failed to watch journal directory: {error}");
            return;
        }
    };

    let mut newest = NewestFile::new(unblocker);

    loop {
        if let Some(Ok(path)) = dir.last_n(1) {
            if newest.maybe_new(&path).is_err_and(|error| {
                eprintln!("failed to open log file: {error}");
                true
            }) {
                continue;
            }
            for event in newest.by_ref() {
                match event {
                    Ok(event) => {
                        // The app quit: stop reading.
                        if tx.send(event).is_err() {
                            return;
                        }
                    }
                    Err(error) => eprintln!("failed to read log entry: {error}"),
                }
            }
        }

        // Unblock when the directory or the current file changes. If the
        // unblocker is gone (watcher died), shut down.
        if blocker.wait().is_err() {
            return;
        }
    }
}

fn journal_dir_from_args() -> Result<PathBuf, Box<dyn std::error::Error>> {
    let usage = "usage: exobio-rat [--dir <journal directory>]";
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--dir" {
            return Ok(args.next().ok_or(usage.to_string())?.into());
        }
        return Err(format!("unknown argument: {arg} ({usage})").into());
    }
    auto_detect_journal_path()
        .ok_or("could not auto-detect the journal directory, pass --dir <path>".into())
}

fn restore_terminal() {
    let _ = terminal::disable_raw_mode();
    let _ = execute!(std::io::stdout(), LeaveAlternateScreen);
}
