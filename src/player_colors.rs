//! Each player's color, given out before the match starts, and how the interface shows them.
//!
//! Colors are the interface's alone: nothing in the simulation reads them, so they can never
//! make two runs of a match differ. Before the match, each player takes a color by their place on
//! their team: Blue's players cool colors, the first of them blue, and Red's warm ones, the first
//! of them red, so that which side a unit is on still reads at a glance. The person then chooses,
//! during the match, between every player in their own color and every ally in one shared ally
//! color (`AllyColors`).

use crate::sim::components::{PlayerId, Team};
use egui::Color32;

/// How a person sees the other players on the field.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum AllyColors {
    /// Every player in the color they were given before the match.
    #[default]
    Unique,
    /// The person's own units in their team's color, and every ally's in one lighter shade of it,
    /// cyan beside blue or orange beside red. The other team likewise: its first player in its
    /// color, and the rest in its lighter shade.
    Shared,
}

impl AllyColors {
    pub const fn toggled(self) -> Self {
        match self {
            Self::Unique => Self::Shared,
            Self::Shared => Self::Unique,
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::Unique => "Ally colors: each their own",
            Self::Shared => "Ally colors: one shared color",
        }
    }
}

/// A player's color, as units wear it on their trim and as the interface draws it.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct PlayerColor {
    /// On a unit's trim, in the linear light the scene is drawn in.
    pub trim: [f32; 3],
    /// On the minimap, the score panel, and the map's icons.
    pub ui: Color32,
}

const fn color(trim: [f32; 3], ui: [u8; 3]) -> PlayerColor {
    PlayerColor { trim, ui: Color32::from_rgb(ui[0], ui[1], ui[2]) }
}

/// Most players a team has: a 4v4.
pub const MOST_A_TEAM: usize = 4;

/// Blue's players' colors, in the order they take them: blue, violet, sea green, and cyan, four
/// cool hues at least 25 degrees apart. Cyan, the allies' shared shade (`BLUE_ALLIES`), comes
/// last, so that in a 2v2 or 3v3 switching to shared colors still shows.
pub const BLUE_SIDE: [PlayerColor; MOST_A_TEAM] = [
    color([0.20, 0.50, 1.00], [90, 160, 255]),
    color([0.62, 0.40, 1.00], [175, 135, 255]),
    color([0.15, 0.80, 0.50], [60, 210, 140]),
    BLUE_ALLIES,
];

/// Red's players' colors, in the order they take them: red, yellow, pink, and orange, four warm
/// hues, with its allies' shared shade (`RED_ALLIES`) last, as on Blue's side.
pub const RED_SIDE: [PlayerColor; MOST_A_TEAM] = [
    color([1.00, 0.27, 0.22], [255, 95, 80]),
    color([0.98, 0.88, 0.22], [245, 220, 70]),
    color([0.95, 0.35, 0.75], [245, 110, 200]),
    RED_ALLIES,
];

/// With ally colors shared, every player of Blue's but its first in this lighter shade of its
/// blue: the one a team match used for allies before every player had their own.
pub const BLUE_ALLIES: PlayerColor = color([0.25, 0.85, 0.95], [70, 220, 240]);
/// And Red's in this orange.
pub const RED_ALLIES: PlayerColor = color([1.00, 0.62, 0.18], [255, 165, 50]);

const fn shared_shade(team: Team) -> PlayerColor {
    match team {
        Team::Blue => BLUE_ALLIES,
        Team::Red => RED_ALLIES,
    }
}

const fn side(team: Team) -> &'static [PlayerColor; MOST_A_TEAM] {
    match team {
        Team::Blue => &BLUE_SIDE,
        Team::Red => &RED_SIDE,
    }
}

/// A team's own color: its first player's, and the one it shows under its name.
pub const fn team_color(team: Team) -> PlayerColor {
    side(team)[0]
}

/// The colors the players were given before the match, by player.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PlayerColors {
    /// By player: their team, and their place on it.
    places: Vec<(Team, usize)>,
}

impl PlayerColors {
    /// Gives each player a color by their place on their team, from the match's players' teams,
    /// by player (`World::players`). The same players are always given the same colors, and no
    /// two players the same one.
    pub fn assign(players: &[Team]) -> Self {
        let mut counts = [0_usize; 2];
        let places = players
            .iter()
            .map(|&team| {
                let place = counts[team.index()];
                counts[team.index()] += 1;
                (team, place)
            })
            .collect();
        Self { places }
    }

    /// The color a player was given.
    pub fn given(&self, player: PlayerId) -> Option<PlayerColor> {
        self.places.get(player.index()).map(|&(team, place)| side(team)[place.min(MOST_A_TEAM - 1)])
    }

    /// How the interface shows each player, with `mode` chosen.
    pub fn shown(&self, mode: AllyColors) -> Palette {
        let by_player = self
            .places
            .iter()
            .map(|&(team, place)| match mode {
                AllyColors::Unique => side(team)[place.min(MOST_A_TEAM - 1)],
                AllyColors::Shared if place > 0 => shared_shade(team),
                AllyColors::Shared => side(team)[0],
            })
            .collect();
        Palette { mode, by_player }
    }
}

/// Each player's color as the interface shows them now, with the person's choice applied.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Palette {
    pub mode: AllyColors,
    /// By player. Empty for none given, as in a test: then each team's first player is in its
    /// color and the rest in its lighter shade.
    by_player: Vec<PlayerColor>,
}

impl Palette {
    /// The color of a unit of `team`, owned by `owner` where the viewer knows whose it is, and
    /// `ally`, a player's of its team other than its first.
    pub fn unit(&self, team: Team, owner: Option<PlayerId>, ally: bool) -> PlayerColor {
        owner.and_then(|owner| self.by_player.get(owner.index()).copied()).unwrap_or_else(|| if ally { shared_shade(team) } else { team_color(team) })
    }

    /// A player's color.
    pub fn player(&self, team: Team, player: PlayerId) -> PlayerColor {
        self.by_player.get(player.index()).copied().unwrap_or_else(|| team_color(team))
    }

    fn id() -> egui::Id {
        egui::Id::new("player palette")
    }

    /// Hands the palette to the interface for this frame.
    pub fn store(self, ctx: &egui::Context) {
        ctx.data_mut(|data| data.insert_temp(Self::id(), self));
    }

    /// The palette the interface was handed, or none given.
    pub fn of(ctx: &egui::Context) -> Self {
        ctx.data(|data| data.get_temp::<Self>(Self::id())).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TWO_V_TWO: [Team; 4] = [Team::Blue, Team::Red, Team::Blue, Team::Red];

    /// Every player in a match is given a color of their own, before it starts, by their place on
    /// their team; and the same players are given the same ones every time.
    #[test]
    fn every_player_is_given_a_color_of_their_own() {
        let teams: Vec<Team> = (0..8).map(|player| if player % 2 == 0 { Team::Blue } else { Team::Red }).collect();
        let colors = PlayerColors::assign(&teams);
        let given: Vec<PlayerColor> = (0..8).map(|player| colors.given(PlayerId(player)).expect("every player has one")).collect();
        for (one, color) in given.iter().enumerate() {
            for other in &given[one + 1..] {
                assert_ne!(color.ui, other.ui, "no two players share a color");
                assert_ne!(color.trim, other.trim);
            }
        }
        // Each team's first keeps its color, so a 1v1 looks as it always has.
        assert_eq!(given[0], team_color(Team::Blue));
        assert_eq!(given[1], team_color(Team::Red));
        assert_eq!(PlayerColors::assign(&teams), colors);
        // Blue's are cool and Red's warm: more blue than red in each of Blue's, and the reverse.
        for (player, color) in given.iter().enumerate() {
            let [red, _, blue] = color.trim;
            assert_eq!(blue > red, player % 2 == 0, "player {player}'s color reads as their side's");
        }
    }

    /// Unique, an ally is in their own color; shared, every ally is in the one ally shade.
    #[test]
    fn the_person_chooses_unique_or_shared_ally_colors() {
        let teams = [Team::Blue, Team::Red, Team::Blue, Team::Red, Team::Blue, Team::Red];
        let colors = PlayerColors::assign(&teams);
        let unique = colors.shown(AllyColors::Unique);
        let shared = colors.shown(AllyColors::Shared);
        let (you, first_ally, second_ally) = (PlayerId(0), PlayerId(2), PlayerId(4));
        assert_ne!(unique.player(Team::Blue, first_ally), unique.player(Team::Blue, second_ally));
        assert_eq!(shared.player(Team::Blue, first_ally), shared.player(Team::Blue, second_ally));
        assert_eq!(shared.player(Team::Blue, first_ally), BLUE_ALLIES);
        // Even with a single ally, the two differ.
        assert_ne!(unique.player(Team::Blue, first_ally), shared.player(Team::Blue, first_ally));
        // Either way, the person's own stay in their team's color.
        assert_eq!(unique.player(Team::Blue, you), team_color(Team::Blue));
        assert_eq!(shared.player(Team::Blue, you), team_color(Team::Blue));
        assert_eq!(AllyColors::Unique.toggled(), AllyColors::Shared);
        assert_eq!(AllyColors::Shared.toggled(), AllyColors::Unique);
    }

    /// A unit whose owner the viewer does not know, an enemy structure remembered out of sight,
    /// is in its team's color.
    #[test]
    fn a_unit_of_no_known_owner_is_in_its_teams_color() {
        let palette = PlayerColors::assign(&TWO_V_TWO).shown(AllyColors::Unique);
        assert_eq!(palette.unit(Team::Red, None, false), team_color(Team::Red));
        assert_eq!(palette.unit(Team::Red, Some(PlayerId(3)), true), RED_SIDE[1]);
        // With no colors given, the first player's and the shared shade, as before.
        assert_eq!(Palette::default().unit(Team::Blue, Some(PlayerId(2)), true), BLUE_ALLIES);
    }
}
