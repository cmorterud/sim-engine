# Technical Specification: Deterministic High-Scale RTS Simulation Engine (PoC)

## 1. Project Overview & Objective

### 1.1 Objective
Build a minimal, high-performance, deterministic Proof of Concept (PoC) simulation engine in **Rust** capable of simulating **5,000 autonomous units** and **2,000 physical ballistic projectiles** simultaneously at a **10 Hz simulation tick rate** and **60+ FPS presentation/render rate**, on maps up to **81,920 × 81,920 units**, featuring a seamless **Strategic Zoom** camera (from 3D close-up cubes to 2D NATO-style point sprites).

### 1.2 Core Hypothesis to Validate
1. **Absolute Determinism:** Two headless simulation instances running the same inputs must produce the exact same 64-bit checksum hash over 10,000+ ticks across all platforms.
2. **Flow-Field Scalability:** 5,000 units must pathfind towards moving destinations through choke points without CPU frame-drops using hierarchical 2D Vector Flow Fields (no per-unit A*).
3. **Ballistic Simulation at Scale:** 2,000 physical 3D projectile arcs must be simulated and tested for collision against moving units and terrain via a sparse 2D spatial grid in < 5 ms per tick.
4. **Strategic Zoom Presentation:** The camera must seamlessly zoom from ground level until the entire map is visible, transitioning from 3D instanced meshes to 2D pixel dots with zero hitching.

### 1.3 Non-Goals for this PoC
* No complex networking transports (sockets/NAT/UDP). Testing is done via headless multi-instance local verification.
* No audio engine, full faction asymmetry, tech trees, or 3D skeletal animations.
* No terrain texturing or PBR lighting (simple unlit or flat-shaded instanced geometry).
* No naval, subsurface, or air behavior. Only land units are simulated, but positions, velocities, terrain, and pathfinding APIs must not assume land-only movement.

### 1.4 World Scale
These constants apply to every system. Benchmarks and gates run at the maximum map size unless stated otherwise.

| Constant | Value | Notes |
| :--- | :--- | :--- |
| World unit | 1 meter | |
| Tick | 100 ms | Speeds and gravity are stored per tick |
| Maximum map size | 81,920 × 81,920 units | Anything outside the map bounds is despawned or clamped, which keeps the math limits in §4.1 valid |
| Gravity | `Fp::from_bits(401)` per tick² | About 9.8 m/s² |
| Terrain cell | 20 units | 4,096 × 4,096 cells at maximum map size |
| Flow field sector | 64 × 64 cells (1,280 units) | 64 × 64 sectors at maximum map size |
| Spatial grid cell | 8 units | Keeps dense crowds cheap for the short-range separation queries, which run for every unit on every tick |

---

## 2. Technical Stack & Dependencies

Add the following to `Cargo.toml`:

```toml
[package]
name = "sim-engine"
version = "0.1.0"
edition = "2021"

[dependencies]
# Deterministic Math
fixed = { version = "1.28", features = ["serde"] }

# Lightweight Entity Component System
hecs = "0.10"

# Graphics & Windowing
wgpu = "0.20"
winit = { version = "0.29", features = ["rwh_05"] }
bytemuck = { version = "1.16", features = ["derive"] }
pollster = "0.3"

# Presentation Math (Only used for rendering interpolation, NOT simulation)
glam = "0.27"

# Deterministic Hashing
twox-hash = "1.6"

# Profiling & CLI
log = "0.4"
env_logger = "0.11"

[dev-dependencies]
criterion = "0.5"

[[bench]]
name = "sim_benchmark"
harness = false
```

---

## 3. Architecture & Directory Layout

```text
sim-engine/
├── Cargo.toml
├── clippy.toml                 # Bans HashMap, HashSet, and wall-clock types in simulation code
├── SPECIFICATION.md
├── src/
│   ├── main.rs                 # Application entry point, window loop, and mode switch (headless vs windowed)
│   ├── lib.rs                  # Library root, so that tests and benchmarks can reach the modules
│   ├── driver.rs               # Runs the simulation on its own thread and hands out snapshots
│   ├── ai.rs                   # The scripted opponent, which acts only through commands
│   ├── soak.rs                 # Plays and replays many AI-vs-AI matches
│   ├── ui.rs                   # Resource bar, build menu, and unit overlays (egui)
│   ├── math/
│   │   ├── mod.rs
│   │   ├── fp.rs               # Fixed-point aliases (I20F12, I40F24), Vec2Fp, Vec3Fp, integer sqrt/trig
│   │   ├── rect.rs             # Fixed-point 2D AABB and distance helpers
│   │   └── rng.rs              # Seeded integer RNG owned by SimWorld
│   ├── sim/
│   │   ├── mod.rs
│   │   ├── config.rs           # World-scale constants and per-world settings
│   │   ├── blueprints.rs       # Unit data, loaded from data/units.toml
│   │   ├── production.rs       # Flow economy, construction, factories, reclaim, and adjacency
│   │   ├── visibility.rs       # Fog of war
│   │   ├── scenario.rs         # Fixed scenarios and scripted orders shared by tests, benchmarks, and the app
│   │   ├── components.rs       # ECS components (SimId, Position, Velocity, Unit, Team, Health, Combat, Projectile)
│   │   ├── terrain.rs          # Height map, water level, per-movement-class cost grids
│   │   ├── spatial.rs          # Sparse sorted 2D grid for neighbor/collision lookups
│   │   ├── flowfield.rs        # Sector portal graph, Dijkstra integration, and cached sector flow fields
│   │   ├── combat.rs           # Target acquisition, ballistic projectile integration, swept collision, damage
│   │   ├── commands.rs         # Deterministic Command definitions (Move, AttackMove, Stop)
│   │   └── world.rs            # SimWorld: Root simulation state, advance_tick(), checksum generation
│   ├── render/
│   │   ├── mod.rs
│   │   ├── camera.rs           # Strategic Zoom camera (smooth interpolation, ortho/perspective blend)
│   │   ├── instances.rs        # GPU instance buffers, per-instance transforms and colors
│   │   ├── pipeline.rs         # WGPU pipeline, vertex/instance buffer layouts
│   │   └── shaders.wgsl        # Shader: 3D cube instancing transitioning to 2D point sprites by distance
│   └── input/
│       ├── mod.rs
│       ├── selection.rs        # Drag-box selection and ground raycasting (screen to world)
│       └── orders.rs           # Right-click orders and structure placement checks
├── tests/
│   ├── determinism_test.rs     # Dual-instance, golden-checksum, and checksum-sensitivity tests
│   ├── navigation_test.rs      # Phase 2 gate, arrival, separation, and moving goals
│   ├── combat_test.rs          # Swept collision, terrain masking, dodging, and friendly fire
│   ├── driver_test.rs          # The threaded driver matches a direct headless run
│   ├── input_test.rs           # Drag-select and right-click, from screen coordinates to moving units
│   ├── economy_test.rs         # The game rules: economy, construction, reclaim, fog, and assassination
│   ├── ai_test.rs              # The scripted opponent: full matches, replays, and fog of war
│   ├── match_test.rs           # A player's session against the AI, through the threaded driver
│   └── golden/                 # Checked-in expected checksums for fixed scenarios
└── benches/
    └── sim_benchmark.rs        # Criterion benchmark measuring tick execution time for 5,000 units
```

---

## 4. Module Specifications

### 4.1 Fixed-Point Math (`src/math/fp.rs`)
All simulation math must be deterministic across all CPU architectures. **Standard floating-point numbers (`f32`, `f64`) are strictly prohibited in `src/sim/`.**

`Fp` stores positions, velocities, and per-unit values. Its maximum is about 524,288, so squaring a difference larger than about 724 overflows. **Every squared length, dot product, and intermediate product must use `FpWide`.** With map bounds enforced, the largest squared 3D distance is about 2.0 × 10¹⁰, which fits in `FpWide` (maximum about 5.5 × 10¹¹), and its square root fits in `Fp`.

Simulation constants use `Fp::lit("0.5")` or `Fp::from_bits(..)`, never a float conversion. Simulation code must not rely on arithmetic wrapping: the unoptimized test run in §7 exists to catch overflow.

```rust
use fixed::types::{I20F12, I40F24};

pub type Fp = I20F12;     // 20 bits integer, 12 bits fractional (~0.000244 precision)
pub type FpWide = I40F24; // Exact product of two Fp values

pub fn wide_mul(a: Fp, b: Fp) -> FpWide {
    FpWide::from_bits(a.to_bits() as i64 * b.to_bits() as i64)
}

// The raw bits of an FpWide have 24 fractional bits, so their integer
// square root has 12 fractional bits: exactly the raw bits of an Fp.
pub fn sqrt_wide(value: FpWide) -> Fp {
    Fp::from_bits((value.to_bits() as u64).isqrt() as i32)
}

#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Vec2Fp {
    pub x: Fp,
    pub y: Fp,
}

#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Vec3Fp {
    pub x: Fp,
    pub y: Fp,
    pub z: Fp,
}

impl Vec2Fp {
    pub const ZERO: Self = Self { x: Fp::ZERO, y: Fp::ZERO };
    pub fn new(x: Fp, y: Fp) -> Self { Self { x, y } }
    
    pub fn length_squared(&self) -> FpWide {
        wide_mul(self.x, self.x) + wide_mul(self.y, self.y)
    }
    
    pub fn distance_squared(&self, other: &Self) -> FpWide {
        let dx = self.x - other.x;
        let dy = self.y - other.y;
        wide_mul(dx, dx) + wide_mul(dy, dy)
    }
    
    // Returns ZERO for a zero-length vector; callers handle that case explicitly.
    pub fn normalized(&self) -> Self {
        let length = sqrt_wide(self.length_squared()).to_bits() as i64;
        if length == 0 {
            return Self::ZERO;
        }
        Self {
            x: Fp::from_bits((((self.x.to_bits() as i64) << 12) / length) as i32),
            y: Fp::from_bits((((self.y.to_bits() as i64) << 12) / length) as i32),
        }
    }
}
```

### 4.2 ECS Components (`src/sim/components.rs`)
Use `hecs` to store entities in contiguous archetype arrays.

```rust
use crate::math::fp::{Fp, Vec3Fp};

// Assigned from a counter on spawn and never reused. The stable identity for
// ordering, hashing, and interpolation. hecs::Entity is only a storage handle.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SimId(pub u32);

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Team {
    Blue,
    Red,
}

// Selects a cost grid. The PoC only spawns Land units.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum MoveClass {
    Land,
    Hover,
    Naval,
    Seabed,
    Air,
}

#[derive(Copy, Clone, Debug)]
pub struct Position(pub Vec3Fp);

#[derive(Copy, Clone, Debug)]
pub struct Velocity(pub Vec3Fp);

#[derive(Copy, Clone, Debug)]
pub struct UnitStats {
    pub max_speed: Fp,
    pub radius: Fp,
    pub team: Team,
    pub move_class: MoveClass,
}

#[derive(Copy, Clone, Debug)]
pub struct Health {
    pub current: Fp,
    pub max: Fp,
}

#[derive(Copy, Clone, Debug)]
pub struct Combat {
    pub range: Fp,
    pub reload_ticks: u32,
    pub ticks_until_ready: u32,
    pub projectile_speed: Fp,
    pub projectile_damage: Fp,
}

#[derive(Copy, Clone, Debug)]
pub struct Projectile {
    pub velocity: Vec3Fp,
    pub damage: Fp,
    pub source: SimId,
    pub source_team: Team,
    pub lifetime_ticks: u32,
}
```

**Determinism rules for all simulation code:**
* Never depend on `hecs` query order, `hecs::Entity` values, or recycled entity IDs. Any loop whose order can affect results iterates `SimWorld::units` or `SimWorld::projectiles` (§4.6), which are always sorted by `SimId`.
* No `HashMap` or `HashSet`, whose iteration order is randomized, and no wall-clock time. `clippy.toml` enforces this crate-wide with `disallowed-types`, with explicit `#[allow]`s only in `src/render/`, `src/input/`, and `main.rs`. `src/sim/` also denies `clippy::float_arithmetic`.
* All randomness comes from `SimWorld::rng`. Every tie-break falls back to the lowest `SimId`.
* Any work budget is a constant count per tick and never depends on elapsed time.

### 4.3 Sparse Spatial Grid (`src/sim/spatial.rs`)
A dense grid would have millions of cells to walk every tick at maximum map size, so each team's entries are stored as a sorted list with a small index of where each row starts. Memory and rebuild cost depend on the unit count plus the row count, not the map's area, and results are deterministic by construction.

* Mobile units and structures are stored in separate lists, so that a large structure's radius does not widen the short-range queries that every mobile unit runs on every tick.
* Entries are sorted by `(row, column, SimId)`, using a counting sort by row followed by a sort within each row.
* A query binary-searches each row of cells it covers and walks that row's entries. Results arrive in a fixed order: team, row, column, then `SimId`.
* Entries carry their position and radius inline, so queries avoid ECS lookups, and a query can be limited to one team, so rear units do not pay for scanning their own army when looking for enemies.
* A query with a radius above 512 units scans all entries, so map-range weapons stay bounded by the unit count.
* Only units are inserted. Projectiles query the grid but are not stored in it.
* The grid is rebuilt twice per tick (§4.6), so combat always sees post-movement positions.

```rust
pub struct SpatialGrid {
    lists: [TeamEntries; 4],    // Per team and layer: sorted entries and row starts
    by_index: Vec<GridEntry>,   // Each unit's entry by its index in SimWorld::units
}

impl SpatialGrid {
    pub fn rebuild(&mut self, units: impl Iterator<Item = GridUnit>);
    pub fn for_each_candidate(&self, team: Option<Team>, layer: Layer, center_x: i32, center_y: i32, radius: i32, visit: impl FnMut(&GridEntry));
    pub fn query_radius(&self, center: Vec2Fp, radius: Fp, out: &mut Vec<hecs::Entity>);
}
```

### 4.4 Hierarchical Flow Field Pathfinding (`src/sim/flowfield.rs`)
A single 256 × 256 field gives cells 320 units wide on a maximum-size map, which hides choke points. A single field at the 20-unit terrain resolution has about 16.8 million cells, which is too slow to recompute during play. Routing is therefore split into a coarse sector graph and small flow fields built only where units need them:

1. **Cost Grid:** One 4,096 × 4,096 grid per `MoveClass` (1 byte per cell, 16 MiB each). 1–254 = passable cost, 255 = impassable. Only `Land` is allocated in the PoC.
2. **Portal Graph:** Built at map load. Each contiguous passable span along a sector edge is a portal. Portals in the same sector are linked when connected, with the path cost as the edge weight.
3. **Route:** A move command runs Dijkstra outward from the goal over the portal graph once per unit group, breaking ties by lowest portal index. This gives every portal its distance to the goal and the sector it continues into, so a group spread across many sectors shares one search (about 0.3 ms at maximum map size). Cost grid changes rebuild the graph and existing routes.
4. **Sector Flow Field:**
   * Dijkstra runs within one 64 × 64 sector from its target cells: the goal cell or the exit portal cells. BFS is not valid because cell costs vary.
   * A portal seeds only the sector it does not continue into. Seeding both sides would let units bounce back and forth across it.
   * Cells along a portal are seeded with a one-step lookahead into the next sector, so that they differ in value. Without this, units cross each border head-on and walk in staircases.
   * Orthogonal steps cost `10 × cell cost` and diagonal steps `14 × cell cost`. Integration values are `u32`, since a worst-case sector path exceeds `u16`.
   * Each cell stores a direction code pointing to the neighbor that reached it first with the lowest value, using a fixed neighbor order for ties. Diagonals may not cut the corner of an impassable cell.
5. **Lazy Build & Cache:**
   * Fields are built only for sectors occupied by units on that route, plus the next sector along it.
   * At most `MAX_FIELD_BUILDS_PER_TICK` fields are built per tick, in request order. The value starts at 8 and is tuned from the Phase 3 benchmark.
   * A unit whose field is not built yet holds position until it is.
   * Fields are cached per route and sector, with least-recently-used eviction at a fixed entry limit. A stale field stays in use until its replacement is built, so that units do not stutter when a goal moves.
6. **Moving Destinations:** A moving goal is re-routed only when it changes cells, and at most once every 10 ticks per group.
7. **Steering & Local Separation:**
   * Units sample the flow direction of the cell they occupy.
   * If two units overlap within their radii, apply a soft repelling push impulse:
     $$\vec{F}_{\text{push}} = \frac{\vec{Pos}_A - \vec{Pos}_B}{\|\vec{Pos}_A - \vec{Pos}_B\|} \cdot (\text{Radius}_A + \text{Radius}_B - \text{Dist})$$
   * The total push per tick is capped at the larger of half the unit's radius and its speed, so that fast units cannot simply overpower separation.
   * Units at exactly the same position push along a direction derived from their `SimId`s.
   * Motion, including push, is applied one axis at a time against the cost grid, so no unit center can enter an impassable cell. A unit that a new structure lands on walks straight out.
   * A moving unit that bumps into a settled friend near its goal counts as arrived, because a crowd can cover a shared goal such as a rally point.
   * A ground unit's `z` is set from the height map after moving.

### 4.5 Ballistics & Combat System (`src/sim/combat.rs`)
* **Target Acquisition:** Units query `SpatialGrid::query_radius` for nearby enemy units and pick the nearest, breaking ties by lowest `SimId`. A unit without a target searches on ticks where `(SimId + tick) % 5 == 0`, which spreads the cost across ticks.
* **Firing:** If ready, spawn a `Projectile` entity aimed at the target's current position. Shots do not lead the target, so moving units can physically dodge slow shells. No trigonometry is needed. Given horizontal distance $d$, height difference $\Delta z$, and horizontal speed $s$:
  $$t = \max(1, \lceil d / s \rceil)$$
  $$\vec{V}_{xy} = \vec{\Delta}_{xy} / t$$
  $$V_z = \frac{\Delta z}{t} + \frac{\text{Gravity} \cdot (t - 1)}{2}$$
  This lands the projectile on the aim point after $t$ ticks under the integration below, apart from fixed-point rounding that grows with $t$ (up to roughly $t / 4096$ units). The rounding is deterministic and small enough to read as weapon spread.
* **Physics Integration:** Every tick, update projectile:
  $$\vec{P}_{t+1} = \vec{P}_t + \vec{V}_t$$
  $$\vec{V}_{z, t+1} = \vec{V}_{z, t} - \text{Gravity}$$
* **Swept Collision Check:** A fast shell can travel farther than a unit's width in one 100 ms tick, so testing only its end position would let it pass through targets. Each tick, test the whole segment $\vec{P}_t \to \vec{P}_{t+1}$:
  * **Units:** Query the grid around the segment midpoint with radius `half segment length + largest unit radius`. A unit is hit if its closest distance to the segment is within its radius. The hit earliest along the segment wins, then the lowest `SimId`. The firing unit is always excluded.
  * **Terrain:** Sample the height map along the segment at intervals of at most 20 units. Terrain hit earlier than any unit stops the projectile, which is what makes hills and cliffs block low-trajectory fire.
  * **On a unit hit:** Deduct health from the target and delete the projectile. If health $\le 0$, despawn the unit entity.
  * **Friendly Fire:** `SimConfig::friendly_fire` controls whether allied units can be hit. It is off for the PoC scenarios and on for the game (see `GAME_SPECIFICATION.md` §7.1).

### 4.6 Simulation World & State Hashing (`src/sim/world.rs`)
The central simulation container:

```rust
pub struct SimWorld {
    pub ecs: hecs::World,
    pub terrain: Terrain,
    pub spatial_grid: SpatialGrid,
    pub flow_field: FlowField,
    pub rng: SimRng,
    pub current_tick: u32,
    pub next_sim_id: u32,
    // Sorted by SimId: spawns append, despawns retain order.
    pub units: Vec<(SimId, hecs::Entity)>,
    pub projectiles: Vec<(SimId, hecs::Entity)>,
    pub obstacles: Vec<RectFp>,
}

impl SimWorld {
    pub fn new(config: SimConfig, seed: u64) -> Self;
    
    // Advances the world by exactly one 100ms deterministic tick.
    // Commands are applied in slice order, so callers must supply the same order on every instance.
    pub fn advance_tick(&mut self, commands: &[SimCommand]) {
        // 1. Process deterministic commands
        // 2. Rebuild Spatial Grid
        // 3. Update Unit Movement via Flow Field + Local Separation
        // 4. Rebuild Spatial Grid with post-movement positions
        // 5. Update Combat, Weapons, and Spawning Projectiles
        // 6. Update Projectile Physics & Swept Collisions
        // 7. Cleanup Dead Entities
        self.current_tick += 1;
    }

    // Computes a 64-bit checksum of the entire simulation state.
    // Every value that can affect a future tick must be hashed. Hasher::write_i32
    // and friends use native endianness, so all values go through to_le_bytes.
    pub fn compute_checksum(&self) -> u64 {
        use twox_hash::XxHash64;
        use std::hash::Hasher;

        let mut hasher = XxHash64::with_seed(0);
        hasher.write(&self.current_tick.to_le_bytes());
        hasher.write(&self.next_sim_id.to_le_bytes());
        hasher.write(&self.rng.state().to_le_bytes());

        for &(id, entity) in self.units.iter().chain(&self.projectiles) {
            hasher.write(&id.0.to_le_bytes());
            self.hash_entity_components(entity, &mut hasher);
        }
        self.flow_field.hash_state(&mut hasher);
        hasher.finish()
    }
}
```

`hash_entity_components` covers every component field: position, velocity, health, reload timers, targets, team, and remaining projectile lifetime. Derived caches that are rebuilt from hashed state, such as the spatial grid, are excluded.

### 4.7 Terrain (`src/sim/terrain.rs`)
Terrain is simulation state, because it affects line of fire, unit height, and movement.

* **Height Map:** 4,097 × 4,097 `i16` samples at maximum map size (about 32 MiB), in steps of 1/16 unit at 20-unit spacing. Sampling is bilinear in fixed-point.
* **Water Level:** A single `Fp` value per map, stored for later naval and seabed movement. The PoC scenarios keep it below the terrain.
* **PoC Maps:** Generated from `SimRng` and the map seed, using hills and box obstacles. No map file format is needed yet.
* **Structures:** Placing or destroying a structure changes the cost grid and rebuilds the portal graph and every route. On the 10 km skirmish map, this costs about 8–15 ms for that one tick. It affects about 0.3% of ticks and runs on the simulation's own thread, so it does not delay frames, but larger maps would need an incremental rebuild.

---

## 5. Rendering & Presentation Pipeline

### 5.1 Strategic Zoom Camera (`src/render/camera.rs`)
The camera smoothly scales along an exponential zoom curve:
* **Near Plane (Close-up):** Altitude = 10 units. Pitch = 45°. Field of view = Perspective 3D.
* **Far Plane (Orbital):** Altitude = 100,000 units, enough for a 45° vertical field of view to show a maximum-size map. Scales down with smaller maps. Pitch = 89° (nearly top-down orthographic).
* **Smooth Zooming:** Scroll wheel adjusts target altitude; camera uses exponential decay toward the target to avoid jarring transitions.
* **Depth Range:** Use a reversed-Z `Depth32Float` buffer so geometry does not flicker across the full altitude range.
* **Camera-Relative Coordinates:** `f32` has a precision of only about 0.008 units at the far edge of a maximum-size map, which would visibly jitter in close-up. Compute camera and interpolated positions in `f64`, subtract the camera position, and send the resulting camera-relative `f32` values to the GPU.

### 5.2 GPU Instanced Rendering (`src/render/instances.rs` & `shaders.wgsl`)
* **Single Draw Call for Units:** All 5,000 units are rendered via one instanced `wgpu::RenderPass::draw_indexed` call. Projectiles use a second instanced call.
* **Terrain:** A single camera-following grid of 512 × 512 vertices, with vertex spacing that doubles as altitude increases. The vertex shader reads heights from a texture of the height map. Flat shading only.
* **Simulation Thread:** The simulation runs on its own thread and hands the renderer per-tick snapshots keyed by `SimId`, so a slow tick cannot stall a frame. Rendering never writes to simulation state. Entities present in only one snapshot are drawn at their known position without interpolation.
* **Instance Buffer Structure:**
  ```rust
  #[repr(C)]
  #[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
  pub struct UnitInstanceRaw {
      pub model_matrix: [[f32; 4]; 4], // Interpolated, camera-relative 3D transform
      pub color: [f32; 4],             // Team color (Blue / Red)
  }
  ```
* **Render-Time Interpolation:**
  The renderer maintains two simulation buffers: `State_Previous` (Tick $N$) and `State_Current` (Tick $N+1$). For each render frame at time $t$:
  $$\alpha = \frac{t - t_{\text{tick\_start}}}{0.100\text{ s}}$$
  $$\vec{Pos}_{\text{render}} = \text{lerp}(\vec{Pos}_{\text{previous}}, \vec{Pos}_{\text{current}}, \alpha)$$
* **Shader LOD Morphing:**
  In `shaders.wgsl`, if camera distance to the unit exceeds `LOD_THRESHOLD`, the vertex shader ignores unit rotation and collapses geometry to a screen-aligned billboard quad (2D tactical dot) with a minimum on-screen size, so units stay visible when zoomed all the way out.

---

## 6. Implementation Roadmap & Acceptance Criteria

Execute the implementation in strict sequential phases:

### Phase 1: Headless Core & Determinism (Days 1–3)
* Implement `math::fp`, `math::rng`, `sim::components`, `sim::terrain`, and `sim::world`.
* Unit test the math at its limits, including a maximum-range distance and `sqrt_wide` on exact squares.
* Write `tests/determinism_test.rs`:
  * Spawn 1,000 units with positions from a seeded `SimRng`.
  * Run two independent `SimWorld` instances for 1,000 ticks with identical injected command inputs.
  * **Dual-Instance Gate:** `assert_eq!(world_a.compute_checksum(), world_b.compute_checksum())` passes.
  * **Golden Gate:** The checksum matches a value checked into `tests/golden/`. Two instances in one process cannot reveal platform differences, so the same test also runs on a second architecture (§7).
  * **Sensitivity Gate:** Changing any single hashed field changes the checksum, covering at least position, velocity, health, reload timer, and RNG state.
  * **Full-Length Gate (from Phase 3):** Rerun with the Phase 3 combat scenario for 10,000 ticks, in release builds only.

### Phase 2: Flow Field Navigation (Days 4–7)
* Implement `sim::flowfield.rs`: the portal graph, Dijkstra sector fields, lazy building, and caching.
* Implement static box obstacle masking on a maximum-size map.
* Spawn 5,000 units and issue a move command with 3 obstacles between them and the goal, on a route crossing at least 3 sectors.
* **Gate:** On every tick, no unit center is inside an impassable cell. At least 95% of units are within 128 units of the goal within 3× the ideal travel time.

### Phase 3: Spatial Grid & Ballistic Combat (Days 8–11)
* Implement `sim::spatial.rs` and `sim::combat.rs`, including swept unit and terrain collision.
* Place 2,500 Blue units vs. 2,500 Red units on a maximum-size map, close enough to form a dense battle.
* Simulate 2,000 concurrent projectiles in flight with gravity trajectories.
* Add Criterion benchmark (`benches/sim_benchmark.rs`). Losses would make the workload easier over time, so the benchmark keeps 5,000 units and 2,000 projectiles alive for the whole run.
* Add collision tests: a fast shell cannot pass through a unit between ticks, a hill blocks a low shot, and a moving unit can dodge a slow shell.
* **Gate:** On an Apple M3, single-threaded, for 5,000 units + 2,000 projectiles, `advance_tick()` executes in **< 6.0 ms at the 99th percentile** and the projectile step in **< 5.0 ms**.

### Phase 4: WGPU Instanced Renderer & Camera (Days 12–15)
* Implement `render::pipeline`, `render::instances`, and `render::camera`.
* Render 5,000 cubes with GPU instancing, plus projectiles and terrain.
* Implement camera scroll-zoom and interpolation between ticks.
* **Gate:** The application holds **60+ FPS** during full combat, with vsync on, over 60 seconds including a full zoom from minimum to maximum altitude (`--autotest 60`, without `--capture`, which disturbs frame timing):
  * Average frame rate of at least 59.5 FPS.
  * 99th-percentile work per frame of ≤ 16.7 ms, excluding the wait for vsync.
  * 99th-percentile frame interval of ≤ 25 ms, which is the point where a frame has clearly missed a 60 Hz vsync. Intervals cannot be compared with 16.7 ms directly, because the nominal 60 Hz interval is 16.67 ms and timestamp jitter alone exceeds that margin.
  * A windowed run and a headless run with the same inputs produce the same checksum.

### Phase 5: Selection & Interactive Control (Days 16–18)
* Implement `input::selection`: Left-click drag selection box and right-click move command.
* Enable full user interaction in the windowed application.
* **Gate:** An automated test drives selection and orders from screen coordinates through to moving units. Real mouse input and the feel of the controls are checked by hand.

---

## 7. How to Validate the Build

The agent or engineer implementing this specification must pass all of the following commands:

```bash
# 1. Run all unit and deterministic desync tests
cargo test --release

# 2. Rerun the tests unoptimized so arithmetic overflow fails loudly
cargo test

# 3. Check the determinism rules in simulation code
cargo clippy --all-targets -- -D warnings

# 4. Check the golden checksums on a second architecture (x86-64 via Rosetta on Apple Silicon)
rustup target add x86_64-apple-darwin
cargo test --release --target x86_64-apple-darwin --test determinism_test

# 5. Run simulation performance benchmarks
cargo bench

# 6. Run the frame-rate gate in a window, then confirm that a headless run prints the same checksum
cargo run --release -- --autotest 60
cargo run --release -- --headless 600

# 7. Launch the visual interactive proof of concept
cargo run --release
```

When `cargo run --release` is executed:
1. 2,500 Blue units and 2,500 Red units spawn facing each other.
2. The user can drag-box select units and right-click to command them via flow fields.
3. Smooth strategic zoom is controllable via the mouse scroll wheel.
4. Units engage in continuous ballistic combat with physical projectiles.
5. An FPS counter in the window title displays $\ge 60$ FPS.
