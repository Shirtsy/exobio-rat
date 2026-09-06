//! Renders the app's UI for a static journal directory into a plain-text dump.
//! Verification aid only (no pty needed):
//!   cargo run --example render_dump -- <journal dir> [width] [height] [id|value|dist] [threshold]
#![allow(dead_code)]

#[path = "../src/state.rs"]
mod state;
#[path = "../src/ui.rs"]
mod ui;

use std::path::PathBuf;

use ed_journals::fs::LogDir;
use ed_journals::io::LogIter;
use ed_state::state::EventSink;
use ratatui::backend::TestBackend;
use ratatui::layout::Position;
use ratatui::Terminal;

use state::SystemsState;
use ui::Ui;

fn main() {
    let dir = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "/tmp/test-journal".to_string());
    let width = std::env::args()
        .nth(2)
        .and_then(|w| w.parse().ok())
        .unwrap_or(100u16);
    let height = std::env::args()
        .nth(3)
        .and_then(|h| h.parse().ok())
        .unwrap_or(30u16);
    let sort = match std::env::args().nth(4).as_deref() {
        Some("value") => SortMode::Value,
        Some("dist") => SortMode::Distance,
        _ => SortMode::BodyId,
    };
    let threshold = std::env::args()
        .nth(5)
        .and_then(|t| t.parse().ok())
        .unwrap_or(0);

    use ui::SortMode;

    let mut systems = SystemsState::default();
    let log_dir = LogDir::new(PathBuf::from(dir.clone()));
    let mut skipped = 0;
    for log_path in log_dir {
        let path = log_path.unwrap();
        let file = std::fs::File::open(path).unwrap();
        for entry in LogIter::new(std::io::BufReader::new(file)) {
            match entry {
                Ok(event) => {
                    systems.sink_log(&event);
                }
                // Unknown events (newer game versions) are skipped, same as the app.
                Err(_) => skipped += 1,
            }
        }
    }
    if skipped > 0 {
        eprintln!("skipped {skipped} unparseable entries");
    }

    let mut ui = Ui {
        journal_dir: dir,
        sort,
        threshold,
        ..Default::default()
    };
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|frame| ui::draw(frame, &systems, &mut ui)).unwrap();

    let buffer = terminal.backend().buffer();
    for y in 0..height {
        let mut line = String::new();
        for x in 0..width {
            line.push_str(
                buffer
                    .cell(Position { x, y })
                    .map(|cell| cell.symbol())
                    .unwrap_or(" "),
            );
        }
        println!("{}", line.trim_end());
    }
}
