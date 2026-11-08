use super::components::{SimId, Team};
use super::config::{SPATIAL_CELL_UNITS, SPATIAL_SCAN_RADIUS_UNITS};
use crate::math::fp::{wide_mul, Fp, Vec2Fp, FP_FRAC_BITS};

const CELL_RAW: i32 = SPATIAL_CELL_UNITS << FP_FRAC_BITS;
const LIST_COUNT: usize = 6;

/// Mobile units and structures are stored apart, so that a 3-cell factory's large radius
/// does not widen the short-range queries that every mobile unit runs on every tick. Aircraft
/// are apart from both, so that only what is meant to reach them does: ground shells, blasts,
/// and collisions never look there.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Layer {
    Mobile,
    Structure,
    Air,
}

const fn layer_of(is_structure: bool, is_air: bool) -> Layer {
    if is_air {
        Layer::Air
    } else if is_structure {
        Layer::Structure
    } else {
        Layer::Mobile
    }
}

const fn list_index(team: Team, layer: Layer) -> usize {
    team.index() * 3 + layer as usize
}

/// A unit's per-tick state, stored inline so queries avoid ECS lookups.
#[derive(Copy, Clone, Debug)]
pub struct GridEntry {
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub radius: i32,
    pub id: SimId,
    /// Index into `SimWorld::units`, valid until the next cleanup.
    pub index: u32,
    pub entity: hecs::Entity,
    pub team: Team,
    pub is_structure: bool,
    /// Flying, in the air layer.
    pub is_air: bool,
    /// Has no order this tick. Moving units settle when they bump into one near their goal.
    pub is_idle: bool,
    cell_x: u16,
}

/// Input to `SpatialGrid::rebuild`.
#[derive(Copy, Clone, Debug)]
pub struct GridUnit {
    pub id: SimId,
    pub entity: hecs::Entity,
    pub team: Team,
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub radius: i32,
    pub is_structure: bool,
    pub is_air: bool,
    pub is_idle: bool,
}

/// One list's occupied cells: entries sorted by `(row, column, SimId)`,
/// with `row_start` giving the slice for each row.
#[derive(Default)]
struct TeamEntries {
    entries: Vec<GridEntry>,
    row_start: Vec<u32>,
}

/// Sparse grid. Memory and rebuild cost depend on the unit count plus the row
/// count, so large empty maps cost almost nothing, and results are deterministic
/// by construction because no hashing or ECS iteration order is involved.
pub struct SpatialGrid {
    rows: usize,
    columns: i32,
    lists: [TeamEntries; LIST_COUNT],
    /// Every unit's entry by `SimWorld::units` index, for looking up a known target.
    by_index: Vec<GridEntry>,
}

impl SpatialGrid {
    pub fn new(map_units: i32) -> Self {
        let columns = map_units / SPATIAL_CELL_UNITS + 1;
        let rows = columns as usize;
        assert!(columns <= i32::from(u16::MAX), "map too large for u16 grid columns");
        Self {
            rows,
            columns,
            lists: std::array::from_fn(|_| TeamEntries {
                entries: Vec::new(),
                row_start: vec![0; rows + 1],
            }),
            by_index: Vec::new(),
        }
    }

    fn cell(&self, raw: i32) -> i32 {
        (raw / CELL_RAW).clamp(0, self.columns - 1)
    }

    /// Rebuilds from units given in `SimId` order.
    pub fn rebuild(&mut self, units: impl Iterator<Item = GridUnit>) {
        self.by_index.clear();
        for list in &mut self.lists {
            list.row_start.fill(0);
        }
        // Counting sort by row, which keeps SimId order within each row.
        for (index, unit) in units.enumerate() {
            let cell_x = (unit.x / CELL_RAW).clamp(0, self.columns - 1) as u16;
            self.by_index.push(GridEntry {
                x: unit.x,
                y: unit.y,
                z: unit.z,
                radius: unit.radius,
                id: unit.id,
                index: index as u32,
                entity: unit.entity,
                team: unit.team,
                is_structure: unit.is_structure,
                is_air: unit.is_air,
                is_idle: unit.is_idle,
                cell_x,
            });
            let row = self.cell(unit.y) as usize;
            self.lists[list_index(unit.team, layer_of(unit.is_structure, unit.is_air))].row_start[row + 1] += 1;
        }
        for list in &mut self.lists {
            let mut total = 0;
            for start in &mut list.row_start {
                total += *start;
                *start = total;
            }
            list.entries.clear();
            if let Some(&first) = self.by_index.first() {
                list.entries.resize(total as usize, first);
            }
        }
        // Each row's cursor starts at its row_start and advances as entries land.
        let mut cursors: [Vec<u32>; LIST_COUNT] =
            std::array::from_fn(|list| self.lists[list].row_start[..self.rows].to_vec());
        for entry in &self.by_index {
            let row = ((entry.y / CELL_RAW).clamp(0, self.columns - 1)) as usize;
            let list = list_index(entry.team, layer_of(entry.is_structure, entry.is_air));
            let cursor = &mut cursors[list][row];
            self.lists[list].entries[*cursor as usize] = *entry;
            *cursor += 1;
        }
        for list in &mut self.lists {
            for row in 0..self.rows {
                let (start, end) = (list.row_start[row] as usize, list.row_start[row + 1] as usize);
                if end - start > 1 {
                    list.entries[start..end].sort_unstable_by_key(|entry| (entry.cell_x, entry.id));
                }
            }
        }
    }

    /// The entry for a `SimWorld::units` index.
    pub fn entry(&self, index: usize) -> &GridEntry {
        &self.by_index[index]
    }

    pub fn entries(&self) -> &[GridEntry] {
        &self.by_index
    }

    /// Visits every entry of one layer in the cells covered by the query box, optionally
    /// for one team. This can include entries beyond `radius`, so callers filter by
    /// distance. Order is fixed: team, row, column, then `SimId`.
    pub fn for_each_candidate(
        &self,
        team: Option<Team>,
        layer: Layer,
        center_x: i32,
        center_y: i32,
        radius: i32,
        mut visit: impl FnMut(&GridEntry),
    ) {
        let blue = [list_index(Team::Blue, layer)];
        let red = [list_index(Team::Red, layer)];
        let both = [blue[0], red[0]];
        let selected: &[usize] = match team {
            Some(Team::Blue) => &blue,
            Some(Team::Red) => &red,
            None => &both,
        };
        let scan_all = radius > SPATIAL_SCAN_RADIUS_UNITS << FP_FRAC_BITS;
        let min_x = self.cell(center_x.saturating_sub(radius));
        let max_x = self.cell(center_x.saturating_add(radius));
        let min_y = self.cell(center_y.saturating_sub(radius));
        let max_y = self.cell(center_y.saturating_add(radius));
        for &list_index in selected {
            let list = &self.lists[list_index];
            if list.entries.is_empty() {
                continue;
            }
            if scan_all {
                list.entries.iter().for_each(&mut visit);
                continue;
            }
            for row in min_y as usize..=max_y as usize {
                let (start, end) = (list.row_start[row] as usize, list.row_start[row + 1] as usize);
                if start == end {
                    continue;
                }
                let slice = &list.entries[start..end];
                let first = slice.partition_point(|entry| i32::from(entry.cell_x) < min_x);
                for entry in &slice[first..] {
                    if i32::from(entry.cell_x) > max_x {
                        break;
                    }
                    visit(entry);
                }
            }
        }
    }

    /// Collects entities within `radius` of `center`, on any team and layer.
    pub fn query_radius(&self, center: Vec2Fp, radius: Fp, out: &mut Vec<hecs::Entity>) {
        let radius_squared = wide_mul(radius, radius);
        let (center_x, center_y) = (center.x.to_bits(), center.y.to_bits());
        for layer in [Layer::Mobile, Layer::Structure, Layer::Air] {
            self.for_each_candidate(None, layer, center_x, center_y, radius.to_bits(), |entry| {
                let position = Vec2Fp::new(Fp::from_bits(entry.x), Fp::from_bits(entry.y));
                if position.distance_squared(&center) <= radius_squared {
                    out.push(entry.entity);
                }
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::fp::fp_int;
    use crate::math::rng::SimRng;

    fn world_with_entities(count: usize) -> (hecs::World, Vec<hecs::Entity>) {
        let mut world = hecs::World::new();
        let entities = (0..count).map(|_| world.spawn(())).collect();
        (world, entities)
    }

    fn unit(id: u32, entity: hecs::Entity, team: Team, x: Fp, y: Fp) -> GridUnit {
        GridUnit {
            id: SimId(id),
            entity,
            team,
            x: x.to_bits(),
            y: y.to_bits(),
            z: 0,
            radius: fp_int(1).to_bits(),
            is_structure: false,
            is_air: false,
            is_idle: false,
        }
    }

    /// The grid must return exactly what a brute-force scan returns, at any radius.
    #[test]
    fn query_matches_brute_force() {
        let map = 81_920;
        let mut rng = SimRng::new(11);
        let (_world, entities) = world_with_entities(2_000);
        let units: Vec<GridUnit> = entities
            .iter()
            .enumerate()
            .map(|(i, &entity)| {
                let team = if i % 2 == 0 { Team::Blue } else { Team::Red };
                // Half are clustered so that small-radius queries find something.
                let (min, max) = if i % 4 < 2 { (fp_int(1_000), fp_int(1_300)) } else { (Fp::ZERO, fp_int(map)) };
                unit(i as u32, entity, team, rng.range_fp(min, max), rng.range_fp(min, max))
            })
            .collect();
        let mut grid = SpatialGrid::new(map);
        grid.rebuild(units.iter().copied());

        for radius in [1, 8, 30, 100, 512, 513, 40_000] {
            for _ in 0..25 {
                let center = Vec2Fp::new(rng.range_fp(fp_int(900), fp_int(1_400)), rng.range_fp(fp_int(900), fp_int(1_400)));
                let radius = fp_int(radius);
                let mut found = Vec::new();
                grid.query_radius(center, radius, &mut found);
                found.sort();
                let mut expected: Vec<hecs::Entity> = units
                    .iter()
                    .filter(|u| {
                        let position = Vec2Fp::new(Fp::from_bits(u.x), Fp::from_bits(u.y));
                        position.distance_squared(&center) <= wide_mul(radius, radius)
                    })
                    .map(|u| u.entity)
                    .collect();
                expected.sort();
                assert_eq!(found, expected, "radius {radius}");
            }
        }
    }

    #[test]
    fn team_filter_and_candidate_order_are_fixed() {
        let (_world, entities) = world_with_entities(4);
        let at = fp_int(100);
        let mut grid = SpatialGrid::new(1_280);
        grid.rebuild(
            [
                unit(3, entities[0], Team::Red, at, at),
                unit(5, entities[1], Team::Blue, at, at),
                unit(8, entities[2], Team::Red, at, at),
                unit(9, entities[3], Team::Blue, at + fp_int(1), at),
            ]
            .into_iter(),
        );
        let mut red = Vec::new();
        grid.for_each_candidate(Some(Team::Red), Layer::Mobile, at.to_bits(), at.to_bits(), fp_int(4).to_bits(), |e| red.push(e.id.0));
        assert_eq!(red, [3, 8]);
        let mut all = Vec::new();
        grid.for_each_candidate(None, Layer::Mobile, at.to_bits(), at.to_bits(), fp_int(4).to_bits(), |e| all.push(e.id.0));
        assert_eq!(all, [5, 9, 3, 8]);
        assert_eq!(grid.entry(2).id, SimId(8));
    }

    #[test]
    fn layers_keep_structures_apart_from_mobile_units() {
        let (_world, entities) = world_with_entities(2);
        let at = fp_int(100);
        let structure = GridUnit { is_structure: true, ..unit(1, entities[0], Team::Blue, at, at) };
        let mut grid = SpatialGrid::new(1_280);
        grid.rebuild([structure, unit(2, entities[1], Team::Blue, at, at)].into_iter());
        let visit = |layer| {
            let mut found = Vec::new();
            grid.for_each_candidate(None, layer, at.to_bits(), at.to_bits(), fp_int(4).to_bits(), |e| found.push(e.id.0));
            found
        };
        assert_eq!(visit(Layer::Mobile), [2]);
        assert_eq!(visit(Layer::Structure), [1]);
    }

    #[test]
    fn empty_rebuild_is_safe() {
        let mut grid = SpatialGrid::new(1_280);
        grid.rebuild(std::iter::empty());
        let mut out = Vec::new();
        grid.query_radius(Vec2Fp::ZERO, fp_int(100), &mut out);
        assert!(out.is_empty());
    }
}
