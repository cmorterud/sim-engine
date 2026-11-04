use super::components::{SimId, Team};
use super::config::{SPATIAL_CELL_UNITS, SPATIAL_SCAN_RADIUS_UNITS};
use crate::math::fp::{wide_mul, Fp, Vec2Fp, FP_FRAC_BITS};

const CELL_RAW: i32 = SPATIAL_CELL_UNITS << FP_FRAC_BITS;
const TEAM_COUNT: usize = 2;

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
}

/// One team's occupied cells: entries sorted by `(row, column, SimId)`,
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
    teams: [TeamEntries; TEAM_COUNT],
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
            teams: std::array::from_fn(|_| TeamEntries {
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
        for team in &mut self.teams {
            team.row_start.fill(0);
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
                cell_x,
            });
            let row = self.cell(unit.y) as usize;
            self.teams[unit.team.index()].row_start[row + 1] += 1;
        }
        for team in &mut self.teams {
            let mut total = 0;
            for start in &mut team.row_start {
                total += *start;
                *start = total;
            }
            team.entries.clear();
            if let Some(&first) = self.by_index.first() {
                team.entries.resize(total as usize, first);
            }
        }
        // Each row's cursor starts at its row_start and advances as entries land.
        let mut cursors: [Vec<u32>; TEAM_COUNT] =
            std::array::from_fn(|team| self.teams[team].row_start[..self.rows].to_vec());
        for entry in &self.by_index {
            let row = ((entry.y / CELL_RAW).clamp(0, self.columns - 1)) as usize;
            let team = entry.team.index();
            let cursor = &mut cursors[team][row];
            self.teams[team].entries[*cursor as usize] = *entry;
            *cursor += 1;
        }
        for team in &mut self.teams {
            for row in 0..self.rows {
                let (start, end) = (team.row_start[row] as usize, team.row_start[row + 1] as usize);
                if end - start > 1 {
                    team.entries[start..end].sort_unstable_by_key(|entry| (entry.cell_x, entry.id));
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

    /// Visits every entry in the cells covered by the query box, optionally for one
    /// team. This can include entries beyond `radius`, so callers filter by distance.
    /// Order is fixed: team, row, column, then `SimId`.
    pub fn for_each_candidate(
        &self,
        team: Option<Team>,
        center_x: i32,
        center_y: i32,
        radius: i32,
        mut visit: impl FnMut(&GridEntry),
    ) {
        let selected: &[usize] = match team {
            Some(Team::Blue) => &[0],
            Some(Team::Red) => &[1],
            None => &[0, 1],
        };
        let scan_all = radius > SPATIAL_SCAN_RADIUS_UNITS << FP_FRAC_BITS;
        let min_x = self.cell(center_x.saturating_sub(radius));
        let max_x = self.cell(center_x.saturating_add(radius));
        let min_y = self.cell(center_y.saturating_sub(radius));
        let max_y = self.cell(center_y.saturating_add(radius));
        for &team_index in selected {
            let team = &self.teams[team_index];
            if scan_all {
                team.entries.iter().for_each(&mut visit);
                continue;
            }
            for row in min_y as usize..=max_y as usize {
                let (start, end) = (team.row_start[row] as usize, team.row_start[row + 1] as usize);
                if start == end {
                    continue;
                }
                let slice = &team.entries[start..end];
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

    /// Collects entities within `radius` of `center`, on any team.
    pub fn query_radius(&self, center: Vec2Fp, radius: Fp, out: &mut Vec<hecs::Entity>) {
        let radius_squared = wide_mul(radius, radius);
        let (center_x, center_y) = (center.x.to_bits(), center.y.to_bits());
        self.for_each_candidate(None, center_x, center_y, radius.to_bits(), |entry| {
            let position = Vec2Fp::new(Fp::from_bits(entry.x), Fp::from_bits(entry.y));
            if position.distance_squared(&center) <= radius_squared {
                out.push(entry.entity);
            }
        });
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
        grid.for_each_candidate(Some(Team::Red), at.to_bits(), at.to_bits(), fp_int(4).to_bits(), |e| red.push(e.id.0));
        assert_eq!(red, [3, 8]);
        let mut all = Vec::new();
        grid.for_each_candidate(None, at.to_bits(), at.to_bits(), fp_int(4).to_bits(), |e| all.push(e.id.0));
        assert_eq!(all, [5, 9, 3, 8]);
        assert_eq!(grid.entry(2).id, SimId(8));
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
