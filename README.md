# exobio-rat

A cross platform TUI for Elite Dangerous exobiology. Lists predicted flora for scanned planets to narrow results down, and offers a simple routing feature to help with stratum sniping.

## Build and run

```sh
cargo build --release
./target/release/exobio-rat            # auto-detects the journal directory
./target/release/exobio-rat --dir PATH # or point at it explicitly
```

## Keys

| Key          | Action                              |
| ------------ | ----------------------------------- |
| `q` / `Esc`  | quit                                |
| `j` / `k`    | scroll the planet list              |
| `v` / `d`    | sort by estimated value / distance  |
| `t`          | set the minimum species value       |
| `r`          | load a route file (path in a popup) |
| `c`          | copy the next route destination     |

## Routes

Generate a route at <https://www.spansh.co.uk/> and download it as JSON. Press `r` in the TUI and enter the file path. A panel at the bottom then tracks your progress along the route:

- visited systems are ticked off — arriving at any system on the route completes everything up to and including it
- the next destination is shown, and its system name is copied to the clipboard automatically whenever you enter a route system, so you can paste it into the in-game system map to locate it
