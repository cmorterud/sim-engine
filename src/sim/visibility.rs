//! Fog of war (GAME_SPECIFICATION.md §9). Each team stamps the cells its units can see.

use super::components::Team;
use super::config::VISIBILITY_CELL_UNITS;
use crate::math::fp::{Fp, FP_FRAC_BITS};

const CELL_RAW: i32 = VISIBILITY_CELL_UNITS << FP_FRAC_BITS;

/// Stamps are refreshed in a rotation, so a stamp this recent still counts as visible.
pub const VISION_REFRESH_TICKS: u32 = 5;

/// A kind of sighting, each with its own cells.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Sense {
    Sight,
    Radar,
    Omni,
}

pub struct Visibility {
    side: usize,
    /// Per team and cell, the tick after the last sighting. Zero means never seen.
    stamps: [Vec<u32>; 2],
    /// The same, for radar and omni coverage. Empty until a team first has such a sensor.
    radar: [Vec<u32>; 2],
    omni: [Vec<u32>; 2],
    current_tick: u32,
}

impl Visibility {
    pub fn new(map_units: i32) -> Self {
        let side = (map_units / VISIBILITY_CELL_UNITS) as usize + 1;
        Self { side, stamps: [vec![0; side * side], vec![0; side * side]], radar: [Vec::new(), Vec::new()], omni: [Vec::new(), Vec::new()], current_tick: 0 }
    }

    fn cells_mut(&mut self, sense: Sense, team: Team) -> &mut Vec<u32> {
        let side = self.side;
        let cells = match sense {
            Sense::Sight => &mut self.stamps[team.index()],
            Sense::Radar => &mut self.radar[team.index()],
            Sense::Omni => &mut self.omni[team.index()],
        };
        if cells.is_empty() {
            cells.resize(side * side, 0);
        }
        cells
    }

    fn cells(&self, sense: Sense, team: Team) -> &[u32] {
        match sense {
            Sense::Sight => &self.stamps[team.index()],
            Sense::Radar => &self.radar[team.index()],
            Sense::Omni => &self.omni[team.index()],
        }
    }

    /// Whether a team has a recent sighting of this kind at a point.
    pub fn senses(&self, sense: Sense, team: Team, x: i32, y: i32) -> bool {
        let cells = self.cells(sense, team);
        if cells.is_empty() {
            return false;
        }
        let last = self.side as i32 - 1;
        let cell_x = (x / CELL_RAW).clamp(0, last) as usize;
        let cell_y = (y / CELL_RAW).clamp(0, last) as usize;
        let stamp = cells[cell_y * self.side + cell_x];
        stamp != 0 && stamp + VISION_REFRESH_TICKS > self.current_tick + 1
    }

    /// Whether a team has ever had a sensor of this kind.
    pub fn has_any(&self, sense: Sense, team: Team) -> bool {
        !self.cells(sense, team).is_empty()
    }

    pub const fn side(&self) -> usize {
        self.side
    }

    pub fn set_tick(&mut self, tick: u32) {
        self.current_tick = tick;
    }

    /// Marks a disc as seen by a team this tick.
    pub fn stamp_disc(&mut self, team: Team, x: i32, y: i32, radius: Fp) {
        self.stamp(Sense::Sight, team, x, y, radius);
    }

    /// Marks a disc as sensed, in one way or another, by a team this tick.
    pub fn stamp(&mut self, sense: Sense, team: Team, x: i32, y: i32, radius: Fp) {
        let radius = i64::from(radius.to_bits());
        let cell = i64::from(CELL_RAW);
        let last = self.side as i64 - 1;
        let (x, y) = (i64::from(x), i64::from(y));
        let stamp = self.current_tick + 1;
        let side = self.side;
        let cells = self.cells_mut(sense, team);
        let min_y = ((y - radius) / cell).clamp(0, last);
        let max_y = ((y + radius) / cell).clamp(0, last);
        for row in min_y..=max_y {
            // The width of the disc at this row's center decides the columns to mark.
            let center_y = row * cell + cell / 2;
            let dy = center_y - y;
            let span_squared = radius * radius - dy * dy;
            if span_squared < 0 {
                continue;
            }
            let span = (span_squared as u64).isqrt() as i64;
            let min_x = ((x - span) / cell).clamp(0, last);
            let max_x = ((x + span) / cell).clamp(0, last);
            let start = row as usize * side;
            cells[start + min_x as usize..=start + max_x as usize].fill(stamp);
        }
    }

    pub fn is_visible(&self, team: Team, x: i32, y: i32) -> bool {
        let last = self.side as i32 - 1;
        let cell_x = (x / CELL_RAW).clamp(0, last) as usize;
        let cell_y = (y / CELL_RAW).clamp(0, last) as usize;
        let stamp = self.stamps[team.index()][cell_y * self.side + cell_x];
        stamp != 0 && stamp + VISION_REFRESH_TICKS > self.current_tick + 1
    }

    /// One byte per cell for the renderer: 255 visible, 90 explored, 0 never seen.
    pub fn fog_bytes(&self, team: Team) -> Vec<u8> {
        self.stamps[team.index()]
            .iter()
            .map(|&stamp| match stamp {
                0 => 0,
                stamp if stamp + VISION_REFRESH_TICKS > self.current_tick + 1 => 255,
                _ => 90,
            })
            .collect()
    }

    pub fn hash_state(&self, hasher: &mut impl std::hash::Hasher) {
        for team in &self.stamps {
            for stamp in team {
                hasher.write(&stamp.to_le_bytes());
            }
        }
        // Nothing is written for sensors nobody has had, so checksums from before they existed hold.
        for (tag, cells) in [(1u8, &self.radar[0]), (2, &self.radar[1]), (3, &self.omni[0]), (4, &self.omni[1])] {
            if !cells.is_empty() {
                hasher.write(&[tag]);
                for stamp in cells {
                    hasher.write(&stamp.to_le_bytes());
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::fp::fp_int;

    fn raw(value: i32) -> i32 {
        fp_int(value).to_bits()
    }

    #[test]
    fn disc_is_visible_inside_and_not_outside() {
        let mut visibility = Visibility::new(5_120);
        visibility.set_tick(10);
        visibility.stamp_disc(Team::Blue, raw(1_000), raw(1_000), fp_int(400));
        assert!(visibility.is_visible(Team::Blue, raw(1_000), raw(1_000)));
        assert!(visibility.is_visible(Team::Blue, raw(1_300), raw(1_000)));
        assert!(!visibility.is_visible(Team::Blue, raw(1_600), raw(1_000)));
        assert!(!visibility.is_visible(Team::Blue, raw(1_300), raw(1_300)), "corners of the box are outside the disc");
        assert!(!visibility.is_visible(Team::Red, raw(1_000), raw(1_000)), "teams do not share sight");
    }

    #[test]
    fn sight_lapses_once_it_is_no_longer_refreshed() {
        let mut visibility = Visibility::new(5_120);
        visibility.set_tick(10);
        visibility.stamp_disc(Team::Blue, raw(1_000), raw(1_000), fp_int(200));
        visibility.set_tick(10 + VISION_REFRESH_TICKS - 1);
        assert!(visibility.is_visible(Team::Blue, raw(1_000), raw(1_000)));
        visibility.set_tick(10 + VISION_REFRESH_TICKS);
        assert!(!visibility.is_visible(Team::Blue, raw(1_000), raw(1_000)));
        assert!(visibility.fog_bytes(Team::Blue).contains(&90), "seen ground should be remembered");
    }
}
