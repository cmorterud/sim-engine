# Technical Specification: Deterministic High-Scale RTS Simulation Engine (PoC)

## 1. Project Overview & Objective

### 1.1 Objective
Build a minimal, high-performance, deterministic Proof of Concept (PoC) simulation engine in **Rust** capable of simulating **5,000 autonomous units** and **2,000 physical ballistic projectiles** simultaneously at a **10 Hz simulation tick rate** and **60+ FPS presentation/render rate**, featuring a seamless **Strategic Zoom** camera (from 3D close-up cubes to 2D NATO-style point sprites).

### 1.2 Core Hypothesis to Validate
1. **Absolute Determinism:** Two headless simulation instances running the same inputs must produce the exact same 64-bit checksum hash over 10,000+ ticks across all platforms.
2. **Flow-Field Scalability:** 5,000 units must pathfind towards moving destinations through choke points without CPU frame-drops using 2D Vector Flow Fields (no per-unit A*).
3. **Ballistic Simulation at Scale:** 2,000 physical 3D projectile arcs must be simulated and tested for collision against moving units via a 2D spatial hash grid in < 5 ms per tick.
4. **Strategic Zoom Presentation:** The camera must seamlessly zoom from ground level to planetary orbit, transitioning from 3D instanced meshes to 2D pixel dots with zero hitching.

### 1.3 Non-Goals for this PoC
* No complex networking transports (sockets/NAT/UDP). Testing is done via headless multi-instance local verification.
* No audio engine, full faction asymmetry, tech trees, or 3D skeletal animations.
* No terrain texturing or PBR lighting (simple unlit or flat-shaded instanced geometry).

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
├── SPECIFICATION.md
├── src/
│   ├── main.rs                 # Application entry point, window loop, and mode switch (headless vs windowed)
│   ├── math/
│   │   ├── mod.rs
│   │   ├── fp.rs               # Fixed-point type alias (I20F12), Vec2Fp, Vec3Fp, integer sqrt/trig
│   │   └── rect.rs             # Fixed-point 2D AABB and distance helpers
│   ├── sim/
│   │   ├── mod.rs
│   │   ├── components.rs       # ECS components (Position, Velocity, Unit, Team, Health, Combat, Projectile)
│   │   ├── spatial.rs          # 2D Uniform Spatial Hash Grid for O(1) neighbor/collision lookups
│   │   ├── flowfield.rs        # Cost Grid, Integration Field (Dijkstra), and Vector Flow Field
│   │   ├── combat.rs           # Target acquisition, ballistic projectile integration, collision, damage
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
│       └── selection.rs        # Drag-box selection and ground raycasting (screen to world)
├── tests/
│   └── determinism_test.rs     # Dual-instance headless desync validation test
└── benches/
    └── sim_benchmark.rs        # Criterion benchmark measuring tick execution time for 5,000 units
```

---

## 4. Module Specifications

### 4.1 Fixed-Point Math (`src/math/fp.rs`)
All simulation math must be deterministic across all CPU architectures. **Standard floating-point numbers (`f32`, `f64`) are strictly prohibited in `src/sim/`.**

```rust
use fixed::types::I20F12;

pub type Fp = I20F12; // 20 bits integer, 12 bits fractional (~0.000244 precision)

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
    
    // Fast integer-based length approximation or square root
    pub fn length_squared(&self) -> Fp {
        self.x * self.x + self.y * self.y
    }
    
    pub fn distance_squared(&self, other: &Self) -> Fp {
        let dx = self.x - other.x;
        let dy = self.y - other.y;
        dx * dx + dy * dy
    }
}
```

### 4.2 ECS Components (`src/sim/components.rs`)
Use `hecs` to store entities in contiguous archetype arrays.

```rust
use crate::math::fp::{Fp, Vec2Fp, Vec3Fp};

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Team {
    Blue,
    Red,
}

#[derive(Copy, Clone, Debug)]
pub struct Position(pub Vec3Fp);

#[derive(Copy, Clone, Debug)]
pub struct Velocity(pub Vec2Fp);

#[derive(Copy, Clone, Debug)]
pub struct UnitStats {
    pub max_speed: Fp,
    pub radius: Fp,
    pub team: Team,
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
    pub source_team: Team,
    pub lifetime_ticks: u32,
}
```

### 4.3 Uniform Spatial Hash Grid (`src/sim/spatial.rs`)
To resolve collisions and target search in $\mathcal{O}(1)$ without an $\mathcal{O}(N^2)$ loop:
* World is partitioned into a 2D grid where each cell size is slightly larger than the maximum weapon range (e.g., cell size = 32.0 units).
* Spatial grid stores entity handles per cell.
* Cleared and repopulated at the start of each simulation tick.

```rust
pub struct SpatialGrid {
    cell_size: Fp,
    inv_cell_size: Fp,
    width: usize,
    height: usize,
    cells: Vec<Vec<hecs::Entity>>,
}

impl SpatialGrid {
    pub fn new(width: usize, height: usize, cell_size: Fp) -> Self;
    pub fn clear(&mut self);
    pub fn insert(&mut self, entity: hecs::Entity, pos: Vec2Fp);
    pub fn query_radius(&self, center: Vec2Fp, radius: Fp, out: &mut Vec<hecs::Entity>);
}
```

### 4.4 Flow Field Pathfinding (`src/sim/flowfield.rs`)
To guide up to 5,000 units simultaneously without lag:
1. **Cost Grid:** $256 \times 256$ cells (1 byte per cell). 1 = Open terrain, 255 = Impassable obstacle.
2. **Integration Field (Dijkstra Wavefront):**
   * Starting at the target goal cell (cost = 0), expand outward using a 4-neighbor / 8-neighbor Breadth-First Search (BFS).
   * Each cell stores the cumulative distance to the goal (`u16`).
3. **Vector Field (Gradients):**
   * For each cell, calculate the 2D directional vector pointing to the neighbor cell with the lowest integration value:
     $$\vec{V}(x, y) = \text{normalize}\left(\text{Integration}(x - 1, y) - \text{Integration}(x + 1, y), \text{Integration}(x, y - 1) - \text{Integration}(x, y + 1)\right)$$
4. **Steering & Local Separation:**
   * Units sample the vector field of the cell they occupy.
   * If two units overlap within their radii, apply a soft repelling push impulse:
     $$\vec{F}_{\text{push}} = \frac{\vec{Pos}_A - \vec{Pos}_B}{\|\vec{Pos}_A - \vec{Pos}_B\|} \cdot (\text{Radius}_A + \text{Radius}_B - \text{Dist})$$

### 4.5 Ballistics & Combat System (`src/sim/combat.rs`)
* **Target Acquisition:** Units query `SpatialGrid::query_radius` for nearby enemy units. Pick the nearest enemy.
* **Firing:** If ready, spawn a `Projectile` entity with 3D velocity directed at the target's current position, factoring in a ballistic firing angle.
* **Physics Integration:** Every tick, update projectile:
  $$\vec{P}_{t+1} = \vec{P}_t + \vec{V}_t$$
  $$\vec{V}_{z, t+1} = \vec{V}_{z, t} - \text{Gravity}$$
* **Collision Check:** Each projectile queries the spatial grid for enemy units within collision radius. If hit:
  * Deduct health from target.
  * Delete projectile entity.
  * If health $\le 0$, despawn the unit entity.

### 4.6 Simulation World & State Hashing (`src/sim/world.rs`)
The central simulation container:

```rust
pub struct SimWorld {
    pub ecs: hecs::World,
    pub spatial_grid: SpatialGrid,
    pub flow_field: FlowField,
    pub current_tick: u32,
}

impl SimWorld {
    pub fn new() -> Self;
    
    // Advances the world by exactly one 100ms deterministic tick
    pub fn advance_tick(&mut self, commands: &[SimCommand]) {
        // 1. Process deterministic commands
        // 2. Clear & rebuild Spatial Grid
        // 3. Update Unit Movement via Flow Field + Local Separation
        // 4. Update Combat, Weapons, and Spawning Projectiles
        // 5. Update Projectile Physics & Collisions
        // 6. Cleanup Dead Entities
        self.current_tick += 1;
    }

    // Computes a 64-bit checksum of the entire simulation state
    pub fn compute_checksum(&self) -> u64 {
        use twox_hash::XxHash64;
        use std::hash::Hasher;

        let mut hasher = XxHash64::default();
        hasher.write_u32(self.current_tick);

        // Sort entities by ID to ensure deterministic hash traversal
        let mut entities: Vec<_> = self.ecs.iter().map(|(e, _)| e).collect();
        entities.sort_by_key(|e| e.id());

        for e in entities {
            if let Ok(pos) = self.ecs.get::<&Position>(e) {
                hasher.write_i32(pos.0.x.to_bits());
                hasher.write_i32(pos.0.y.to_bits());
                hasher.write_i32(pos.0.z.to_bits());
            }
            if let Ok(health) = self.ecs.get::<&Health>(e) {
                hasher.write_i32(health.current.to_bits());
            }
        }
        hasher.finish()
    }
}
```

---

## 5. Rendering & Presentation Pipeline

### 5.1 Strategic Zoom Camera (`src/render/camera.rs`)
The camera smoothly scales along an exponential zoom curve:
* **Near Plane (Close-up):** Altitude = 10 units. Pitch = 45°. Field of view = Perspective 3D.
* **Far Plane (Orbital):** Altitude = 500 units. Pitch = 89° (nearly top-down orthographic).
* **Smooth Zooming:** Scroll wheel adjusts target altitude; camera uses exponential decay toward the target to avoid jarring transitions.

### 5.2 GPU Instanced Rendering (`src/render/instances.rs` & `shaders.wgsl`)
* **Single Draw Call for Units:** All 5,000 units are rendered via `wgpu::RenderPass::draw_indexed_instanced`.
* **Instance Buffer Structure:**
  ```rust
  #[repr(C)]
  #[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
  pub struct UnitInstanceRaw {
      pub model_matrix: [[f32; 4]; 4], // Interpolated 3D world transform
      pub color: [f32; 4],             // Team color (Blue / Red)
  }
  ```
* **Render-Time Interpolation:**
  The renderer maintains two simulation buffers: `State_Previous` (Tick $N$) and `State_Current` (Tick $N+1$). For each render frame at time $t$:
  $$\alpha = \frac{t - t_{\text{tick\_start}}}{0.100\text{ s}}$$
  $$\vec{Pos}_{\text{render}} = \text{lerp}(\vec{Pos}_{\text{previous}}, \vec{Pos}_{\text{current}}, \alpha)$$
* **Shader LOD Morphing:**
  In `shaders.wgsl`, if camera distance to the unit exceeds `LOD_THRESHOLD`, the vertex shader ignores unit rotation and collapses geometry to a screen-aligned billboard quad (2D tactical dot).

---

## 6. Implementation Roadmap & Acceptance Criteria

Execute the implementation in strict sequential phases:

### Phase 1: Headless Core & Determinism (Days 1–3)
* Implement `math::fp`, `sim::components`, and `sim::world`.
* Write `tests/determinism_test.rs`:
  * Spawn 1,000 units with pseudo-random positions.
  * Run two independent `SimWorld` instances for 1,000 ticks with identical injected command inputs.
  * **Gate:** `assert_eq!(world_a.compute_checksum(), world_b.compute_checksum())` passes.

### Phase 2: Flow Field Navigation (Days 4–7)
* Implement `sim::flowfield.rs` with Dijkstra BFS wave and gradient calculation.
* Implement static box obstacle masking.
* Spawn 5,000 units and issue a move command across a map with 3 obstacles.
* **Gate:** All units stream through choke points without stalling or overlapping inside walls.

### Phase 3: Spatial Grid & Ballistic Combat (Days 8–11)
* Implement `sim::spatial.rs` and `sim::combat.rs`.
* Place 2,500 Blue units vs. 2,500 Red units.
* Simulate 2,000 concurrent projectiles in flight with gravity trajectories.
* Add Criterion benchmark (`benches/sim_benchmark.rs`).
* **Gate:** `advance_tick()` executes in **< 6.0 ms** on modern hardware for 5,000 units + 2,000 projectiles.

### Phase 4: WGPU Instanced Renderer & Camera (Days 12–15)
* Implement `render::pipeline`, `render::instances`, and `render::camera`.
* Render 5,000 cubes with GPU instancing.
* Implement camera scroll-zoom and interpolation between ticks.
* **Gate:** The application runs at a steady **60+ FPS** during full combat.

### Phase 5: Selection & Interactive Control (Days 16–18)
* Implement `input::selection`: Left-click drag selection box and right-click move command.
* Enable full user interaction in the windowed application.

---

## 7. How to Validate the Build

The agent or engineer implementing this specification must pass all of the following commands:

```bash
# 1. Run all unit and deterministic desync tests
cargo test --release

# 2. Run simulation performance benchmarks
cargo bench

# 3. Launch the visual interactive proof of concept
cargo run --release
```

When `cargo run --release` is executed:
1. 2,500 Blue units and 2,500 Red units spawn facing each other.
2. The user can drag-box select units and right-click to command them via flow fields.
3. Smooth strategic zoom is controllable via the mouse scroll wheel.
4. Units engage in continuous ballistic combat with physical projectiles.
5. In-game FPS counter displays $\ge 60$ FPS.
