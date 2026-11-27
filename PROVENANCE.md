# Provenance

sim-engine is a free, open-source game under the GNU General Public License version 3
(`LICENSE`). It is a spiritual successor to *Supreme Commander: Forged Alliance*, and is not
affiliated with or endorsed by its makers or by Forged Alliance Forever (FAF). It is its own game:
its own engine, written from scratch, and its own setting, names, and look (GAME_SPECIFICATION.md
§1.1, §14, §20). This file records what came from elsewhere and what was replaced, so that it stays
deliberate, honest, and easy to check. It is the only file that names other games: a CI check
fails on the names in `.github/denylist.txt` anywhere else.

## What the game never contains

* No code from *Forged Alliance*, FAF, or any other game. Everything here is written for this
  project.
* No art, models, textures, sounds, maps, scripts, or blueprint files from either, and nothing that
  downloads or reads them (see Maps from other games).

## Remediation, 2026-11-15

The game was first built with *Forged Alliance*'s names and four factions as working references,
and with starting numbers from FAF. On 2026-11-15, in commits `e805813`, `4a2571b`, `a03ab6e`, and
the documentation change that followed them:

* The four factions, their lore, and their named experimentals and units were removed from
  GAME_SPECIFICATION.md. In their place is one original faction, the Frontier Garrison Corps, in an
  original setting, with a second faction and experimentals to be designed later from the game's
  own roles, not from another game's units.
* Every display name, blueprint id, and code identifier taken from *Forged Alliance* was replaced
  (see Names, below), and the map importer and map downloader were removed.
* Starting numbers are next: they are to be re-derived from this project's own cost model,
  targets, and tuning runs, after which "Starting numbers" below gives way to a dated note. Until
  then, comments in the unit data and code said "FAF's" next to the values that came from FAF,
  so that the record stayed accurate, and no new value is taken from FAF. Since 2026-11-24 they
  say "the inspiration's" instead (see Comments, 2026-11-24, below).

## Comments, 2026-11-24

From 2026-11-24 no file but this one names FAF either: `.github/denylist.txt` has the name, so the
CI check that keeps other games' names here fails on it anywhere else. Comments in the code, the
tests, and `data/units.toml` that said "FAF's" or "as in FAF" next to a value or a rule now say
"the inspiration's" or "as in the inspiration", which means *Forged Alliance* as FAF plays it, so
that what came from elsewhere is still marked where it is used. Nothing else changed: no value
moved, and nothing was re-derived. The values still taken are the ones listed under Starting
numbers, which stays until they are re-derived from this game's own targets (BALANCE.md, Order
of work), and it then gives way to a dated note of its own.

## Starting numbers

To save early playtesting, unit and enhancement numbers started from FAF's published balance,
taken from its unit blueprint files in <https://github.com/FAForever/fa>. They change freely with
this game's own testing and are not kept in step with FAF. `data/units.toml` notes where a value
came from, and how it was scaled, next to the value. FAF's map unit is 20 of ours, one terrain
cell, and every range and radius uses that scale; speeds are doubled, for ticks.

Taken so far:

* Missiles and their defenses (§8): stock limits, the nuclear blast's two rings, and the tactical
  missile's build time, scaled to our launcher's cost.
* The commander's field enhancements (§2.1), and the field rig, the rig yard, and the field rig's
  enhancements (§2.2), from FAF's UEF units, with the field rig's Orbital Redeploy and Rapid
  Fabricator from its Aeon one.
* How one enhancement's effects replace another's of the same kind, as FAF's commander scripts do.
* Materiel extractor tiers, materiel fabricators, and the geothermal plant (§3), from FAF's UEF
  units.
* The air factories, aircraft, and anti-air (§10), from FAF's UEF units. How far a bomb is let go
  short of its target is worked out here from its fall.
* The rest of the UEF's non-experimental units and structures: the Mobile Shield Generator, Combat Engineer, Heavy and Siege Walkers, Torpedo Boat,
  Heavy Point Defense, Heavy Shield Generator, Wall Section, and Field Workshops. The workshops'
  drones are folded into the workshop's own build power. The workshops left the roster on
  2026-11-27, the design owner's pick; their data stays as a test fixture.
* The three air transports and their room by size class, from FAF's UEF units.
* Veterancy, wreckage, turn rates, firing randomness, and missile speeds (§6, §11, §18.4), from
  FAF's rules and blueprints.

Not taken: the Assault Hovercraft, Grenadier Walker, and Escort Corvette were given new concepts and
this game's own numbers on 2026-11-15, in place of the hover tank, gatling bot, and shield boat they
replaced.

Re-derived from this game's own targets and duels on 2026-11-15 (BALANCE.md,
`reports/balance/2026-11-15-t1-land.md`), no longer FAF's: the Medium Tank, Light Walker, Light
Artillery, and Land Scout. And on 2026-11-20 (`reports/balance/2026-11-20-t2-land.md`): the Heavy
Tank, Grenadier Walker, Assault Hovercraft, and Mobile Missile Launcher. And on 2026-11-21
(`reports/balance/2026-11-21-t3-land.md`): the Heavy Walker, Siege Walker, and Mobile Heavy
Artillery. And on 2026-11-24 (`reports/balance/2026-11-24-t1-navy.md`): the Frigate, Attack
Submarine, and T1 Torpedo Launcher, each from FAF's ratios against its own T1 tank (the medians
across its factions, below) applied to this game's Medium Tank, not from FAF's values. And the same
day (`reports/balance/2026-11-24-t2-air-navy.md`), the same way: the Destroyer, Cruiser, Torpedo
Boat, T2 Torpedo Launcher, Gunship, Torpedo Bomber, and Fighter-Bomber, with this game's own
changes past those ratios for its fights between domains. And the Battleship, Battlecruiser, and
Heavy Gunship (`reports/balance/2026-11-24-t3-air-navy.md`), keeping this game's own damage for
the battleship's turrets and the battlecruiser's lasers.

Written down on 2026-11-27 as this game's own, at the numbers they had (`[constants]` in
`data/balance_targets.toml`, `tests/constants_test.rs`): veterancy and wreckage, and the speeds of
ship guns, torpedoes, tactical and strategic missiles, and fighter guns, and the radius the
fighters turn on, each now a rule every unit of its family follows. The design owner keeps them,
with each hull's turn rate and each weapon's spread, as their own; none is read from FAF again.

## Balance targets

`data/balance_targets.toml` (BALANCE.md) is this game's own design intent. Its feel was set on
2026-11-15 with FAF's ratios for a few UEF land units as a reference: time to kill, closing time
over time to kill, sight over range, shell flight time, tier efficiency, and role spread, read
from their blueprints. The targets are chosen deliberately apart from them in places (fights a
little longer, range worth a little less, units cheaper for their income), and no unit's values
were taken from them.

On 2026-11-24, for balance between domains (`[domains]`), FAF's public unit blueprints were read
again: the `FAForever/fa` repository on GitHub, `units/<id>/<id>_unit.bp`, for one unit per
faction (UEF, Cybran, Aeon, Seraphim) in each land, air, naval, and defense role, and the UEF
factories. A script kept outside this repository read their costs, health, shields, weapons, and
ranges, and took the median across factions of each role's ratio to that faction's T1 line unit,
and a few ratios between roles (`reports/balance/2026-11-24-inspiration-ratios.md`). Only those
ratios are recorded, in `[domains] strength_over_anchor`, as a reference for feel; no file, value,
or unit's numbers were taken.

## Names

Replaced on 2026-11-15, in display names, ids, code, tests, and documentation:

| Was (from *Forged Alliance*) | Now |
| --- | --- |
| Armored Command Unit (ACU) | Command Rig |
| Overcharge | Discharge |
| Support Commander (sACU) | Field Rig |
| Quantum Gateway | Rig Yard |
| Quantum teleportation, Personal Teleporter | Orbital Redeploy |
| Hydrocarbon Power Plant | Geothermal Plant |
| Omni Sensor, omni | Deep Scan Array, deep scan |
| Engineering Station | Field Workshop (out of the roster since 2026-11-27) |
| Air Staging Facility | Airfield |
| Anti-Nuke Interceptor | Strategic Interceptor |
| Amphibious Tank | Assault Hovercraft |
| Gatling Bot | Grenadier Walker |
| Shield Boat | Escort Corvette |
| Light, Heavy, and Armored Assault Bots | Light, Heavy, and Siege Walkers |
| Field Engineer | Combat Engineer |
| Mass | Materiel, in everything players read |
| Right arm, left arm, back | Weapons, fabrication, and systems bays |
| T2 and T3 Engineering Suites | T2 and T3 Fabrication Packs |
| Nano-Repair System | Damage Control, moved to the systems bay |
| Resource Allocation System | Onboard Reactor |
| Personal Stealth, Personal Cloak | Signature Suppression, Thermal Camouflage |
| Personal Shield, Shield Generator Field | Field Shield, Area Shield |
| Heavy Plasma Refractor, Energy Accelerator | Long Barrel, Capacitor Bank |
| Enhanced Sensor System | Sensor Mast, moved to the systems bay |
| Smart Charge, Phalanx | anti-torpedo charge, close-in gun |
| UEF, Cybran Nation, Aeon Illuminate, Seraphim, and their experimentals | Frontier Garrison Corps (Charter and Accord); others later |

Kept as ordinary words of the genre or of the military: Rapid Fabricator, Radar Jammer, Tactical
and Strategic Missile Launcher and Defense, Mobile Shield Generator, and the tank, artillery, ship,
and aircraft classes.

## Fonts

The bold hotkeys in the interface use Ubuntu Bold (`assets/fonts/Ubuntu-Bold.ttf`), by Canonical,
the bold weight of the font egui ships with, under the Ubuntu Font Licence 1.0
(`assets/fonts/UFL.txt`), which allows it to be shipped with the game.

## Maps from other games

None. Until 2026-11-15, `src/map_import.rs` read maps made for *Forged Alliance* from a folder the
player gave, and `src/bin/map-download.rs` fetched two community maps from FAF's content server
into the player's cache. Neither shipped or committed any map, and both were removed so that the
game depends on nothing of the original's.

## Bundled maps

`highland` and `twin_passes` (`src/sim/maps.rs`) are this game's own, generated by its own code
from fixed seeds. Their ideas come from community maps for *Forged Alliance*, Hilly Plateau's
rolling plateau and rock fields and Dual Gap's two crossings, but nothing of those maps' data is
used.

## Formulas

* Orbital Redeploy's cost and charge time (`src/sim/enhancements.rs`): a flat cost plus half the
  distance to the power 2.1, and a delay plus a hundred-thousandth of it, each capped. The formula
  comes from FAF's `lua/shared/teleport.lua`, Copyright (c) 2022 clyf, under the MIT license. No
  code was copied: the formula is worked out here in whole numbers.
