use std::collections::HashMap;

use ed_journals::logs::LogEvent;
use ed_state::state::{EventSink, SinkResult};
use ed_state::system::{PlanetSpeciesEntry, SystemState};

/// All systems visited this session, plus the one the player is currently in.
#[derive(Default)]
pub struct SystemsState {
    pub systems: HashMap<u64, SystemState>,
    pub current: Option<u64>,
}

impl EventSink for SystemsState {
    fn sink_log(&mut self, log_event: &LogEvent) -> SinkResult {
        // Location events (Location, FSDJump, CarrierJump) tell us which system
        // we are in; switch to it, creating it on first sight.
        if let Some(location_info) = log_event.content.location_info() {
            let address = location_info.system_address;
            self.current = Some(address);
            self.systems
                .entry(address)
                .or_insert_with(|| SystemState::from(address));
        }

        let Some(current) = self.current else {
            return SinkResult::Ignored;
        };

        // Each SystemState ignores events that don't belong to it.
        self.systems
            .get_mut(&current)
            .map(|system| system.sink_log(log_event))
            .unwrap_or(SinkResult::Ignored)
    }
}

/// A species entry still worth acting on: predicted to spawn (or already
/// confirmed) and above the player's value threshold.
pub fn is_active(entry: &PlanetSpeciesEntry, threshold: u64) -> bool {
    (entry.will_spawn.yes() || entry.will_spawn.maybe())
        && entry.species.base_value() >= threshold
}

/// A planet is done when it has species to find and every active one is
/// complete (3/3 scans) — or none of them are active.
pub fn is_complete(entries: &[PlanetSpeciesEntry], threshold: u64) -> bool {
    !entries.is_empty()
        && entries
            .iter()
            .all(|entry| !is_active(entry, threshold) || entry.completed)
}
