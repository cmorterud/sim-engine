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

/// Fixed neighbor order, which also serves as the tie-break order.
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

    fn mid(&self, sector: u32) -> (usize, usize) {
        self.side_cell(sector, self.length / 2)
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

    /// Runs Dijkstra outward from the seeds. Heap keys are unique `(value, cell)`
    /// pairs, so pop order, and therefore every tie-break, is fully determined.
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
                if next < self.values[neighbor] {
                    self.values[neighbor] = next;
                    // Stepping from the neighbor back to this cell is the way to the goal.
                    self.directions[neighbor] = OPPOSITE[direction];
                    self.heap.push(Reverse((u64::from(next) << 32) | neighbor as u64));
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

pub struct FlowField {
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

    /// Call after any cost grid change. The graph and existing routes rebuild on next use.
    pub fn invalidate(&mut self) {
        self.graph_dirty = true;
        for links in &mut self.link_cache {
            links.clear();
        }
    }

    /// Call after the cost of a square of cells changes, as when a structure goes up or comes
    /// down. Only the sectors it touches forget their distances.
    pub fn invalidate_cells(&mut self, cell_x: u32, cell_y: u32, size: u32) {
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
        let cost = land_cost(terrain);
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
                        let walk = octile(self.portals[a as usize].mid(sector), self.portals[b as usize].mid(sector));
                        self.link(a, b, sector, walk * u32::from(uniform));
                    }
                }
            } else {
                let mut links = Vec::new();
                for (i, &a) in members.iter().enumerate() {
                    let start = local_index(self.portals[a as usize].mid(sector), origin) as u16;
                    let targets: Vec<(u32, u16)> =
                        members[i + 1..].iter().map(|&b| (b, local_index(self.portals[b as usize].mid(sector), origin) as u16)).collect();
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
        nearest_passable_cell(terrain, goal)?;
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
        let cost = land_cost(terrain);
        let cells = self.cells;
        let Some(goal_cell) = nearest_passable_cell(terrain, goal) else {
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
            let walk = self.search.values[local_index(self.portals[portal as usize].mid(goal_sector), origin)];
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
        if let Some(route) = self.routes.get_mut(&id) {
            route.references += 1;
        }
    }

    /// Drops one reference. The route and its fields are freed at zero.
    pub fn release(&mut self, id: u32) {
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
        self.routes.get(&id).map(|route| route.goal)
    }

    /// The open cell the route ends in, which is not the goal's own cell if that is blocked.
    pub fn goal_cell(&self, id: u32) -> Option<(usize, usize)> {
        self.routes.get(&id).map(|route| route.goal_cell)
    }

    pub fn arrive_radius(&self, id: u32) -> Option<Fp> {
        self.routes.get(&id).map(|route| route.arrive_radius)
    }

    pub fn is_following(&self, id: u32) -> bool {
        self.routes.get(&id).is_some_and(|route| route.follow.is_some())
    }

    /// Re-routes moving goals when they change cells, at most once every
    /// `REROUTE_INTERVAL_TICKS`. `locate` returns a unit's current position.
    pub fn update_moving_goals(&mut self, terrain: &Terrain, tick: u32, locate: impl Fn(SimId) -> Option<Vec2Fp>) {
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
        let cost = land_cost(terrain);
        let cells = self.cells;
        let origin = self.sector_origin(sector);
        let route = &self.routes[&id];
        self.search.reset();
        if route.goal_sector == sector {
            self.search.seed(local_index(route.goal_cell, origin), 0, DIR_NONE);
        }
        let mut best_onward: Option<(u32, u32)> = None;
        for &portal_id in &self.sector_portals[sector as usize] {
            let index = portal_id as usize;
            let distance = route.portal_distance[index];
            let onward = route.onward_sector[index];
            // A portal only guides units out of the sector it does not continue into.
            // Seeding both sides would let units bounce back and forth across it.
            if distance == UNREACHED || onward == sector {
                continue;
            }
            if best_onward.is_none_or(|(best, _)| distance < best) {
                best_onward = Some((distance, onward));
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
                            let ahead = route.portal_distance[next] + octile(there, self.portals[next].mid(onward)) + 10;
                            value = value.min(ahead);
                        }
                    }
                }
                if value == UNREACHED {
                    value = distance;
                }
                self.search.seed(local_index(here, origin), value, exit);
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

        // Prepare the next sector along the route, so that units rarely wait at a border.
        if let Some((_, onward)) = best_onward {
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
    }
}

fn land_cost(terrain: &Terrain) -> &[u8] {
    terrain.cost_grid(MoveClass::Land).expect("land cost grid exists")
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
/// Ties resolve in a fixed scan order.
pub fn nearest_passable_cell(terrain: &Terrain, point: Vec2Fp) -> Option<(usize, usize)> {
    let cost = land_cost(terrain);
    let cells = terrain.cells_per_side() as i64;
    let (cx, cy) = terrain.cell_of(point);
    let (cx, cy) = (cx as i64, cy as i64);
    for ring in 0..cells {
        let mut best: Option<(i64, (usize, usize))> = None;
        for y in (cy - ring).max(0)..=(cy + ring).min(cells - 1) {
            for x in (cx - ring).max(0)..=(cx + ring).min(cells - 1) {
                let on_ring = (x - cx).abs() == ring || (y - cy).abs() == ring;
                if on_ring && cost[(y * cells + x) as usize] != COST_IMPASSABLE {
                    let distance = (x - cx).pow(2) + (y - cy).pow(2);
                    if best.is_none_or(|(closest, _)| distance < closest) {
                        best = Some((distance, (x as usize, y as usize)));
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
                position = position + Vec2Fp::new(direction.x * speed, direction.y * speed);
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
