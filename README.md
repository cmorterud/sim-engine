# sim-engine

A deterministic RTS simulation engine proof of concept in Rust: 5,000 units and 2,000 physical shells at 10 ticks per second on an 81 km map, with a continuous zoom from ground level to the whole map.

- [`SPECIFICATION.md`](SPECIFICATION.md): the engine design and its acceptance gates
- [`GAME_SPECIFICATION.md`](GAME_SPECIFICATION.md): the game this engine is meant to support

## Run

```bash
cargo run --release
```

| Control | Action |
| :--- | :--- |
| Left-drag or click | Select your units (Blue) |
| Right-click | Move |
| Alt + right-click | Attack-move: stop to fire when enemies are in range |
| Scroll | Zoom, towards the cursor |
| WASD or arrow keys | Pan |
| Esc | Clear the selection |

The window title shows the frame rate, tick time, and army sizes. Run with `--help` for the headless and scripted test modes.

## Validate

See [`SPECIFICATION.md` §7](SPECIFICATION.md#7-how-to-validate-the-build) for the full list.

```bash
cargo test --release
cargo test                                # Unoptimized, so that arithmetic overflow fails loudly
cargo clippy --all-targets -- -D warnings # Enforces the determinism rules
cargo test --release --target x86_64-apple-darwin --test determinism_test
cargo bench
cargo run --release -- --autotest 60      # Opens a window for 60 seconds
```

The stored checksums in `tests/golden/` change whenever simulation behavior changes on purpose. Update them with `UPDATE_GOLDEN=1 cargo test --release --test determinism_test`, then confirm that the x86-64 run still matches.
