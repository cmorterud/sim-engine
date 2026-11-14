# sim-engine

A deterministic RTS in Rust, inspired by *Supreme Commander: Forged Alliance*. This is the vertical slice: one 10 km skirmish against a scripted AI, on an engine tested to 5,000 units and 2,000 physical shells at 10 ticks per second on an 81 km map.

- [`SPECIFICATION.md`](SPECIFICATION.md): the engine design and its acceptance gates
- [`GAME_SPECIFICATION.md`](GAME_SPECIFICATION.md): the game design, with the slice's scope in §18
- [`data/units.toml`](data/units.toml): unit and structure stats, which are starting values for tuning

## Play

```bash
cargo run --release              # A skirmish against the AI, which plays easy (--ai normal for the full AI)
cargo run --release -- --battle  # The 5,000-unit stress test
```

Or play in the browser at **https://codymorterud.com/sim-engine/**, in any browser with WebGL2. The page asks for a map, the AI's difficulty, and optionally a seed, with a picture of each map (drawn by `examples/map_picture.rs` into `assets/web/maps`). Its choices go in the address, so a game can be linked to directly: `?map=highland`, `?map=twin_passes`, or `?map=skirmish` for the maps, `?seed=7` for another seed, `?ai=normal` for a harder opponent, or `?battle` for the stress test. Without any of these, the page shows the choices and the game waits. It is rebuilt and published once a push to `main` passes CI and the soak (`.github/workflows/pages.yml`), and CI checks that the browser build compiles. The browser build is the same game, but it has no sound yet, cannot load maps from your files, and is slower, so huge battles may stutter. To build it yourself:

```bash
rustup target add wasm32-unknown-unknown
cargo install trunk --locked
trunk serve --release    # then open http://127.0.0.1:8080
```

Destroy the enemy commander to win. Yours explodes if it dies, and your whole team goes with it.

1. Press Space to select your commander. Choose a structure from the bottom panel, then left-click the ground to place it.
2. Mass extractors go on the teal deposits, and snap to the nearest free one near the cursor. Power generators supply energy. Land factories build units.
3. Click a factory to queue units. Every build menu is one bar: pick T1, T2, or T3 at its left to see that level's items, and hover over an item for its cost and what it does. Factories build in the open: you can watch each unit take shape in the bay, then drive out of the end facing the rally point (aircraft lift off their pad). Right-click the ground with a factory selected to set its rally point. With several factories selected, every order goes to all of them that can follow it. A factory still under construction takes orders too, and starts on them once it is built. Once an upgrade is queued, the factory can also queue what the upgrade unlocks, which waits for it to finish.
4. If the top bar says STALL, you are spending faster than you earn, and everything slows down.

| Control | Action |
| :--- | :--- |
| Left-drag or click | Select your units |
| Double-click | Select all your units of that type on screen |
| Shift + click or drag | Add to the selection, and click again to remove a unit |
| Hold Shift | Show every unit's pending orders, which fade after you let go |
| Right-click | Attack an enemy, assist or repair a friend, reclaim a wreck, tree, or rock, move, or set a rally point |
| M, then left-click | Move there, whatever is under the cursor |
| F, then left-click | Attack-move: stop to fire when enemies are in range |
| Y, then left-click a friendly unit | Assist: stay with it and protect it, and builders also help it build |
| P, then left-click each point, then right-click or P | Patrol |
| Alt + right-click | Attack-move, as above |
| Ctrl (or Cmd) + right-click the ground | Formation move: the group travels at its slowest unit's pace, keeping its shape all the way. Add Alt to attack-move, which stops the whole group to fight |
| J, then left-click | Formation move, as above. Hold Alt when clicking to attack-move |
| I, then left-click | Attack ground: armed units keep firing at a spot, even out of sight, until given another order. Click debris to aim at its middle. Shells that land on debris destroy it, so the enemy cannot reclaim it |
| Hold P + right-click | Add patrol points, then release P |
| Hold Shift, drag an order's icon | Move a move, build, or rally point somewhere else. Right-click or Esc cancels |
| Shift + right-click | Queue the order after what the selection is already doing, including moves for groups |
| Shift + left-click | Queue a structure, and keep placing. With M, F, J, or Y armed, queue that order and stay armed |
| Shift + left-drag | Queue a row of structures, and keep placing |
| X | Stop: drop all orders |
| L, then left-click an enemy | Overcharge with the commander: 3,000 stored energy for a shot that destroys almost any unit, but does little to structures |
| O | Pause or resume the selected factories and construction sites. They keep their queue and progress |
| Ctrl + 1 to 0 | Save the selection as a group, or clear the group if nothing is selected. Command works too |
| 1 to 0, or click it in the bar on the left | Select that group, and press the key again quickly to jump the camera to it. Hold Shift on the bar to add it to the selection |
| N | Mute or unmute, when nothing selected builds anything on N |
| - and = | With a builder selected, Sonar and Torpedo Launcher (on water). Sound quieter and louder otherwise |
| ` (above Tab) | Show the next tech level in the build bars. The T1, T2, and T3 toggles at the start of a bar do the same |
| Q E G T C Z V B H N , . / ; ' [ ] \\ | Build what is in that slot of the build bar, at the level shown. Each kind of thing keeps its key at every level. With a builder: Q Land Factory (T2 and T3: support factories), E Mass Extractor (all tiers), G Power Generator (all tiers), T Mass Storage, `[` Energy Storage, `]` Artillery Installation (T2) and Heavy Artillery Installation (T3), `\` Naval Factory, C Point Defense (T1, T2), Z Radar (T1, T2, T3 Omni Sensor), V anti-air (AA Turret, Flak Cannon, SAM Launcher), B Air Factory (T2 and T3: support factories), H Hydrocarbon Power Plant at T1 and the Mass Fabricators at T2 and T3, N Shield Generator, `,` missile launchers, `.` missile defenses, `/` Stealth Field Generator, `;` Air Staging Facility, `'` Quantum Gateway. With a factory, which takes the keys when builders are selected too: Q engineers, scouts, the support commander, and missiles, E bots, tanks, and fighters, G artillery (Mobile Heavy Artillery at T3) and bombers, T anti-air and gunships. Hold Shift to queue 5 |
| O, with a cloaking commander selected | Switch its cloak on or off, when nothing selected can be paused |
| U | Upgrade a factory or a mass extractor to its next tier, or a radar to an omni sensor |
| Ctrl (or Cmd) + T, then left-click | Teleport a commander that has a teleporter, to open ground you can see or have on radar |
| Ctrl (or Cmd) + C, then left-click | Capture an enemy unit or structure with the selected builders, in half its build time for its energy cost. Right-clicking an enemy with engineers alone does it too. Commanders cannot be captured |
| Ctrl (or Cmd) + K | Self-destruct the selected units in 5 seconds, leaving no debris. Press again to stop |
| U, R, K | With a factory selected: Upgrade, Repeat, Clear queue |
| Space | Jump to your commander and select it |
| Ctrl (or Cmd) + P, or the Pause button at the top right | Pause or resume the game. Orders given while paused are carried out when it resumes |
| Scroll | Zoom, towards the cursor |
| WASD or arrow keys | Pan |
| Esc | Cancel placement or attack-move, or clear the selection |

Selected units show a dashed trail through their pending orders, in the order they will carry them out, and selected factories show a dimmer line to their rally point. Queued builds always show a dimmed footprint, and a faint grid appears while placing, so that the next one can be lined up against them. Units that destroy enemies earn their mass as experience, and rise through five veteran ranks, shown as gold pips: each adds health, heals at once, and adds regeneration. Weapons swivel at their own rate and only fire once they point at their target, so a tank's turret has to come round first. Units only move forward along the way they face. Vehicles turn like cars, in arcs no tighter than their turn radius, while walkers such as the commander and bots turn on the spot. The commander's legs and the engineer's hull face the way they move, while the body or arm above turns to face what they are working on, and a beam with moving pulses joins them to it for as long as work is really happening: it stops if they are out of resources. Each option in the command panel shows its key, including the unit orders above, which are also buttons, and the panel lists what is selected has queued: right-click an item to remove it. A site that was already started stays, and can be finished by assisting it. The bar on the right selects your commander or factories without moving the view: click a tier for the factory closest to your view, double-click for all of them, and hold Shift to add to the selection. Building and producing make a sound when you are zoomed in on them, selecting units gives a short tick, and an alarm and an alert show when your commander takes damage, at most once every 30 seconds. Run with `--sound-check` to hear each sound. The panel shows what a selected structure makes and what a factory is really spending. Trees give quick energy and a little mass when reclaimed, and rocks give mass. Building over them clears them, and you get what they held. Debris can be shot to pieces: every shell that lands on it, and every blast that reaches it, wears it down, and it loses its mass as it goes, so a graveyard you cannot hold can be denied to the enemy. Trees and rocks are not harmed. A Shield Generator raises a see-through dome that stops enemy shells from outside while your units under it fire out. It heals over time, collapses for 30 seconds when worn out, costs energy while up (less beside power generators), and falls if your energy runs out. Splash that strikes overlapping domes hits them all. Pause it with O to save energy. Missile launchers build their own missiles with Q and fire them only when ordered: right-click an enemy, or attack the ground. A Tactical Missile Launcher keeps up to 6 cruise missiles and fires them over walls and hills at its target until it is gone, and a Tactical Missile Defense shoots them down one at a time. A Strategic Missile Launcher keeps up to 5 nuclear missiles and fires one for each order, anywhere on the map: its inner ring destroys anything, and its outer ring only light units and buildings. Shields do not stop them, but a Strategic Missile Defense with an interceptor built shoots down any that flies within its range. A Radar shows enemies out of sight as blips, and upgrades with U to a T2 Radar and then an Omni Sensor, which sees through stealth, cloaks, and decoys. A Stealth Field Generator hides your units near it from enemy radar and shows the enemy decoy blips. Your commander has a little omni of its own. Its enhancements, under the Enhancements heading in the command panel, fit in three slots: a heavier cannon in the right arm; the T2 and T3 engineering suites, which it needs to build higher-tier structures, or a repair system in the left; and a shield, stealth and then a cloak, a teleporter, or a resource generator on the back. It stands still while one is built, and a new order cancels it. Once the T2 or T3 engineering suite is started, that tier's structures can already be ordered: they wait behind the suite, and are dropped if it is cancelled. Anything upgrading, yours or an enemy's in sight, has a pulsing purple ring, and yours also show a progress bar. A Quantum Gateway builds support commanders: heavily armored engineers with a strong gun and income of their own, whose deaths do not end the match, with enhancements of their own. Zoomed out, lines show the outer edge of what your radar and omni cover. To fire at a blip, attack the ground there. An order that cannot be reached, because walls or buildings cut it off, is cancelled with a message, and units move on to whatever else they were told to do. A marker and a line of text confirm each order. A click that shows nothing never reached the game. The window title shows the frame rate and tick time. Run with `--help` for the headless and test modes. An Air Factory builds aircraft, which fly over walls, water, and cliffs and land when they have nothing to do, where there is open ground, or circle where there is none. Landed, they can be hit by ground units but not by anti-air: the Air Scout sees and has radar far around it, the Interceptor shoots down aircraft and nothing else, and the Attack Bomber makes passes over ground targets, dropping a bomb on each, and keeps to the target it picks until it is destroyed, even on an attack-move. Every unit on an attack-move or a patrol does the same: once it picks a target, it chases it until it is destroyed, then goes on with its move. The Air Factory upgrades to T2 for the Gunship, which hovers over what it attacks, and the Fighter-Bomber, which carpets the ground with napalm and fights aircraft, and to T3 for the Spy Plane, Air Superiority Fighter, Strategic Bomber (one small nuclear bomb at a time, and guns against aircraft on its tail), and Heavy Gunship. Flak (and Mobile Flak) bursts among aircraft and hits every one nearby, and the T3 SAM Launcher brings down heavy aircraft. Artillery installations (T2 and T3) and Mobile Heavy Artillery shell targets from long range, but cannot hit anything too close, and their shells scatter a little. An installation's shots cost energy, and power generators beside it make them cheaper and faster. A Naval Factory, built on deep water near the shore, upgrades to T2 and T3 for Destroyers, Cruisers, which fire cruise missiles on their own and shoot down missiles, Battleships, and Battlecruisers, and builds Frigates, which sail only where the water is deep enough and shell the shore and aircraft alike. The Shield Boat carries a dome over your fleet, and destroyers and battlecruisers shoot down torpedoes. Right-click a missile's build button to build them one after another, and again to stop. The Strategic Missile Submarine builds nuclear missiles on order and fires cruise missiles on its own, and the Mobile Missile Launcher fires cruise missiles on its own at anything in range. Your commander walks along the seabed, hidden and unable to fire while under the water, and the Amphibious Tank hovers over land and water alike, as engineers do, so they can build out on the water. Attack Submarines sail submerged, where only torpedoes reach them and only Sonar finds them; Torpedo Launchers and Torpedo Bombers hunt them and ships. Aircraft burn fuel in the air, and without it fly low and slow. An Air Staging Facility (T2) refuels and repairs four aircraft at a time: right-click it with aircraft selected. Aircraft on patrol stay in the air, and go to refuel on their own when low. Only anti-air can hit aircraft in flight: interceptors, the Anti-Air Turret, and Mobile Anti-Air from the land factory. Two maps come with the game: `--map highland`, a 10 km plateau of hills, outcrops and rock fields, and `--map twin_passes`, 20 km with a sea down the middle crossed by two land bridges. To play on a map made for *Forged Alliance* that you already have, unzip it and run with `--map <folder>`; `--starts 1,2` picks which two of its starts are used. The map is read from your files, and none are included with the game. `cargo run --release --bin map-download` offers two maps from Forged Alliance Forever's vault, Hilly Plateau and Dual Gap Adaptive, and downloads the ones you pick into your system's cache folder (set `SIM_ENGINE_MAP_CACHE` to choose another), keeping any already there.

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

Every push to `main` also runs the soak on GitHub's runners (`.github/workflows/soak.yml`): the 200 skirmish matches and 120 on each bundled map, in shards of 25 seeds, one runner each, all from one release build. The run's summary page adds up wins by side, unfinished matches, and replay mismatches per map, with the chance of a split at least that lopsided between equally strong sides (a p below 0.05 suggests a real lean). Tick times are reported there but do not fail the run (`--no-tick-gate`), since a shared runner's other load would fail it falsely; the benchmarks check speed locally. Start a soak by hand from the Actions tab, or with `gh workflow run soak.yml`; its inputs set the matches per map and per runner, for example `gh workflow run soak.yml -f bundled_matches=500` for a closer look at a map's fairness.

Any soak match can be watched: the simulation and the AIs are deterministic, so the map and seed alone replay it. `cargo run --release -- --watch --map highland --seed 223` shows everything, with both teams' economies; `--view blue` or `--view red` starts in one side's sight instead. In the match, V steps between everything, Blue, and Red, - and = change the speed up to 16x, and Space pauses. In the browser, use the page's Watch section, or `?watch&map=highland&seed=223&view=all`.

The stored checksums in `tests/golden/` change whenever simulation behavior changes on purpose. Update them with `UPDATE_GOLDEN=1 cargo test --release --test determinism_test`, then confirm that the x86-64 run still matches.

## License and credits

sim-engine is free software under the [GNU General Public License, version 3](LICENSE).

It is inspired by *Supreme Commander: Forged Alliance* and is not affiliated with or endorsed by its makers or by Forged Alliance Forever. It contains no code, art, sound, or maps from either. Some unit and enhancement names follow the original's for now, as placeholders until the game has its own names and look (GAME_SPECIFICATION.md §20). [`PROVENANCE.md`](PROVENANCE.md) records everything taken from elsewhere, such as starting balance numbers. The map downloader fetches maps from Forged Alliance Forever's public map vault for your own use; no maps are included in or hosted by this repository.
