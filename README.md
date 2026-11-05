# sim-engine

A deterministic RTS in Rust, inspired by *Supreme Commander: Forged Alliance*. This is the vertical slice: one 10 km skirmish against a scripted AI, on an engine tested to 5,000 units and 2,000 physical shells at 10 ticks per second on an 81 km map.

- [`SPECIFICATION.md`](SPECIFICATION.md): the engine design and its acceptance gates
- [`GAME_SPECIFICATION.md`](GAME_SPECIFICATION.md): the game design, with the slice's scope in §18
- [`data/units.toml`](data/units.toml): unit and structure stats, which are starting values for tuning

## Play

```bash
cargo run --release              # A skirmish against the AI
cargo run --release -- --battle  # The 5,000-unit stress test
```

Destroy the enemy commander to win. Yours explodes if it dies, and your whole team goes with it.

1. Press Space to select your commander. Choose a structure from the bottom panel, then left-click the ground to place it.
2. Mass extractors go on the teal deposits, and snap to the nearest free one near the cursor. Power generators supply energy. Land factories build units.
3. Click a factory to queue units. Right-click the ground with a factory selected to set its rally point. With several factories selected, every order goes to all of them that can follow it. A factory still under construction takes orders too, and starts on them once it is built. Once an upgrade is queued, the factory can also queue what the upgrade unlocks, which waits for it to finish.
4. If the top bar says STALL, you are spending faster than you earn, and everything slows down.

| Control | Action |
| :--- | :--- |
| Left-drag or click | Select your units |
| Double-click | Select all your units of that type on screen |
| Shift + click or drag | Add to the selection, and click again to remove a unit |
| Hold Shift | Show every unit's pending orders, which fade after you let go |
| Right-click | Attack an enemy, assist or repair a friend, reclaim a wreck, move, or set a rally point |
| M, then left-click | Move there, whatever is under the cursor |
| F, then left-click | Attack-move: stop to fire when enemies are in range |
| Y, then left-click a friendly unit | Assist: stay with it and protect it, and builders also help it build |
| P, then left-click each point, then right-click or P | Patrol |
| Alt + right-click | Attack-move, as above |
| Hold P + right-click | Add patrol points, then release P |
| Shift + left-click | Queue a structure, and keep placing |
| Shift + left-drag | Queue a row of structures, and keep placing |
| X | Stop: drop all orders |
| Q, E, G, T, C | With a builder selected: Land Factory, Mass Extractor, Power Generator, Mass Storage, Point Defense |
| Z, B, V, H | With a factory selected: queue an Engineer, Light Bot, Light Artillery, Heavy Tank |
| U, R, K | With a factory selected: Upgrade, Repeat, Clear queue |
| Space | Jump to your commander and select it |
| Scroll | Zoom, towards the cursor |
| WASD or arrow keys | Pan |
| Esc | Cancel placement or attack-move, or clear the selection |

Selected units show a dashed trail through their pending orders, in the order they will carry them out, and selected factories show a dimmer line to their rally point. Queued builds always show a dimmed footprint, and a faint grid appears while placing, so that the next one can be lined up against them. Builders turn to face what they are working on, and a beam with moving pulses joins them to it for as long as work is really happening: it stops if they are out of resources. Each option in the command panel shows its key, including the unit orders above, which are also buttons, and the panel lists what is selected has queued: right-click an item to remove it. A site that was already started stays, and can be finished by assisting it. A marker and a line of text confirm each order. A click that shows nothing never reached the game. The window title shows the frame rate and tick time. Run with `--help` for the headless and test modes.

## Validate

See [`SPECIFICATION.md` §7](SPECIFICATION.md#7-how-to-validate-the-build) for the full list.

```bash
cargo test --release
cargo test                                # Unoptimized, so that arithmetic overflow fails loudly
cargo clippy --all-targets -- -D warnings # Enforces the determinism rules
cargo test --release --target x86_64-apple-darwin --test determinism_test
cargo bench
cargo run --release -- --soak 100         # Plays and replays 100 AI-vs-AI matches
cargo run --release -- --autotest 60 --fast-forward 10000   # Opens a window for 60 seconds
```

The stored checksums in `tests/golden/` change whenever simulation behavior changes on purpose. Update them with `UPDATE_GOLDEN=1 cargo test --release --test determinism_test`, then confirm that the x86-64 run still matches.
