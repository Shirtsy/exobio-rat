use std::path::Path;

use serde::Deserialize;

/// One stop on a loaded route: the system name (for display and the
/// clipboard) and the address (for matching journal location events).
pub struct Stop {
    pub name: String,
    pub address: u64,
    pub jumps: u32,
}

/// A loaded route: ordered stops plus how many of them are done. The order
/// is strict — entering the stop at index i ticks off everything up to and
/// including i, so no state needs to persist between sessions.
pub struct Route {
    stops: Vec<Stop>,
    visited: usize,
}

impl Route {
    /// Load a route from an EDSearch job result file.
    pub fn from_file(path: &Path) -> Result<Route, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
        let file: RouteFile =
            serde_json::from_str(&text).map_err(|error| format!("not a route file: {error}"))?;
        let stops = file
            .result
            .system_jumps
            .into_iter()
            .map(|stop| {
                let address = stop
                    .id64
                    .parse::<u64>()
                    .map_err(|_| format!("bad system id64 '{}' in route", stop.id64))?;
                Ok(Stop {
                    name: stop.system,
                    address,
                    jumps: stop.jumps,
                })
            })
            .collect::<Result<Vec<Stop>, String>>()?;
        if stops.is_empty() {
            return Err("route file contains no systems".into());
        }
        Ok(Route { stops, visited: 0 })
    }

    /// Tick off stops up to and including the first one matching `address`
    /// at or after the current position. Returns true if anything new was
    /// ticked off.
    pub fn advance(&mut self, address: u64) -> bool {
        let remaining = &self.stops[self.visited..];
        match remaining.iter().position(|stop| stop.address == address) {
            Some(index) => {
                self.visited += index + 1;
                true
            }
            None => false,
        }
    }

    /// The next destination, or None when the route is complete.
    pub fn next(&self) -> Option<&Stop> {
        self.stops.get(self.visited)
    }

    /// The stop just ticked off, if any.
    pub fn last(&self) -> Option<&Stop> {
        if self.visited == 0 {
            None
        } else {
            self.stops.get(self.visited - 1)
        }
    }

    pub fn visited(&self) -> usize {
        self.visited
    }

    pub fn complete(&self) -> bool {
        self.next().is_none()
    }

    pub fn len(&self) -> usize {
        self.stops.len()
    }
}

// The EDSearch job result file shape; everything else is ignored.
#[derive(Deserialize)]
struct RouteFile {
    result: RouteResult,
}

#[derive(Deserialize)]
struct RouteResult {
    system_jumps: Vec<RawStop>,
}

#[derive(Deserialize)]
struct RawStop {
    system: String,
    id64: String,
    #[serde(default)]
    jumps: u32,
}

