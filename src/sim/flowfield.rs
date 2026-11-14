//! Hierarchical flow fields (SPECIFICATION.md §4.4).
//!
//! A portal graph built at map load connects sectors. A route runs Dijkstra outward
//! from the goal over that graph, giving every portal its distance to the goal and
//! the sector it continues into. Sector flow fields are then built lazily, only where
//! units need them, from those distances.

use std::cmp::Reverse;
use std::collections::{BTreeMap, BinaryHeap, VecDeque};
use std::hash::Hasher;

use super::components::{MoveClass, SimId};
use super::config::{
    MAX_CACHED_FIELDS, MAX_FIELD_BUILDS_PER_TICK, REROUTE_INTERVAL_TICKS, SECTOR_CELLS,
};
use super::terrain::{Terrain, COST_IMPASSABLE};
use crate::math::fp::{Fp, Vec2Fp};

const SECTOR_AREA: usize = SECTOR_CELLS * SECTOR_CELLS;
const UNREACHED: u32 = u32::MAX;
const NO_SLOT: u32 = u32::MAX;
const DIR_NONE: u8 = 255;

/// Fixed neighbor order. Ties between them go to the one nearer the map's center (`nearer_center`).
const NEIGHBORS: [(i32, i32); 8] = [(1, 0), (-1, 0), (0, 1), (0, -1), (1, 1), (-1, 1), (1, -1), (-1, -1)];
const STEP_COST: [u32; 8] = [10, 10, 10, 10, 14, 14, 14, 14];
const OPPOSITE: [u8; 8] = [1, 0, 3, 2, 7, 6, 5, 4];
const DIAGONAL_RAW: i32 = 2896; // 1/sqrt(2) as raw Fp bits

const fn direction_vector(direction: usize) -> Vec2Fp {
    let (dx, dy) = NEIGHBORS[direction];
    let scale = if dx != 0 && dy != 0 { DIAGONAL_RAW } else { 4096 };
    Vec2Fp::new(Fp::from_bits(dx * scale), Fp::from_bits(dy * scale))
}

/// Path cost of an unobstructed walk between two cells over cost-1 terrain.
fn octile(a: (usize, usize), b: (usize, usize)) -> u32 {
    let dx = a.0.abs_diff(b.0) as u32;
    let dy = a.1.abs_diff(b.1) as u32;
    10 * dx.max(dy) + 4 * dx.min(dy)
}

/// A contiguous passable span along the border of two sectors.
#[derive(Clone, Debug)]
struct Portal {
    high_sector: u32,
    /// True when the sectors are side by side in x, so the span runs along y.
    crosses_x: bool,
    /// First cell on the high side, on the axis that crosses the border.
    border: u32,
    /// First cell of the span, on the axis along the border.
    start: u32,
    length: u32,
}

impl Portal {
    /// The global cell at `offset` along the span, on the given sector's side.
    fn side_cell(&self, sector: u32, offset: u32) -> (usize, usize) {
        let across = if sector == self.high_sector { self.border } else { self.border - 1 };
        let along = self.start + offset;
        if self.crosses_x {
            (across as usize, along as usize)
        } else {
            (along as usize, across as usize)
        }
    }

    /// The span's middle cell. A span an even number of cells long has two, and it takes the
    /// one nearer the middle of the map, so that a span and its image turned 180 degrees about
    /// the map's center pick cells that are images of each other, and paths mirror.
    fn mid(&self, sector: u32, cells: usize) -> (usize, usize) {
        let half = self.length / 2;
        if self.length % 2 == 1 || half == 0 {
            return self.side_cell(sector, half);
        }
        let low = (self.start + half - 1) as usize;
        let toward_center = if 2 * low + 1 < cells - 1 { half } else { half - 1 };
        self.side_cell(sector, toward_center)
    }

    /// The `NEIGHBORS` index for stepping out of `sector` through this portal.
    fn exit_direction(&self, sector: u32) -> u8 {
        match (self.crosses_x, sector == self.high_sector) {
            (true, false) => 0,
            (true, true) => 1,
            (false, false) => 2,
            (false, true) => 3,
        }
    }
}

#[derive(Copy, Clone, Debug)]
struct PortalEdge {
    to: u32,
    cost: u32,
    /// The sector this edge crosses.
    sector: u32,
}

/// Reusable Dijkstra state for one sector.
struct SectorSearch {
    values: Vec<u32>,
    directions: Vec<u8>,
    heap: BinaryHeap<Reverse<u64>>,
}

impl SectorSearch {
    fn new() -> Self {
        Self {
            values: vec![UNREACHED; SECTOR_AREA],
            directions: vec![DIR_NONE; SECTOR_AREA],
            heap: BinaryHeap::new(),
        }
    }

    fn reset(&mut self) {
        self.values.fill(UNREACHED);
        self.directions.fill(DIR_NONE);
        self.heap.clear();
    }

    /// Adds a target cell. Keeps the cheaper value when a cell is seeded twice.
    fn seed(&mut self, local: usize, value: u32, direction: u8) {
        if value < self.values[local] {
            self.values[local] = value;
            self.directions[local] = direction;
            self.heap.push(Reverse((u64::from(value) << 32) | local as u64));
        }
    }

    /// Adds a cell on the sector's edge that leads out of it. Of two equally good ways out of
    /// one cell, such as a corner on two borders, it keeps the one leading nearer the center.
    fn seed_exit(&mut self, local: usize, value: u32, direction: u8, origin: (usize, usize), cells: usize) {
        if value == self.values[local] && nearer_center(origin, cells, local, direction, self.directions[local]) {
            self.directions[local] = direction;
        } else {
            self.seed(local, value, direction);
        }
    }

    /// Runs Dijkstra outward from the seeds. Heap keys are unique `(value, cell)`
    /// pairs, so pop order is fully determined. Of two equally short ways from a cell, the one
    /// whose next step is nearer the map's center wins, not whichever was found first: then on
    /// a map with 180° symmetry, mirror-image cells point in mirror-image directions.
    fn run(&mut self, cost: &[u8], cells: usize, origin: (usize, usize)) {
        let local_cost = |lx: i32, ly: i32| cost[(origin.1 + ly as usize) * cells + origin.0 + lx as usize];
        let size = SECTOR_CELLS as i32;
        while let Some(Reverse(key)) = self.heap.pop() {
            let (value, local) = ((key >> 32) as u32, (key & 0xFFFF_FFFF) as usize);
            if value != self.values[local] {
                continue;
            }
            let lx = (local % SECTOR_CELLS) as i32;
            let ly = (local / SECTOR_CELLS) as i32;
            for (direction, &(dx, dy)) in NEIGHBORS.iter().enumerate() {
                let (nx, ny) = (lx + dx, ly + dy);
                if nx < 0 || ny < 0 || nx >= size || ny >= size {
                    continue;
                }
                let neighbor_cost = local_cost(nx, ny);
                if neighbor_cost == COST_IMPASSABLE {
                    continue;
                }
                // Diagonals may not cut the corner of an impassable cell.
                if dx != 0 && dy != 0
                    && (local_cost(lx + dx, ly) == COST_IMPASSABLE || local_cost(lx, ly + dy) == COST_IMPASSABLE)
                {
                    continue;
                }
                let next = value + STEP_COST[direction] * u32::from(neighbor_cost);
                let neighbor = ny as usize * SECTOR_CELLS + nx as usize;
                // Stepping from the neighbor back to this cell is the way to the goal.
                let back = OPPOSITE[direction];
                if next < self.values[neighbor] {
                    self.values[neighbor] = next;
                    self.directions[neighbor] = back;
                    self.heap.push(Reverse((u64::from(next) << 32) | neighbor as u64));
                } else if next == self.values[neighbor] && nearer_center(origin, cells, neighbor, back, self.directions[neighbor]) {
                    // Its value is unchanged, so nothing beyond it changes.
                    self.directions[neighbor] = back;
                }
            }
        }
    }
}

/// A built sector flow field: one direction code per cell.
struct Field {
    directions: Box<[u8]>,
    route: u32,
    sector: u32,
    generation: u32,
    last_used: u32,
}

struct Route {
    goal: Vec2Fp,
    goal_cell: (usize, usize),
    goal_sector: u32,
    /// Distance from each portal to the goal, or `UNREACHED`.
    portal_distance: Vec<u32>,
    /// The sector a unit continues into after crossing each portal.
    onward_sector: Vec<u32>,
    /// True when a portal's distance comes straight from a walk to the goal cell.
    reaches_goal_directly: Vec<bool>,
    /// Increases each time the goal moves, which marks older fields as stale.
    generation: u32,
    references: u32,
    arrive_radius: Fp,
    follow: Option<SimId>,
    last_reroute_tick: u32,
    field_slots: Vec<u32>,
    queued: Vec<bool>,
}

/// The result of sampling a route at a position.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum FlowSample {
    Direction(Vec2Fp),
    /// No up-to-date field yet, or the unit is at the goal.
    Hold,
    /// The up-to-date field has no way from this cell to the goal: it is walled off, or on
    /// the far side of ground this unit cannot cross.
    Unreachable,
}

/// Naval and amphibious routes are numbered from here, so that any route can be found from its
/// number alone.
pub const NAVAL_ROUTE_BASE: u32 = 1 << 30;
pub const AMPHIBIOUS_ROUTE_BASE: u32 = 1 << 29;

pub struct FlowField {
    /// Whose ground this field paths over: land, or water for ships.
    class: MoveClass,
    /// For ships, made the first time one needs a route, so that games without ships are
    /// exactly as they were. Calls about a naval route go to it.
    naval: Option<Box<FlowField>>,
    /// The same, for hovercraft and seabed walkers, over land and water both.
    amphibious: Option<Box<FlowField>>,
    sectors_per_side: usize,
    cells: usize,
    portals: Vec<Portal>,
    sector_portals: Vec<Vec<u32>>,
    adjacency: Vec<Vec<PortalEdge>>,
    graph_dirty: bool,
    /// For each sector, walking distances found between points on its borders, by their
    /// places in the sector. They depend only on the sector's own cells, so they stay good
    /// until one of those changes, and rebuilding the graph after a structure goes up only
    /// searches the sectors it touched. Worked out again from the cost grid whenever missing,
    /// so it is never hashed and never changes a result.
    link_cache: Vec<BTreeMap<(u16, u16), u32>>,
    routes: BTreeMap<u32, Route>,
    next_route: u32,
    queue: VecDeque<(u32, u32)>,
    fields: Vec<Option<Field>>,
    free_slots: Vec<u32>,
    search: SectorSearch,
    field_builds_total: u64,
}

impl FlowField {
    pub fn new(terrain: &Terrain) -> Self {
        let cells = terrain.cells_per_side();
        Self {
            class: MoveClass::Land,
            naval: None,
            amphibious: None,
            sectors_per_side: cells / SECTOR_CELLS,
            cells,
            portals: Vec::new(),
            sector_portals: Vec::new(),
            adjacency: Vec::new(),
            graph_dirty: true,
            link_cache: vec![BTreeMap::new(); (cells / SECTOR_CELLS) * (cells / SECTOR_CELLS)],
            routes: BTreeMap::new(),
            next_route: 1,
            queue: VecDeque::new(),
            fields: Vec::new(),
            free_slots: Vec::new(),
            search: SectorSearch::new(),
            field_builds_total: 0,
        }
    }

    fn new_naval(terrain: &Terrain) -> Self {
        Self { class: MoveClass::Naval, next_route: NAVAL_ROUTE_BASE, ..Self::new(terrain) }
    }

    fn new_amphibious(terrain: &Terrain) -> Self {
        Self { class: MoveClass::Hover, next_route: AMPHIBIOUS_ROUTE_BASE, ..Self::new(terrain) }
    }

    /// Whether a route of this number is held by one of this field's others.
    fn held_elsewhere(&self, id: u32) -> bool {
        self.class == MoveClass::Land && id >= AMPHIBIOUS_ROUTE_BASE
    }

    /// The field that holds a route, by its number.
    fn holder(&self, id: u32) -> Option<&Self> {
        if !self.held_elsewhere(id) {
            Some(self)
        } else if id >= NAVAL_ROUTE_BASE {
            self.naval.as_deref()
        } else {
            self.amphibious.as_deref()
        }
    }

    fn holder_mut(&mut self, id: u32) -> Option<&mut Self> {
        if !self.held_elsewhere(id) {
            Some(self)
        } else if id >= NAVAL_ROUTE_BASE {
            self.naval.as_deref_mut()
        } else {
            self.amphibious.as_deref_mut()
        }
    }

    /// This field's others, naval first, that exist.
    fn others_mut(&mut self) -> impl Iterator<Item = &mut Self> {
        self.naval.as_deref_mut().into_iter().chain(self.amphibious.as_deref_mut())
    }

    /// Creates a route over the ground of a movement class: water for ships, and land for
    /// everything else.
    #[allow(clippy::too_many_arguments)]
    pub fn create_route_for(
        &mut self,
        class: MoveClass,
        terrain: &Terrain,
        tick: u32,
        goal: Vec2Fp,
        references: u32,
        arrive_radius: Fp,
        follow: Option<SimId>,
    ) -> Option<u32> {
        if matches!(class, MoveClass::Naval | MoveClass::Submarine) && self.class == MoveClass::Land {
            let naval = self.naval.get_or_insert_with(|| Box::new(Self::new_naval(terrain)));
            return naval.create_route(terrain, tick, goal, references, arrive_radius, follow);
        }
        // Without water, amphibians cross exactly what land units do, and share their routes.
        if matches!(class, MoveClass::Hover | MoveClass::Seabed) && self.class == MoveClass::Land && terrain.has_water() {
            let amphibious = self.amphibious.get_or_insert_with(|| Box::new(Self::new_amphibious(terrain)));
            return amphibious.create_route(terrain, tick, goal, references, arrive_radius, follow);
        }
        self.create_route(terrain, tick, goal, references, arrive_radius, follow)
    }

    /// Call after any cost grid change. The graph and existing routes rebuild on next use.
    pub fn invalidate(&mut self) {
        for other in self.others_mut() {
            other.invalidate();
        }
        self.graph_dirty = true;
        for links in &mut self.link_cache {
            links.clear();
        }
    }

    /// Call after the cost of a square of cells changes, as when a structure goes up or comes
    /// down. Only the sectors it touches forget their distances.
    pub fn invalidate_cells(&mut self, cell_x: u32, cell_y: u32, size: u32) {
        for other in self.others_mut() {
            other.invalidate_cells(cell_x, cell_y, size);
        }
        self.graph_dirty = true;
        let last = self.cells.saturating_sub(1);
        let corners = [(cell_x as usize, cell_y as usize), ((cell_x + size - 1) as usize, (cell_y + size - 1) as usize)];
        let [(low_x, low_y), (high_x, high_y)] = corners.map(|(x, y)| (x.min(last) / SECTOR_CELLS, y.min(last) / SECTOR_CELLS));
        for sy in low_y..=high_y {
            for sx in low_x..=high_x {
                self.link_cache[sy * self.sectors_per_side + sx].clear();
            }
        }
    }

    pub fn portal_count(&self) -> usize {
        self.portals.len()
    }

    /// Total sector fields built so far. Used to verify lazy building.
    pub const fn field_builds_total(&self) -> u64 {
        self.field_builds_total
    }

    pub fn route_count(&self) -> usize {
        self.routes.len()
    }

    pub fn cached_field_count(&self) -> usize {
        self.fields.len() - self.free_slots.len()
    }

    fn sector_of_cell(&self, cell: (usize, usize)) -> u32 {
        ((cell.1 / SECTOR_CELLS) * self.sectors_per_side + cell.0 / SECTOR_CELLS) as u32
    }

    fn sector_origin(&self, sector: u32) -> (usize, usize) {
        let sector = sector as usize;
        (
            (sector % self.sectors_per_side) * SECTOR_CELLS,
            (sector / self.sectors_per_side) * SECTOR_CELLS,
        )
    }

    fn build_graph(&mut self, terrain: &Terrain) {
        let cost = class_cost(terrain, self.class);
        let sectors = self.sectors_per_side;
        let cells = self.cells;
        self.portals.clear();
        self.sector_portals = vec![Vec::new(); sectors * sectors];

        // Find portals along every internal sector border.
        for sy in 0..sectors {
            for sx in 0..sectors {
                let sector = (sy * sectors + sx) as u32;
                if sx + 1 < sectors {
                    let border = (sx + 1) * SECTOR_CELLS;
                    let open = |along: usize| {
                        let y = sy * SECTOR_CELLS + along;
                        cost[y * cells + border - 1] != COST_IMPASSABLE && cost[y * cells + border] != COST_IMPASSABLE
                    };
                    collect_portals(&mut self.portals, &mut self.sector_portals, sector, sector + 1, true, border, sy * SECTOR_CELLS, open);
                }
                if sy + 1 < sectors {
                    let border = (sy + 1) * SECTOR_CELLS;
                    let open = |along: usize| {
                        let x = sx * SECTOR_CELLS + along;
                        cost[(border - 1) * cells + x] != COST_IMPASSABLE && cost[border * cells + x] != COST_IMPASSABLE
                    };
                    collect_portals(&mut self.portals, &mut self.sector_portals, sector, sector + sectors as u32, false, border, sx * SECTOR_CELLS, open);
                }
            }
        }

        // Link portals that are connected within each sector.
        self.adjacency = vec![Vec::new(); self.portals.len()];
        for sector in 0..(sectors * sectors) as u32 {
            let members = self.sector_portals[sector as usize].clone();
            if members.len() < 2 {
                continue;
            }
            let origin = self.sector_origin(sector);
            if let Some(uniform) = uniform_cost(cost, cells, origin) {
                // Open, uniform sectors are the vast majority, and have an exact closed form.
                for (i, &a) in members.iter().enumerate() {
                    for &b in &members[i + 1..] {
                        let walk = octile(self.portals[a as usize].mid(sector, self.cells), self.portals[b as usize].mid(sector, self.cells));
                        self.link(a, b, sector, walk * u32::from(uniform));
                    }
                }
            } else {
                let mut links = Vec::new();
                for (i, &a) in members.iter().enumerate() {
                    let start = local_index(self.portals[a as usize].mid(sector, self.cells), origin) as u16;
                    let targets: Vec<(u32, u16)> =
                        members[i + 1..].iter().map(|&b| (b, local_index(self.portals[b as usize].mid(sector, self.cells), origin) as u16)).collect();
                    let cache = &mut self.link_cache[sector as usize];
                    // One search from this point finds every distance it is missing.
                    if targets.iter().any(|&(_, target)| !cache.contains_key(&(start, target))) {
                        self.search.reset();
                        self.search.seed(usize::from(start), 0, DIR_NONE);
                        self.search.run(cost, cells, origin);
                        for &(_, target) in &targets {
                            cache.insert((start, target), self.search.values[usize::from(target)]);
                        }
                    }
                    for (b, target) in targets {
                        let walk = cache[&(start, target)];
                        if walk != UNREACHED {
                            links.push((a, b, walk));
                        }
                    }
                }
                for (a, b, walk) in links {
                    self.link(a, b, sector, walk);
                }
            }
        }
        self.graph_dirty = false;
    }

    fn link(&mut self, a: u32, b: u32, sector: u32, walk: u32) {
        // The crossing itself costs one orthogonal step.
        let cost = walk + 10;
        self.adjacency[a as usize].push(PortalEdge { to: b, cost, sector });
        self.adjacency[b as usize].push(PortalEdge { to: a, cost, sector });
    }

    fn ensure_graph(&mut self, terrain: &Terrain) {
        if self.graph_dirty {
            self.build_graph(terrain);
            let ids: Vec<u32> = self.routes.keys().copied().collect();
            for id in ids {
                let goal = self.routes[&id].goal;
                self.route_to(id, terrain, goal);
            }
        }
    }

    /// Builds the portal graph now if it is out of date.
    pub fn prepare(&mut self, terrain: &Terrain) {
        // Commanders walk the seabed, so on a map with water there is always an amphibian, and
        // its graph is better built now than in the middle of a game.
        if self.class == MoveClass::Land && terrain.has_water() && self.amphibious.is_none() {
            self.amphibious = Some(Box::new(Self::new_amphibious(terrain)));
        }
        for other in self.others_mut() {
            other.prepare(terrain);
        }
        self.ensure_graph(terrain);
    }

    /// Creates a route. Returns `None` when no passable cell is near the goal.
    /// The new route starts with one reference per unit in the group.
    pub fn create_route(
        &mut self,
        terrain: &Terrain,
        tick: u32,
        goal: Vec2Fp,
        references: u32,
        arrive_radius: Fp,
        follow: Option<SimId>,
    ) -> Option<u32> {
        self.ensure_graph(terrain);
        nearest_passable_cell_for(terrain, self.class, goal)?;
        let id = self.next_route;
        self.next_route += 1;
        let sectors = self.sectors_per_side * self.sectors_per_side;
        self.routes.insert(
            id,
            Route {
                goal,
                goal_cell: (0, 0),
                goal_sector: 0,
                portal_distance: Vec::new(),
                onward_sector: Vec::new(),
                reaches_goal_directly: Vec::new(),
                generation: 0,
                references,
                arrive_radius,
                follow,
                last_reroute_tick: tick,
                field_slots: vec![NO_SLOT; sectors],
                queued: vec![false; sectors],
            },
        );
        self.route_to(id, terrain, goal);
        Some(id)
    }

    /// Runs Dijkstra outward from the goal over the portal graph. Every sector gets
    /// routing data at once, so a group spread across many sectors shares one search.
    fn route_to(&mut self, id: u32, terrain: &Terrain, goal: Vec2Fp) {
        let cost = class_cost(terrain, self.class);
        let cells = self.cells;
        let Some(goal_cell) = nearest_passable_cell_for(terrain, self.class, goal) else {
            return;
        };
        let goal_sector = self.sector_of_cell(goal_cell);
        let origin = self.sector_origin(goal_sector);
        let portal_count = self.portals.len();
        let mut distance = vec![UNREACHED; portal_count];
        let mut onward = vec![0u32; portal_count];
        let mut direct = vec![false; portal_count];
        let mut heap: BinaryHeap<Reverse<(u32, u32)>> = BinaryHeap::new();

        self.search.reset();
        self.search.seed(local_index(goal_cell, origin), 0, DIR_NONE);
        self.search.run(cost, cells, origin);
        for &portal in &self.sector_portals[goal_sector as usize] {
            let walk = self.search.values[local_index(self.portals[portal as usize].mid(goal_sector, self.cells), origin)];
            if walk != UNREACHED {
                distance[portal as usize] = walk + 10;
                onward[portal as usize] = goal_sector;
                direct[portal as usize] = true;
                heap.push(Reverse((walk + 10, portal)));
            }
        }
        while let Some(Reverse((value, portal))) = heap.pop() {
            if value != distance[portal as usize] {
                continue;
            }
            for edge in &self.adjacency[portal as usize] {
                let next = value + edge.cost;
                if next < distance[edge.to as usize] {
                    distance[edge.to as usize] = next;
                    onward[edge.to as usize] = edge.sector;
                    direct[edge.to as usize] = false;
                    heap.push(Reverse((next, edge.to)));
                }
            }
        }

        let route = self.routes.get_mut(&id).expect("route exists");
        route.goal = goal;
        route.goal_cell = goal_cell;
        route.goal_sector = goal_sector;
        route.portal_distance = distance;
        route.onward_sector = onward;
        route.reaches_goal_directly = direct;
        route.generation += 1;
    }

    /// Adds references to a route, for units reassigned to it.
    pub fn retain(&mut self, id: u32) {
        if let Some(route) = self.holder_mut(id).and_then(|field| field.routes.get_mut(&id)) {
            route.references += 1;
        }
    }

    /// Drops one reference. The route and its fields are freed at zero.
    pub fn release(&mut self, id: u32) {
        if self.held_elsewhere(id) {
            if let Some(other) = self.holder_mut(id) {
                other.release(id);
            }
            return;
        }
        let Some(route) = self.routes.get_mut(&id) else {
            return;
        };
        route.references = route.references.saturating_sub(1);
        if route.references > 0 {
            return;
        }
        let route = self.routes.remove(&id).expect("route exists");
        for slot in route.field_slots {
            if slot != NO_SLOT {
                self.fields[slot as usize] = None;
                self.free_slots.push(slot);
            }
        }
    }

    pub fn goal(&self, id: u32) -> Option<Vec2Fp> {
        self.holder(id)?.routes.get(&id).map(|route| route.goal)
    }

    /// The open cell the route ends in, which is not the goal's own cell if that is blocked.
    pub fn goal_cell(&self, id: u32) -> Option<(usize, usize)> {
        self.holder(id)?.routes.get(&id).map(|route| route.goal_cell)
    }

    pub fn arrive_radius(&self, id: u32) -> Option<Fp> {
        self.holder(id)?.routes.get(&id).map(|route| route.arrive_radius)
    }

    pub fn is_following(&self, id: u32) -> bool {
        self.holder(id).is_some_and(|field| field.routes.get(&id).is_some_and(|route| route.follow.is_some()))
    }

    /// Re-routes moving goals when they change cells, at most once every
    /// `REROUTE_INTERVAL_TICKS`. `locate` returns a unit's current position.
    pub fn update_moving_goals(&mut self, terrain: &Terrain, tick: u32, locate: impl Fn(SimId) -> Option<Vec2Fp>) {
        self.update_moving_goals_with(terrain, tick, &locate);
    }

    fn update_moving_goals_with(&mut self, terrain: &Terrain, tick: u32, locate: &dyn Fn(SimId) -> Option<Vec2Fp>) {
        for other in self.others_mut() {
            other.update_moving_goals_with(terrain, tick, locate);
        }
        self.ensure_graph(terrain);
        let mut reroutes = Vec::new();
        for (&id, route) in &mut self.routes {
            let Some(target) = route.follow else {
                continue;
            };
            if tick.wrapping_sub(route.last_reroute_tick) < REROUTE_INTERVAL_TICKS {
                continue;
            }
            match locate(target) {
                None => route.follow = None,
                Some(position) => {
                    if terrain.cell_of(position) != route.goal_cell {
                        route.last_reroute_tick = tick;
                        reroutes.push((id, position));
                    }
                }
            }
        }
        for (id, position) in reroutes {
            self.route_to(id, terrain, position);
        }
    }

    /// Samples a route for a unit, requesting a field build when one is missing or stale.
    /// A stale field stays in use until its replacement is ready, so units do not stutter.
    pub fn sample(&mut self, id: u32, tick: u32, position: Vec2Fp, terrain: &Terrain) -> FlowSample {
        if self.held_elsewhere(id) {
            return self.holder_mut(id).map_or(FlowSample::Hold, |other| other.sample(id, tick, position, terrain));
        }
        let cell = terrain.cell_of(position);
        let sector = self.sector_of_cell(cell);
        let origin = self.sector_origin(sector);
        let Some(route) = self.routes.get_mut(&id) else {
            return FlowSample::Hold;
        };
        let slot = route.field_slots[sector as usize];
        let (field_generation, direction) = if slot == NO_SLOT {
            (None, DIR_NONE)
        } else {
            let field = self.fields[slot as usize].as_mut().expect("slot is occupied");
            field.last_used = tick;
            (Some(field.generation), field.directions[local_index(cell, origin)])
        };
        if field_generation != Some(route.generation) && !route.queued[sector as usize] {
            route.queued[sector as usize] = true;
            self.queue.push_back((id, sector));
        }
        if direction != DIR_NONE {
            FlowSample::Direction(direction_vector(direction as usize))
        } else if field_generation == Some(route.generation) && cell != route.goal_cell {
            FlowSample::Unreachable
        } else {
            FlowSample::Hold
        }
    }

    /// Builds queued sector fields, up to the fixed per-tick budget.
    pub fn build_pending(&mut self, terrain: &Terrain, tick: u32) {
        for other in self.others_mut() {
            other.build_pending(terrain, tick);
        }
        self.ensure_graph(terrain);
        let mut built = 0;
        while built < MAX_FIELD_BUILDS_PER_TICK {
            let Some((id, sector)) = self.queue.pop_front() else {
                break;
            };
            let Some(route) = self.routes.get_mut(&id) else {
                continue;
            };
            route.queued[sector as usize] = false;
            let slot = route.field_slots[sector as usize];
            let is_current = slot != NO_SLOT
                && self.fields[slot as usize].as_ref().is_some_and(|field| field.generation == route.generation);
            if is_current {
                continue;
            }
            self.build_field(id, sector, terrain, tick);
            built += 1;
        }
    }

    fn build_field(&mut self, id: u32, sector: u32, terrain: &Terrain, tick: u32) {
        let cost = class_cost(terrain, self.class);
        let cells = self.cells;
        let origin = self.sector_origin(sector);
        let route = &self.routes[&id];
        self.search.reset();
        if route.goal_sector == sector {
            self.search.seed(local_index(route.goal_cell, origin), 0, DIR_NONE);
        }
        // The nearest of the sectors the route goes on into, and any as near, all of which are
        // prepared: a unit heading diagonally can tie two, and preparing only the first in the
        // portal list left a unit on one side of the map waiting where its mirror image did not.
        let mut best_onward: Option<u32> = None;
        let mut onward_sectors: Vec<u32> = Vec::new();
        for &portal_id in &self.sector_portals[sector as usize] {
            let index = portal_id as usize;
            let distance = route.portal_distance[index];
            let onward = route.onward_sector[index];
            // A portal only guides units out of the sector it does not continue into.
            // Seeding both sides would let units bounce back and forth across it.
            if distance == UNREACHED || onward == sector {
                continue;
            }
            if best_onward.is_none_or(|best| distance < best) {
                best_onward = Some(distance);
                onward_sectors.clear();
            }
            if best_onward == Some(distance) && !onward_sectors.contains(&onward) {
                onward_sectors.push(onward);
            }
            let portal = &self.portals[index];
            let exit = portal.exit_direction(sector);
            for offset in 0..portal.length {
                let here = portal.side_cell(sector, offset);
                let there = portal.side_cell(onward, offset);
                // Look one step ahead so that cells along the span differ in value.
                // Without this, units cross each border head-on and walk in staircases.
                let mut value = UNREACHED;
                if route.reaches_goal_directly[index] {
                    value = octile(there, route.goal_cell) + 10;
                } else {
                    for edge in &self.adjacency[index] {
                        let next = edge.to as usize;
                        if edge.sector == onward && route.portal_distance[next] < distance {
                            let ahead = route.portal_distance[next] + octile(there, self.portals[next].mid(onward, self.cells)) + 10;
                            value = value.min(ahead);
                        }
                    }
                }
                if value == UNREACHED {
                    value = distance;
                }
                self.search.seed_exit(local_index(here, origin), value, exit, origin, cells);
            }
        }
        self.search.run(cost, cells, origin);
        let generation = route.generation;

        let slot = route.field_slots[sector as usize];
        let slot = if slot != NO_SLOT { slot } else { self.allocate_slot() };
        let directions = match self.fields[slot as usize].take() {
            Some(mut field) => {
                field.directions.copy_from_slice(&self.search.directions);
                field.directions
            }
            None => self.search.directions.clone().into_boxed_slice(),
        };
        self.fields[slot as usize] = Some(Field { directions, route: id, sector, generation, last_used: tick });
        self.field_builds_total += 1;
        let route = self.routes.get_mut(&id).expect("route exists");
        route.field_slots[sector as usize] = slot;

        // Prepare the next sectors along the route, so that units rarely wait at a border.
        for onward in onward_sectors {
            let onward_slot = route.field_slots[onward as usize];
            if onward_slot == NO_SLOT && !route.queued[onward as usize] {
                route.queued[onward as usize] = true;
                self.queue.push_back((id, onward));
            }
        }
    }

    fn allocate_slot(&mut self) -> u32 {
        if let Some(slot) = self.free_slots.pop() {
            return slot;
        }
        if self.fields.len() < MAX_CACHED_FIELDS {
            self.fields.push(None);
            return (self.fields.len() - 1) as u32;
        }
        // Evict the least recently used field, breaking ties by route then sector.
        let (slot, route, sector) = self
            .fields
            .iter()
            .enumerate()
            .filter_map(|(slot, field)| field.as_ref().map(|field| (slot, field)))
            .min_by_key(|(_, field)| (field.last_used, field.route, field.sector))
            .map(|(slot, field)| (slot as u32, field.route, field.sector))
            .expect("cache is full, so a field exists");
        self.fields[slot as usize] = None;
        if let Some(route) = self.routes.get_mut(&route) {
            route.field_slots[sector as usize] = NO_SLOT;
        }
        slot
    }

    /// Hashes all state that can affect future ticks. Field contents are derived from
    /// hashed state, but which fields exist changes how units behave, so that is included.
    pub fn hash_state(&self, hasher: &mut impl Hasher) {
        hasher.write(&self.next_route.to_le_bytes());
        for (&id, route) in &self.routes {
            hasher.write(&id.to_le_bytes());
            hasher.write(&route.goal.x.to_bits().to_le_bytes());
            hasher.write(&route.goal.y.to_bits().to_le_bytes());
            hasher.write(&route.generation.to_le_bytes());
            hasher.write(&route.references.to_le_bytes());
            hasher.write(&route.last_reroute_tick.to_le_bytes());
            hasher.write(&route.arrive_radius.to_bits().to_le_bytes());
            hasher.write(&route.follow.map_or(u32::MAX, |target| target.0).to_le_bytes());
        }
        for &(route, sector) in &self.queue {
            hasher.write(&route.to_le_bytes());
            hasher.write(&sector.to_le_bytes());
        }
        for field in self.fields.iter().flatten() {
            hasher.write(&field.route.to_le_bytes());
            hasher.write(&field.sector.to_le_bytes());
            hasher.write(&field.generation.to_le_bytes());
            hasher.write(&field.last_used.to_le_bytes());
        }
        if let Some(naval) = &self.naval {
            hasher.write(&[1]);
            naval.hash_state(hasher);
        }
        if let Some(amphibious) = &self.amphibious {
            hasher.write(&[2]);
            amphibious.hash_state(hasher);
        }
    }
}

#[cfg(test)]
fn land_cost(terrain: &Terrain) -> &[u8] {
    terrain.cost_grid(MoveClass::Land).expect("land cost grid exists")
}

fn class_cost(terrain: &Terrain, class: MoveClass) -> &[u8] {
    terrain.cost_grid(class).expect("land, naval, and amphibious cost grids exist")
}

/// Whether a step from a sector's cell in `direction` is preferred over one in `other`, when
/// both are equally short: the one ending nearer the map's center, and of two equally near, the
/// one turning anticlockwise about it. A 180° turn of the map leaves both rules as they are,
/// unlike any fixed order of directions, so mirror-image cells choose mirror-image steps. The
/// second matters on the line through the center, where the two sides' starts often lie.
fn nearer_center(origin: (usize, usize), cells: usize, local: usize, direction: u8, other: u8) -> bool {
    if other == DIR_NONE || direction == DIR_NONE {
        return false;
    }
    // In half cells, from the center, so that everything stays whole.
    let side = cells as i64;
    let x = 2 * (origin.0 + local % SECTOR_CELLS) as i64 + 1 - side;
    let y = 2 * (origin.1 + local / SECTOR_CELLS) as i64 + 1 - side;
    let key = |direction: u8| {
        let (dx, dy) = NEIGHBORS[direction as usize];
        let (dx, dy) = (i64::from(dx), i64::from(dy));
        let (nx, ny) = (x + 2 * dx, y + 2 * dy);
        (nx * nx + ny * ny, -(x * dy - y * dx))
    };
    key(direction) < key(other)
}

fn local_index(cell: (usize, usize), origin: (usize, usize)) -> usize {
    (cell.1 - origin.1) * SECTOR_CELLS + (cell.0 - origin.0)
}

/// Returns the sector's cost if every cell shares one passable cost.
fn uniform_cost(cost: &[u8], cells: usize, origin: (usize, usize)) -> Option<u8> {
    let first = cost[origin.1 * cells + origin.0];
    if first == COST_IMPASSABLE {
        return None;
    }
    for y in origin.1..origin.1 + SECTOR_CELLS {
        let row = &cost[y * cells + origin.0..y * cells + origin.0 + SECTOR_CELLS];
        if row.iter().any(|&value| value != first) {
            return None;
        }
    }
    Some(first)
}

/// Finds the closest passable cell to a point, searching outward in square rings.
/// Of equally close cells, the one nearest the map's center wins, so that on a map with 180°
/// symmetry the two sides' searches pick mirror images of each other.
pub fn nearest_passable_cell(terrain: &Terrain, point: Vec2Fp) -> Option<(usize, usize)> {
    nearest_passable_cell_for(terrain, MoveClass::Land, point)
}

/// The same, for the ground a movement class can be on.
pub fn nearest_passable_cell_for(terrain: &Terrain, class: MoveClass, point: Vec2Fp) -> Option<(usize, usize)> {
    let cost = class_cost(
        terrain,
        match class {
            MoveClass::Naval | MoveClass::Submarine => MoveClass::Naval,
            MoveClass::Hover | MoveClass::Seabed => MoveClass::Hover,
            MoveClass::Land | MoveClass::Air => MoveClass::Land,
        },
    );
    let cells = terrain.cells_per_side() as i64;
    let (cx, cy) = terrain.cell_of(point);
    let (cx, cy) = (cx as i64, cy as i64);
    // Square rings outward, visiting only each ring's own cells. The nearest on a ring wins,
    // and of equals the one nearest the center, measured in half cells so that it stays whole,
    // and of those, which mirror each other across the line through the point and the center,
    // the one anticlockwise of it. Breaking ties by row and column instead favored one corner
    // of the map over the other; a 180° turn of the map leaves these rules as they are.
    let (px, py) = (2 * cx + 1 - cells, 2 * cy + 1 - cells);
    for ring in 0..cells {
        let mut best: Option<(i64, i64, i64, i64, i64)> = None;
        let mut consider = |x: i64, y: i64| {
            if (0..cells).contains(&x) && (0..cells).contains(&y) && cost[(y * cells + x) as usize] != COST_IMPASSABLE {
                let (hx, hy) = (2 * x + 1 - cells, 2 * y + 1 - cells);
                let turn = px * (hy - py) - py * (hx - px);
                let candidate = ((x - cx).pow(2) + (y - cy).pow(2), hx * hx + hy * hy, -turn, y, x);
                if best.is_none_or(|current| candidate < current) {
                    best = Some(candidate);
                }
            }
        };
        if ring == 0 {
            consider(cx, cy);
        } else {
            for x in cx - ring..=cx + ring {
                consider(x, cy - ring);
                consider(x, cy + ring);
            }
            for y in cy - ring + 1..cy + ring {
                consider(cx - ring, y);
                consider(cx + ring, y);
            }
        }
        if let Some((_, _, _, y, x)) = best {
            return Some((x as usize, y as usize));
        }
    }
    None
}

#[allow(clippy::too_many_arguments)]
fn collect_portals(
    portals: &mut Vec<Portal>,
    sector_portals: &mut [Vec<u32>],
    low_sector: u32,
    high_sector: u32,
    crosses_x: bool,
    border: usize,
    along_origin: usize,
    open: impl Fn(usize) -> bool,
) {
    let mut span_start = None;
    for along in 0..=SECTOR_CELLS {
        let is_open = along < SECTOR_CELLS && open(along);
        match (is_open, span_start) {
            (true, None) => span_start = Some(along),
            (false, Some(start)) => {
                let id = portals.len() as u32;
                portals.push(Portal {
                    high_sector,
                    crosses_x,
                    border: border as u32,
                    start: (along_origin + start) as u32,
                    length: (along - start) as u32,
                });
                sector_portals[low_sector as usize].push(id);
                sector_portals[high_sector as usize].push(id);
                span_start = None;
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::fp::fp_int;
    use crate::math::rect::RectFp;
    use crate::math::rng::SimRng;
    use crate::sim::config::{TerrainStyle, SECTOR_UNITS, TERRAIN_CELL_UNITS};

    fn point(x: i32, y: i32) -> Vec2Fp {
        Vec2Fp::new(fp_int(x), fp_int(y))
    }

    fn rect(min_x: i32, min_y: i32, max_x: i32, max_y: i32) -> RectFp {
        RectFp::new(point(min_x, min_y), point(max_x, max_y))
    }

    fn flat(map_units: i32) -> Terrain {
        Terrain::generate(map_units, TerrainStyle::Flat, &mut SimRng::new(0))
    }

    /// Every edge of the portal graph, as plain numbers, to compare two graphs.
    fn edges(flow: &FlowField) -> Vec<Vec<(u32, u32, u32)>> {
        flow.adjacency.iter().map(|edges| edges.iter().map(|edge| (edge.to, edge.cost, edge.sector)).collect()).collect()
    }

    /// The search as it was, scanning each whole square: slow, and the reference.
    fn nearest_by_squares(terrain: &Terrain, point: Vec2Fp) -> Option<(usize, usize)> {
        let cost = land_cost(terrain);
        let cells = terrain.cells_per_side() as i64;
        let (cx, cy) = terrain.cell_of(point);
        let (cx, cy) = (cx as i64, cy as i64);
        let (px, py) = (2 * cx + 1 - cells, 2 * cy + 1 - cells);
        for ring in 0..cells {
            // Nearest, then nearest the center, then anticlockwise of the point; and the cell.
            type Best = ((i64, i64, i64), (usize, usize));
            let mut best: Option<Best> = None;
            for y in (cy - ring).max(0)..=(cy + ring).min(cells - 1) {
                for x in (cx - ring).max(0)..=(cx + ring).min(cells - 1) {
                    let on_ring = (x - cx).abs() == ring || (y - cy).abs() == ring;
                    if on_ring && cost[(y * cells + x) as usize] != COST_IMPASSABLE {
                        let (hx, hy) = (2 * x + 1 - cells, 2 * y + 1 - cells);
                        let key = ((x - cx).pow(2) + (y - cy).pow(2), hx * hx + hy * hy, -(px * (hy - py) - py * (hx - px)));
                        if best.is_none_or(|(closest, _)| key < closest) {
                            best = Some((key, (x as usize, y as usize)));
                        }
                    }
                }
            }
            if let Some((_, cell)) = best {
                return Some(cell);
            }
        }
        None
    }

    #[test]
    fn the_nearest_passable_cell_is_the_one_a_full_square_scan_finds() {
        let mut terrain = flat(SECTOR_UNITS * 2);
        // Walls and a big block, with points inside, beside, and at the edge of the map.
        terrain.add_box_obstacle(rect(300, 300, 900, 900));
        terrain.add_box_obstacle(rect(1_200, 0, 1_240, 2_000));
        terrain.add_box_obstacle(rect(0, 1_500, 700, 2_560));
        let mut rng = SimRng::new(9);
        for _ in 0..400 {
            let point = Vec2Fp::new(rng.range_fp(fp_int(0), fp_int(2_559)), rng.range_fp(fp_int(0), fp_int(2_559)));
            assert_eq!(nearest_passable_cell(&terrain, point), nearest_by_squares(&terrain, point), "at {point:?}");
        }
        for point in [point(600, 600), point(0, 2_559), point(1_220, 1_000)] {
            assert_eq!(nearest_passable_cell(&terrain, point), nearest_by_squares(&terrain, point));
        }
    }

    #[test]
    fn on_a_map_turned_180_degrees_onto_itself_the_nearest_cells_mirror_each_other() {
        let mut terrain = flat(SECTOR_UNITS * 2);
        // Each block with its mirror image: the map is 2,560 across.
        for (min_x, min_y, max_x, max_y) in [(300, 300, 900, 900), (1_200, 0, 1_240, 1_000), (0, 1_500, 700, 1_700)] {
            terrain.add_box_obstacle(rect(min_x, min_y, max_x, max_y));
            terrain.add_box_obstacle(rect(2_560 - max_x, 2_560 - max_y, 2_560 - min_x, 2_560 - min_y));
        }
        let last = terrain.cells_per_side() - 1;
        let mirror = |(x, y): (usize, usize)| (last - x, last - y);
        let mut rng = SimRng::new(4);
        for _ in 0..400 {
            // Cell centers, whose mirror images are cell centers too.
            let (x, y) = (rng.below(128) as i32 * 20 + 10, rng.below(128) as i32 * 20 + 10);
            let found = nearest_passable_cell(&terrain, point(x, y)).expect("there is open ground");
            let mirrored = nearest_passable_cell(&terrain, point(2_560 - x, 2_560 - y)).expect("there is open ground");
            assert_eq!(mirror(found), mirrored, "from ({x}, {y})");
        }
    }

    #[test]
    fn a_graph_rebuilt_from_cached_distances_matches_one_built_from_scratch() {
        // Walls across several sectors, so that their distances need searches.
        let mut terrain = flat(SECTOR_UNITS * 3);
        terrain.add_box_obstacle(rect(300, 0, 340, 2_000));
        terrain.add_box_obstacle(rect(1_500, 900, 2_600, 940));
        let mut flow = FlowField::new(&terrain);
        flow.prepare(&terrain);
        // Structures go up and come down in one sector, and the graph is rebuilt each time.
        for (x, y, size, blocked) in [(70, 70, 3, true), (100, 20, 2, true), (70, 70, 3, false), (130, 130, 4, true)] {
            terrain.set_structure_cells(x, y, size, blocked);
            flow.invalidate_cells(x, y, size);
            flow.prepare(&terrain);
            let mut fresh = FlowField::new(&terrain);
            fresh.prepare(&terrain);
            assert_eq!(edges(&flow), edges(&fresh), "after a change at {x}, {y}");
        }
    }

    /// Walks a single point along a route, building fields on demand, until it
    /// arrives or the step limit runs out. Fails if it enters an impassable cell.
    fn walk(flow: &mut FlowField, terrain: &Terrain, route: u32, start: Vec2Fp, steps: u32) -> (Vec2Fp, u32) {
        let mut position = start;
        let goal = flow.goal(route).unwrap();
        let speed = fp_int(5);
        for step in 0..steps {
            if position.distance_squared(&goal).to_num::<i64>() < 900 {
                return (position, step);
            }
            flow.build_pending(terrain, step);
            if let FlowSample::Direction(direction) = flow.sample(route, step, position, terrain) {
                position = position + direction.scaled(speed);
                assert!(terrain.is_passable(position), "walked into a wall at {position:?}");
            }
        }
        (position, steps)
    }

    #[test]
    fn open_map_has_one_portal_per_border() {
        let terrain = flat(SECTOR_UNITS * 3);
        let mut flow = FlowField::new(&terrain);
        flow.build_graph(&terrain);
        // 3 x 3 sectors have 6 vertical and 6 horizontal borders.
        assert_eq!(flow.portal_count(), 12);
    }

    #[test]
    fn wall_splits_a_border_into_separate_portals() {
        let mut terrain = flat(SECTOR_UNITS * 2);
        // Crosses the vertical border with a gap on either side.
        terrain.add_box_obstacle(rect(1_260, 600, 1_300, 700));
        let mut flow = FlowField::new(&terrain);
        flow.build_graph(&terrain);
        let spans: Vec<_> = flow.portals.iter().filter(|p| p.crosses_x && p.start < 64).collect();
        assert_eq!(spans.len(), 2);
        assert_eq!(flow.portal_count(), 5);
    }

    #[test]
    fn walks_straight_across_open_sectors() {
        let terrain = flat(SECTOR_UNITS * 4);
        let mut flow = FlowField::new(&terrain);
        let route = flow.create_route(&terrain, 0, point(4_900, 700), 1, fp_int(1), None).unwrap();
        let (position, steps) = walk(&mut flow, &terrain, route, point(100, 700), 3_000);
        assert!(position.distance_squared(&point(4_900, 700)).to_num::<i64>() < 900);
        // 4,800 units at 5 per step is 960 steps. Allow a small margin for building and hugging.
        assert!(steps < 1_050, "took {steps} steps");
    }

    #[test]
    fn diagonal_route_does_not_staircase() {
        let terrain = flat(SECTOR_UNITS * 4);
        let mut flow = FlowField::new(&terrain);
        let route = flow.create_route(&terrain, 0, point(4_900, 4_900), 1, fp_int(1), None).unwrap();
        let (position, steps) = walk(&mut flow, &terrain, route, point(100, 100), 3_000);
        assert!(position.distance_squared(&point(4_900, 4_900)).to_num::<i64>() < 900);
        // An ideal diagonal covers 6,788 units, or 1,358 steps. A Manhattan staircase covers 1,920.
        assert!(steps < 1_500, "took {steps} steps");
    }

    #[test]
    fn route_goes_around_a_wall_through_its_gap() {
        let mut terrain = flat(SECTOR_UNITS * 4);
        // A full-height wall with a gap near the top, spanning several sectors.
        terrain.add_box_obstacle(rect(2_500, 0, 2_560, 4_400));
        let mut flow = FlowField::new(&terrain);
        let route = flow.create_route(&terrain, 0, point(4_800, 300), 1, fp_int(1), None).unwrap();
        let (position, steps) = walk(&mut flow, &terrain, route, point(200, 300), 6_000);
        assert!(position.distance_squared(&point(4_800, 300)).to_num::<i64>() < 900, "stopped at {position:?} after {steps}");
    }

    #[test]
    fn walled_off_goal_is_reported_as_unreachable_once_the_field_is_built() {
        let mut terrain = flat(SECTOR_UNITS * 3);
        terrain.add_box_obstacle(rect(1_900, 0, 1_940, 3_840));
        let mut flow = FlowField::new(&terrain);
        let route = flow.create_route(&terrain, 0, point(3_000, 300), 1, fp_int(1), None).unwrap();
        // Until a field exists there is nothing to go on, so it only holds.
        assert_eq!(flow.sample(route, 0, point(300, 300), &terrain), FlowSample::Hold);
        for tick in 0..4 {
            flow.build_pending(&terrain, tick);
            assert_eq!(flow.sample(route, tick, point(300, 300), &terrain), FlowSample::Unreachable);
        }
        // The goal's own side can still get there.
        assert!(matches!(flow.sample(route, 5, point(2_500, 300), &terrain), FlowSample::Hold | FlowSample::Direction(_)));
    }

    #[test]
    fn goal_inside_a_wall_snaps_to_the_nearest_open_cell() {
        let mut terrain = flat(SECTOR_UNITS);
        terrain.add_box_obstacle(rect(600, 600, 700, 700));
        let mut flow = FlowField::new(&terrain);
        let route = flow.create_route(&terrain, 0, point(650, 650), 1, fp_int(1), None).unwrap();
        let cell = flow.routes[&route].goal_cell;
        assert_ne!(land_cost(&terrain)[cell.1 * terrain.cells_per_side() + cell.0], COST_IMPASSABLE);
    }

    #[test]
    fn fields_are_built_lazily_and_within_budget() {
        let terrain = flat(SECTOR_UNITS * 16);
        let mut flow = FlowField::new(&terrain);
        let route = flow.create_route(&terrain, 0, point(20_000, 20_000), 1, fp_int(1), None).unwrap();
        assert_eq!(flow.field_builds_total(), 0);
        for sector in 0..200 {
            let x = (sector % 16) * SECTOR_UNITS + 10;
            let y = (sector / 16) * SECTOR_UNITS + 10;
            flow.sample(route, 0, point(x, y), &terrain);
        }
        flow.build_pending(&terrain, 0);
        assert_eq!(flow.field_builds_total(), MAX_FIELD_BUILDS_PER_TICK as u64);
    }

    #[test]
    fn released_route_frees_its_fields() {
        let terrain = flat(SECTOR_UNITS * 2);
        let mut flow = FlowField::new(&terrain);
        let route = flow.create_route(&terrain, 0, point(2_000, 2_000), 2, fp_int(1), None).unwrap();
        flow.sample(route, 0, point(10, 10), &terrain);
        flow.build_pending(&terrain, 0);
        assert!(flow.cached_field_count() > 0);
        flow.release(route);
        assert_eq!(flow.route_count(), 1);
        flow.release(route);
        assert_eq!(flow.route_count(), 0);
        assert_eq!(flow.cached_field_count(), 0);
    }

    #[test]
    fn terrain_cell_size_constant_matches_sector_units() {
        assert_eq!(SECTOR_UNITS, SECTOR_CELLS as i32 * TERRAIN_CELL_UNITS);
    }
}
