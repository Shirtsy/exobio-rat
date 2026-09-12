use std::collections::HashMap;
use std::path::Path;

use arboard::Clipboard;
use crossterm::event::{KeyCode, KeyModifiers};
use ed_journals::exobiology::Species;
use ed_state::system::{PlanetSpeciesEntry, PlanetState, SystemState};
use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Table, TableState};

use crate::route::Route;
use crate::state::{SystemsState, is_active, is_complete};

/// Table columns: near marker, name, scan, bio, est value, exobio.
const WIDTHS: [Constraint; 3] = [
    Constraint::Length(2),
    Constraint::Min(22),
    Constraint::Length(13),
];

#[derive(Default)]
pub struct Ui {
    pub journal_dir: String,
    pub scroll: usize,
    pub sort: SortMode,
    pub threshold: u64,
    pub input: Option<Input>,
    pub last_system: Option<u64>,
    pub route: Option<Route>,
    /// Set when a clipboard write failed; the route panel shows it.
    pub copy_error: bool,
    /// Kept for the app's lifetime: dropping the clipboard soon after a
    /// write can make clipboard managers miss the contents (on X11 it also
    /// triggers a debug-mode eprintln that would garble the TUI).
    pub clipboard: Option<Clipboard>,
}

pub struct Input {
    pub mode: InputMode,
    pub buffer: String,
    pub error: Option<String>,
}

pub enum InputMode {
    Threshold,
    RoutePath,
}

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum SortMode {
    #[default]
    BodyId,
    Value,
    Distance,
}

impl SortMode {
    fn label(self) -> &'static str {
        match self {
            SortMode::BodyId => "body id",
            SortMode::Value => "value",
            SortMode::Distance => "distance",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum KeyAction {
    Keep,
    Quit,
    CopyNext,
}

impl Ui {
    pub fn handle_key(&mut self, code: KeyCode, modifiers: KeyModifiers) -> KeyAction {
        // While the threshold popup is open it swallows all keys.
        if self.input.is_some() {
            match code {
                KeyCode::Esc => self.input = None,
                KeyCode::Enter => match self.input.as_ref().unwrap().mode {
                    InputMode::Threshold => {
                        let buffer = self.input.as_ref().unwrap().buffer.clone();
                        match buffer.parse::<u64>() {
                            Ok(value) => {
                                self.threshold = value;
                                self.input = None;
                            }
                            Err(_) => {
                                self.input.as_mut().unwrap().error =
                                    Some("not a valid number".to_string())
                            }
                        }
                    }
                    InputMode::RoutePath => {
                        let buffer = self.input.as_ref().unwrap().buffer.trim().to_string();
                        if buffer.is_empty() {
                            self.input.as_mut().unwrap().error =
                                Some("enter a route file path".to_string());
                        } else {
                            match Route::from_file(Path::new(&buffer)) {
                                Ok(route) => {
                                    self.route = Some(route);
                                    self.input = None;
                                }
                                Err(error) => {
                                    self.input.as_mut().unwrap().error = Some(error)
                                }
                            }
                        }
                    }
                }
                KeyCode::Backspace => {
                    self.input.as_mut().unwrap().buffer.pop();
                    self.input.as_mut().unwrap().error = None;
                }
                KeyCode::Char(c) if !modifiers.contains(KeyModifiers::CONTROL) => {
                    self.input.as_mut().unwrap().buffer.push(c);
                    self.input.as_mut().unwrap().error = None;
                }
                _ => {}
            }
            return KeyAction::Keep;
        }

        match code {
            KeyCode::Char('q') | KeyCode::Esc => return KeyAction::Quit,
            KeyCode::Char('k') => self.scroll = self.scroll.saturating_sub(3),
            KeyCode::Char('j') => self.scroll += 3,
            KeyCode::Char('v') => self.sort = SortMode::Value,
            KeyCode::Char('d') => self.sort = SortMode::Distance,
            KeyCode::Char('t') => {
                self.input = Some(Input {
                    mode: InputMode::Threshold,
                    buffer: String::new(),
                    error: None,
                })
            }
            KeyCode::Char('r') => {
                self.input = Some(Input {
                    mode: InputMode::RoutePath,
                    buffer: String::new(),
                    error: None,
                })
            }
            KeyCode::Char('c') if self.route.as_ref().is_some_and(|route| !route.complete()) => {
                return KeyAction::CopyNext
            }
            _ => {}
        }
        KeyAction::Keep
    }
}

/// Per-planet data prepared once per frame for sorting and rendering.
struct View<'a> {
    id: u8,
    planet: &'a PlanetState,
    name: String,
    entries: Vec<PlanetSpeciesEntry>,
    complete: bool,
    estimated: u64,
    distance: Option<f32>,
}

fn species_entries(system: &SystemState, planet: &PlanetState) -> Vec<PlanetSpeciesEntry> {
    match &system.exobiology_system {
        Some(target_system) => planet.get_planet_species(target_system),
        None => Vec::new(),
    }
}

fn estimated_value(system: &SystemState, planet: &PlanetState) -> u64 {
    match &system.exobiology_system {
        Some(target_system) => planet.get_lowest_exobiology_value(target_system),
        None => 0,
    }
}

fn body_name(
    system: &SystemState,
    id: u8,
    planet: &PlanetState,
    names: &HashMap<(u64, u8), String>,
) -> String {
    planet
        .scan
        .as_ref()
        .map(|scan| scan.body_name.clone())
        .or_else(|| planet.saa_scan.as_ref().map(|scan| scan.body_name.clone()))
        .or_else(|| names.get(&(system.system_address, id)).cloned())
        .unwrap_or_else(|| format!("Body {id}"))
}

fn build_views<'a>(
    system: &'a SystemState,
    ui: &Ui,
    names: &HashMap<(u64, u8), String>,
) -> Vec<View<'a>> {
    system
        .planet_state
        .iter()
        // Only planets with biological signals are exobio targets; the rest
        // (including phantom entries for body ids that never got a scan) is
        // noise.
        .filter(|(_, planet)| planet.has_biological_signals())
        .map(|(id, planet)| {
            let entries = species_entries(system, planet);
            View {
                id: *id,
                planet,
                name: body_name(system, *id, planet, names),
                complete: is_complete(&entries, ui.threshold),
                estimated: estimated_value(system, planet),
                distance: planet
                    .scan
                    .as_ref()
                    .map(|scan| scan.distance_from_arrival.as_ls()),
                entries,
            }
        })
        .collect()
}

pub fn draw(frame: &mut Frame, systems: &SystemsState, ui: &mut Ui) {
    let area = frame.area();
    let has_route = ui.route.is_some();
    let chunks = if has_route {
        Layout::vertical([
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(2),
            Constraint::Length(1),
        ])
        .split(area)
    } else {
        Layout::vertical([
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .split(area)
    };

    let (system_address, system) = match systems.current {
        Some(address) => (Some(address), systems.systems.get(&address)),
        None => (None, None),
    };

    // Start at the top of the list when entering a new system.
    if ui.last_system != system_address {
        ui.scroll = 0;
        ui.last_system = system_address;
    }

    let views = system
        .map(|system| build_views(system, ui, &systems.planet_names))
        .unwrap_or_default();

    draw_header(frame, chunks[0], system, &systems.planet_names, ui);
    draw_list(frame, chunks[1], system, &views, ui);
    if let Some(route) = &ui.route {
        draw_route(frame, chunks[2], route, ui);
        draw_footer(frame, chunks[3], ui);
    } else {
        draw_footer(frame, chunks[2], ui);
    }

    if let Some(input) = &ui.input {
        draw_popup(frame, area, input);
    }
}

fn draw_header(
    frame: &mut Frame,
    area: Rect,
    system: Option<&SystemState>,
    names: &HashMap<(u64, u8), String>,
    ui: &Ui,
) {
    let name = system
        .and_then(|system| system.location_info.as_ref())
        .map(|info| info.star_system.as_str())
        .unwrap_or("waiting for system…");

    // Count bodies that actually received a scan (planets + stars). The raw
    // planet_state map also holds phantom ids (e.g. the system barycentre)
    // that never got scanned.
    let bodies = match system {
        Some(system) => {
            let scanned = system
                .planet_state
                .values()
                .filter(|planet| planet.scan.is_some())
                .count()
                + system.star_scans.len();
            match system.number_of_bodies {
                Some(total) => format!("{scanned}/{total} bodies"),
                None => format!("{scanned} bodies scanned"),
            }
        }
        None => String::new(),
    };

    let near = system
        .and_then(|system| {
            system.near_body.and_then(|id| {
                system
                    .planet_state
                    .get(&id)
                    .map(|planet| body_name(system, id, planet, names))
            })
        })
        .unwrap_or_else(|| "—".to_string());

    let line = Line::from(vec![
        Span::styled(
            format!(" {name}"),
            Style::default().add_modifier(Modifier::BOLD),
        ),
        Span::raw(format!("  {bodies}")),
        Span::raw(format!("  sort: {}", ui.sort.label())),
        Span::styled(
            format!("  threshold: {}", group(ui.threshold)),
            Style::default().fg(Color::Cyan),
        ),
        Span::styled(
            format!("  near: {near}"),
            Style::default().fg(Color::Yellow),
        ),
    ]);

    frame.render_widget(Paragraph::new(line), area);
}

fn draw_list(
    frame: &mut Frame,
    area: Rect,
    system: Option<&SystemState>,
    views: &[View<'_>],
    ui: &mut Ui,
) {
    let dim = Style::default().add_modifier(Modifier::DIM);

    let Some(system) = system else {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                " Waiting for journal events…",
                dim,
            ))),
            area,
        );
        return;
    };

    // Sort without cloning: PlanetSpeciesEntry is not Clone.
    let mut order: Vec<usize> = (0..views.len()).collect();
    order.sort_by(|&a, &b| {
        let (va, vb) = (&views[a], &views[b]);
        va.complete.cmp(&vb.complete).then_with(|| match ui.sort {
            SortMode::BodyId => va.id.cmp(&vb.id),
            SortMode::Value => vb.estimated.cmp(&va.estimated),
            SortMode::Distance => {
                // Unscanned bodies have no distance; they sort last.
                let ka = va.distance.map_or((1, 0.0f32), |d| (0, d));
                let kb = vb.distance.map_or((1, 0.0f32), |d| (0, d));
                ka.0.cmp(&kb.0).then_with(|| ka.1.total_cmp(&kb.1))
            }
        })
    });

    let near = system.near_body;
    let mut rows: Vec<Row<'static>> = Vec::new();
    let mut header_rows: Vec<(u8, usize)> = Vec::new();
    for &index in &order {
        let view = &views[index];
        header_rows.push((view.id, rows.len()));
        rows.push(planet_header_row(view, Some(view.id) == near));
        rows.extend(planet_inset_rows(view, ui));
    }

    if rows.is_empty() {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                " No planets with biological signals. ",
                dim,
            ))),
            area,
        );
        return;
    }

    // Auto-scroll to keep the near planet visible; manual scroll clamps it.
    // The table header takes one row.
    let visible = area.height.saturating_sub(1) as usize;
    if let Some(row) = near.and_then(|body| {
        header_rows
            .iter()
            .find(|(id, _)| *id == body)
            .map(|(_, row)| *row)
    }) {
        if row < ui.scroll {
            ui.scroll = row;
        } else if row + 1 > ui.scroll + visible {
            ui.scroll = row.saturating_sub(visible.saturating_sub(1));
        }
    }
    ui.scroll = ui.scroll.min(rows.len().saturating_sub(visible));

    let table = Table::new(rows, WIDTHS).column_spacing(1).header(
        Row::new(vec!["", "name", "est value"])
            .style(Style::default().add_modifier(Modifier::BOLD)),
    );

    let mut table_state = TableState::default();
    *table_state.offset_mut() = ui.scroll;
    frame.render_stateful_widget(table, area, &mut table_state);
}

fn planet_header_row(view: &View<'_>, near: bool) -> Row<'static> {
    let base = Style::default();
    let style = if view.complete {
        base.add_modifier(Modifier::DIM)
    } else if near {
        base.add_modifier(Modifier::BOLD).fg(Color::Yellow)
    } else {
        base
    };

    Row::new(vec![
        Cell::new(if near { "@" } else { " " }).style(if near {
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::DarkGray)
        }),
        Cell::new(view.name.clone()).style(style),
        Cell::new(format!("{:>12}", group(view.estimated))).style(style),
    ])
}

/// A row indented under a planet header: content in the name column, optional
/// value in the est-value column.
fn inset_row(name: Line<'static>, value: Option<u64>) -> Row<'static> {
    let dim = Style::default().add_modifier(Modifier::DIM);
    Row::new(vec![
        Cell::new(" "),
        Cell::new(name),
        match value {
            Some(value) => Cell::new(format!("{:>12}", group(value))).style(dim),
            None => Cell::new(""),
        },
    ])
}

fn planet_inset_rows(view: &View<'_>, ui: &Ui) -> Vec<Row<'static>> {
    let mut rows: Vec<Row<'static>> = Vec::new();
    let dim = Style::default().add_modifier(Modifier::DIM);

    if let Some(signals) = &view.planet.signal_counts {
        let kinds = [
            (signals.biological_signal_count, "bio", Color::Green),
            (signals.human_signal_count, "human", Color::Blue),
            (signals.geological_signal_count, "gel", Color::Yellow),
            (signals.thargoid_signal_count, "tharg", Color::Magenta),
            (signals.guardian_signal_count, "guard", Color::Cyan),
            (signals.other_signal_count, "other", Color::Gray),
        ]
        .into_iter()
        .filter(|(count, _, _)| *count > 0);

        let mut spans: Vec<Span<'static>> = vec![Span::styled("  signals:", dim)];
        let mut any = false;
        for (count, label, color) in kinds {
            any = true;
            spans.push(Span::raw(" "));
            spans.push(Span::styled(
                format!("{label} {count}"),
                Style::default().fg(color),
            ));
        }
        if !any {
            spans.push(Span::styled(" none", dim));
        }
        rows.push(inset_row(Line::from(spans), None));
    }

    // Species checklist: active incomplete first, then completed, then inactive.
    let mut items: Vec<(u8, String, Style, String, u64)> = Vec::new();
    let mut excluded = 0;

    for entry in &view.entries {
        let active = is_active(entry, ui.threshold);
        if entry.will_spawn.no() && !entry.confirmed {
            excluded += 1;
            continue;
        }

        let organic = view.planet.organics.get(&entry.species.genus());
        let label = match organic.and_then(|organic| organic.variant.as_ref()) {
            Some(variant) => variant.to_string(),
            None => entry.species.to_string(),
        };
        let value = entry.species.base_value();

        let (bucket, marker, marker_style) = if entry.completed {
            (1, "✓✓✓".to_string(), Style::default().fg(Color::Green))
        } else if !active {
            (2, "·".to_string(), dim)
        } else if entry.confirmed {
            let progress = organic.map(|organic| organic.progress_nr()).unwrap_or(1);
            (
                0,
                format!("✓ {progress}/3"),
                Style::default().fg(Color::Green),
            )
        } else {
            (0, "?".to_string(), Style::default().fg(Color::Yellow))
        };

        items.push((bucket, marker, marker_style, label, value));
    }

    // Scanned organics the prediction did not list.
    let predicted: Vec<Species> = view
        .entries
        .iter()
        .map(|entry| entry.species.clone())
        .collect();
    for organic in view.planet.organics.values() {
        if predicted.contains(&organic.species) {
            continue;
        }
        let (bucket, marker) = if organic.is_completed() {
            (1, "✓✓✓".to_string())
        } else {
            (0, format!("✓ {}/3", organic.progress_nr()))
        };
        items.push((
            bucket,
            marker,
            Style::default().fg(Color::Green),
            format!("{} (not predicted)", organic.species),
            organic.species.base_value(),
        ));
    }

    items.sort_by(|(a, _, _, _, va), (b, _, _, _, vb)| a.cmp(b).then(vb.cmp(va)));
    for (bucket, marker, marker_style, label, value) in items {
        let text_style = if bucket == 0 { Style::default() } else { dim };
        rows.push(inset_row(
            Line::from(vec![
                Span::styled(format!("{marker:<5}"), marker_style),
                Span::styled(label, text_style),
            ]),
            Some(value),
        ));
    }

    if view.entries.is_empty() {
        let hint = if view.planet.scan.is_none() {
            "  scan this body to predict flora"
        } else if view
            .planet
            .exobiology_body
            .as_ref()
            .is_some_and(|body| !body.landable)
        {
            "  not landable"
        } else {
            "  no spawnable species predicted"
        };
        rows.push(inset_row(Line::from(Span::styled(hint, dim)), None));
    } else if excluded > 0 {
        rows.push(inset_row(
            Line::from(Span::styled(
                format!("  — {excluded} species excluded (impossible)"),
                dim,
            )),
            None,
        ));
    }

    rows
}

fn draw_route(frame: &mut Frame, area: Rect, route: &Route, ui: &Ui) {
    let dim = Style::default().add_modifier(Modifier::DIM);

    let (first, second) = match route.next() {
        // Route complete: one line, that's it.
        None => (
            Line::from(Span::styled(
                " route complete ✓ ",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::default(),
        ),
        Some(next) => {
            let (visited, total) = (route.visited(), route.len());
            let last = route
                .last()
                .map(|stop| format!("  last: {}", stop.name))
                .unwrap_or_default();
            let first = Line::from(vec![
                Span::styled(
                    format!(" route {visited}/{total} "),
                    Style::default().add_modifier(Modifier::BOLD),
                ),
                Span::raw(progress_bar(visited, total, 20)),
                Span::styled(format!(" {}%", visited * 100 / total), dim),
                Span::styled(last, dim),
            ]);
            let copy_hint = if ui.copy_error {
                Span::styled("  clipboard unavailable", Style::default().fg(Color::Red))
            } else {
                Span::styled("  [c] copy", dim)
            };
            let second = Line::from(vec![
                Span::raw(" next: "),
                Span::styled(next.name.clone(), Style::default().add_modifier(Modifier::BOLD)),
                Span::styled(
                    format!(" ({} {})", next.jumps, jump_word(next.jumps)),
                    dim,
                ),
                copy_hint,
            ]);
            (first, second)
        }
    };

    frame.render_widget(Paragraph::new(vec![first, second]), area);
}

/// A fixed-width block bar: filled for visited stops, empty for the rest.
fn progress_bar(visited: usize, total: usize, width: usize) -> String {
    let filled = visited * width / total;
    format!("{}{}", "▓".repeat(filled), "░".repeat(width - filled))
}

fn jump_word(jumps: u32) -> &'static str {
    if jumps == 1 { "jump" } else { "jumps" }
}

fn draw_footer(frame: &mut Frame, area: Rect, ui: &Ui) {
    let dim = Style::default().add_modifier(Modifier::DIM);
    let active = Style::default().add_modifier(Modifier::BOLD);
    let line = Line::from(vec![
        Span::styled(" q quit", dim),
        Span::raw(" · "),
        Span::styled("j/k scroll", dim),
        Span::raw(" · "),
        Span::styled(
            "v value",
            if ui.sort == SortMode::Value {
                active
            } else {
                dim
            },
        ),
        Span::raw(" · "),
        Span::styled(
            "d distance",
            if ui.sort == SortMode::Distance {
                active
            } else {
                dim
            },
        ),
        Span::raw(" · "),
        Span::styled("t threshold", dim),
        Span::raw(" · "),
        Span::styled("r route", dim),
        Span::raw(" · "),
        Span::styled(
            "c copy",
            if ui.route.as_ref().is_some_and(|route| !route.complete()) {
                active
            } else {
                dim
            },
        ),
        Span::raw(format!("   journal: {}", ui.journal_dir)),
    ]);
    frame.render_widget(Paragraph::new(line), area);
}

fn draw_popup(frame: &mut Frame, area: Rect, input: &Input) {
    // Route paths can be long; give that mode a wider box.
    let width = match input.mode {
        InputMode::Threshold => 40u16,
        InputMode::RoutePath => 64u16,
    }
    .min(area.width.saturating_sub(2));
    let height = 5u16.min(area.height.saturating_sub(2));
    let popup = Rect::new(
        area.x + (area.width.saturating_sub(width)) / 2,
        area.y + (area.height.saturating_sub(height)) / 2,
        width,
        height,
    );

    let hint = match &input.error {
        Some(error) => Line::from(Span::styled(error.clone(), Style::default().fg(Color::Red))),
        None => Line::from(Span::styled(
            "Enter to apply · Esc to cancel",
            Style::default().add_modifier(Modifier::DIM),
        )),
    };

    let title = match input.mode {
        InputMode::Threshold => " Species value threshold ",
        InputMode::RoutePath => " Load route file ",
    };
    let content = Paragraph::new(vec![
        Line::from(Span::styled(
            format!("> {}", input.buffer),
            Style::default().add_modifier(Modifier::BOLD),
        )),
        hint,
    ])
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title(title),
    )
    .alignment(Alignment::Center);

    frame.render_widget(content, popup);
}

/// Groups a number with thousands separators (Rust's format strings have no `,` flag).
fn group(value: u64) -> String {
    let digits = value.to_string();
    let total = digits.len();
    let mut result = String::with_capacity(total + total / 3);
    for (i, digit) in digits.chars().enumerate() {
        result.push(digit);
        let remaining = total - i - 1;
        if remaining > 0 && remaining.is_multiple_of(3) {
            result.push(',');
        }
    }
    result
}
