use crossterm::event::{KeyCode, KeyModifiers};
use ed_journals::exobiology::Species;
use ed_state::system::{PlanetState, PlanetSpeciesEntry, SystemState};
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListState, Paragraph};
use ratatui::Frame;

use crate::state::{is_active, is_complete, SystemsState};

const INDENT: &str = "     ";

#[derive(Default)]
pub struct Ui {
    pub journal_dir: String,
    pub scroll: usize,
    pub sort: SortMode,
    pub threshold: u64,
    pub input: Option<Input>,
    pub last_system: Option<u64>,
}

pub struct Input {
    pub buffer: String,
    pub error: Option<String>,
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
}

impl Ui {
    pub fn handle_key(&mut self, code: KeyCode, modifiers: KeyModifiers) -> KeyAction {
        // While the threshold popup is open it swallows all keys.
        if self.input.is_some() {
            match code {
                KeyCode::Esc => self.input = None,
                KeyCode::Enter => {
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
            KeyCode::Up | KeyCode::Char('k') => self.scroll = self.scroll.saturating_sub(3),
            KeyCode::Down | KeyCode::Char('j') => self.scroll += 3,
            KeyCode::Char('v') => self.sort = SortMode::Value,
            KeyCode::Char('d') => self.sort = SortMode::Distance,
            KeyCode::Char('t') => self.input = Some(Input { buffer: String::new(), error: None }),
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

fn build_views<'a>(system: &'a SystemState, ui: &Ui) -> Vec<View<'a>> {
    system
        .planet_state
        .iter()
        // Stars and belt clusters get a PlanetState entry too; they are not exobio targets.
        .filter(|(id, _)| {
            !system.star_scans.contains_key(id) && !system.belt_scans.contains_key(id)
        })
        .map(|(id, planet)| {
            let entries = species_entries(system, planet);
            let name = planet
                .scan
                .as_ref()
                .map(|scan| scan.body_name.clone())
                .or_else(|| planet.saa_scan.as_ref().map(|scan| scan.body_name.clone()))
                .unwrap_or_else(|| format!("Body {id}"));
            View {
                id: *id,
                planet,
                name,
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
    let chunks = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .split(area);

    let (system_address, system) = match systems.current {
        Some(address) => (Some(address), systems.systems.get(&address)),
        None => (None, None),
    };

    // Start at the top of the list when entering a new system.
    if ui.last_system != system_address {
        ui.scroll = 0;
        ui.last_system = system_address;
    }

    let views = system.map(|system| build_views(system, ui)).unwrap_or_default();

    draw_header(frame, chunks[0], system, &views, ui);
    draw_list(frame, chunks[1], system, &views, ui);
    draw_footer(frame, chunks[2], ui);

    if let Some(input) = &ui.input {
        draw_popup(frame, area, input);
    }
}

fn draw_header(
    frame: &mut Frame,
    area: Rect,
    system: Option<&SystemState>,
    views: &[View<'_>],
    ui: &Ui,
) {
    let name = system
        .and_then(|system| system.location_info.as_ref())
        .map(|info| info.star_system.as_str())
        .unwrap_or("waiting for system…");

    let bodies = match system {
        Some(system) => match system.number_of_bodies {
            Some(total) => format!("{}/{} bodies", system.nr_of_scanned_bodies(), total),
            None => format!("{} bodies scanned", system.nr_of_scanned_bodies()),
        },
        None => String::new(),
    };

    let near = system
        .and_then(|system| system.near_body)
        .and_then(|body| views.iter().find(|view| view.id == body))
        .map(|view| view.name.as_str())
        .unwrap_or("—");

    let line = Line::from(vec![
        Span::styled(format!(" {name}"), Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(format!("  {bodies}")),
        Span::raw(format!("  sort: {}", ui.sort.label())),
        Span::styled(
            format!("  threshold: {}", group(ui.threshold)),
            Style::default().fg(Color::Cyan),
        ),
        Span::styled(format!("  near: {near}"), Style::default().fg(Color::Yellow)),
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
        va.complete
            .cmp(&vb.complete)
            .then_with(|| match ui.sort {
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
    let mut rows: Vec<Line<'static>> = Vec::new();
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
                " No bodies discovered yet — run an FSS scan.",
                dim,
            ))),
            area,
        );
        return;
    }

    // Auto-scroll to keep the near planet visible; manual scroll clamps it.
    let visible = area.height as usize;
    if let Some(row) = near
        .and_then(|body| header_rows.iter().find(|(id, _)| *id == body).map(|(_, row)| *row))
    {
        if row < ui.scroll {
            ui.scroll = row;
        } else if row + 1 > ui.scroll + visible {
            ui.scroll = row.saturating_sub(visible.saturating_sub(1));
        }
    }
    ui.scroll = ui.scroll.min(rows.len().saturating_sub(visible));

    let mut list_state = ListState::default();
    *list_state.offset_mut() = ui.scroll;
    frame.render_stateful_widget(List::new(rows), area, &mut list_state);
}

fn planet_header_row(view: &View<'_>, near: bool) -> Line<'static> {
    let base = Style::default();
    let style = if view.complete {
        base.add_modifier(Modifier::DIM)
    } else if near {
        base.add_modifier(Modifier::BOLD).fg(Color::Yellow)
    } else {
        base
    };

    let land = match &view.planet.exobiology_body {
        Some(body) if body.landable => "landable",
        Some(_) => "no-land",
        None => "—",
    };
    let scan = if view.planet.scan.is_some() {
        "full"
    } else if view.planet.saa_scan.is_some() {
        "saa"
    } else {
        "fss"
    };
    let bio_span = match view.planet.signal_counts.as_ref().map(|s| s.biological_signal_count) {
        Some(count) if count > 0 => Span::styled(
            format!("  bio:{count}"),
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ),
        Some(_) => Span::styled(
            "  bio:0",
            Style::default().add_modifier(Modifier::DIM),
        ),
        None => Span::styled(
            "  bio:—",
            Style::default().add_modifier(Modifier::DIM),
        ),
    };

    let mut spans = vec![
        Span::styled(
            if near { " @" } else { "   " },
            if near {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::DarkGray)
            },
        ),
        Span::styled(format!(" {}  ", view.id), style),
        Span::styled(truncate(&view.name, 20), style),
        Span::styled(format!(" {land}"), style),
        Span::styled(format!(" [{scan}]"), style),
        bio_span,
        Span::styled(format!("  est {:>12}", group(view.estimated)), style),
    ];
    if view.complete {
        spans.push(Span::styled(
            "  [EXOBIO ✓]",
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ));
    }
    Line::from(spans)
}

fn planet_inset_rows(view: &View<'_>, ui: &Ui) -> Vec<Line<'static>> {
    let mut rows: Vec<Line<'static>> = Vec::new();
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

        let mut spans: Vec<Span<'static>> = vec![Span::raw(INDENT), Span::styled("signals:", dim)];
        let mut any = false;
        for (count, label, color) in kinds {
            any = true;
            spans.push(Span::raw(" "));
            spans.push(Span::styled(format!("{label} {count}"), Style::default().fg(color)));
        }
        if !any {
            spans.push(Span::styled(" none", dim));
        }
        rows.push(Line::from(spans));
    }

    // Species checklist: active incomplete first, then completed, then inactive.
    let mut items: Vec<(u8, Line<'static>, u64)> = Vec::new();
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
            (
                1,
                "✓✓✓".to_string(),
                Style::default().fg(Color::Green),
            )
        } else if !active {
            (2, "·  ".to_string(), dim)
        } else if entry.confirmed {
            let progress = organic.map(|organic| organic.progress_nr()).unwrap_or(1);
            (
                0,
                format!("✓ {progress}/3"),
                Style::default().fg(Color::Green),
            )
        } else {
            (0, "?   ".to_string(), Style::default().fg(Color::Yellow))
        };

        let text_style = if bucket == 0 {
            Style::default()
        } else {
            dim
        };
        items.push((
            bucket,
            Line::from(vec![
                Span::raw(INDENT),
                Span::styled(marker, marker_style),
                Span::styled(label, text_style),
                Span::styled(format!("  {:>12}", group(value)), dim),
            ]),
            value,
        ));
    }

    // Scanned organics the prediction did not list.
    let predicted: Vec<Species> = view.entries.iter().map(|entry| entry.species.clone()).collect();
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
            Line::from(vec![
                Span::raw(INDENT),
                Span::styled(marker, Style::default().fg(Color::Green)),
                Span::styled(
                    format!("{} (not predicted)", organic.species),
                    if bucket == 0 {
                        Style::default()
                    } else {
                        dim
                    },
                ),
                Span::styled(format!("  {:>12}", group(organic.species.base_value())), dim),
            ]),
            organic.species.base_value(),
        ));
    }

    items.sort_by(|(a, _, va), (b, _, vb)| a.cmp(b).then(vb.cmp(va)));
    rows.extend(items.into_iter().map(|(_, line, _)| line));

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
            "  no biological signals"
        };
        rows.push(Line::from(Span::styled(hint, dim)));
    } else if excluded > 0 {
        rows.push(Line::from(Span::styled(
            format!("{INDENT}— {excluded} species excluded (impossible)"),
            dim,
        )));
    }

    rows
}

fn draw_footer(frame: &mut Frame, area: Rect, ui: &Ui) {
    let dim = Style::default().add_modifier(Modifier::DIM);
    let active = Style::default().add_modifier(Modifier::BOLD);
    let line = Line::from(vec![
        Span::styled(" q quit", dim),
        Span::raw(" · "),
        Span::styled("↑/↓ scroll", dim),
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
        Span::raw(format!("   journal: {}", ui.journal_dir)),
    ]);
    frame.render_widget(Paragraph::new(line), area);
}

fn draw_popup(frame: &mut Frame, area: Rect, input: &Input) {
    let width = 40u16.min(area.width.saturating_sub(2));
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
            .title(" Species value threshold "),
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
        if remaining > 0 && remaining % 3 == 0 {
            result.push(',');
        }
    }
    result
}

fn truncate(value: &str, width: usize) -> String {
    let mut chars = value.chars();
    let mut result: String = chars.by_ref().take(width).collect();
    if chars.next().is_some() {
        result.pop();
        result.push('…');
    }
    result
}
