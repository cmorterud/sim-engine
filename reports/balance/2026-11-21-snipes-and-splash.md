# Snipes and heavy splash, 2026-11-21

To play as the inspiration does. Two kinds of rule, both in data/balance_targets.toml, both checked by tests/kills_test.rs against the unit data.

## Splash in three sizes (`[splash]`)

A blast's reach is its splash plus its target's radius, in formation spacings (2.5 radii) of the line unit of the weapon's own tier: radius 9 at T1, 11 at T2, 13 at T3. That says how many units in a tight formation one hit catches.

| Size | Reach, in spacings | Units caught | Weapons | Splash |
|---|---|---|---|---|
| Narrow | 1.0-1.2 | about 5 | T1 Light Artillery, Grenadier Walker, the Discharge | 16, 19.25 |
| Medium | 1.45-1.95 | about 9 | T1 Attack Bomber, Fighter-Bomber | 29 (was 60), 36 (was 60) |
| Wide | 2.85-3.15 | about 29 | T2 and T3 Artillery Installations, Mobile Heavy Artillery, Strategic Bomber | 70 (was 60), 83 (was 120), 83 (was 22.75), 83 (was 110) |

One size for every splash weapon, as first written, had narrowed the heavy ones until they barely felt heavy.

## Hits to kill (`[kills]`)

Exact counts, at base health, with no veterancy or enhancements; a personal shield counts with the health it guards.

| Weapon | Damage a hit | Target | Toughness | Hits |
|---|---|---|---|---|
| Strategic Bomber | 5,200 (was 3,100) | Heavy Walker | 4,292 + 700 shield | 1 |
| | | Siege Walker | 11,010 | 3 |
| | | Mobile Heavy Artillery | 1,300 | 1 |
| | | T2 Artillery Installation | 7,000 (was 3,600) | 2 |
| | | Commander | 18,000 (was 12,000) | 4 |
| Tactical Missile Launcher | 10,000 (was 1,500) | Field Rig | 16,000 | 2 |
| | | Commander | 18,000 | 2 |
| Mobile Heavy Artillery | 859 | Mobile Heavy Artillery | 1,300 | 2 |
| | | Medium Tank | 420 | 1 |

The T3 Artillery Installation's shell, 11,500 (was 7,840), destroys every T3 land unit with a direct hit, the Field Rig aside, but scatters up to 175 (was 9.3), so it is for bases; the T2 one keeps its spread of 50, for small bases.

The commander is tough against everything else and falls to the snipe weapons, which fall in turn to T3 fighters, SAMs, and missile defense. A Heavy Walker in one bomb and a commander in exactly four could not both hold at 12,000 health, so the commander went to 18,000 rather than the walker to two bombs. Its reactor's blast went from 15,000 to 18,000 with it, so that a commander dying beside another still takes it along, a draw, as designed.

## Mobile Heavy Artillery, a glass cannon

With its blast wide, the walk-in duel that priced it could not: at the lowest strength the search allows, 0.125, it still wiped out Heavy Walkers walking into it. Made a glass cannon by a short time to kill, it died to one shell from its own kind. So it is set by hits instead: it survives one shell from another of its kind but not two, and its shell keeps the 859 the arcs pass gave it, which kills a Medium Tank. Health 1,300 (was 2,305), spread 60 (was 25), inaccurate as heavy artillery is in the inspiration.

Against it, at equal materiel (3,600), mirrored:

| Against it | Score for them |
|---|---|
| Medium Tanks, attack-moving | -0.67 |
| Medium Tanks, spread out and charging | +0.72 |
| Light Walkers, attack-moving | +0.65 |
| Light Walkers, with micro | +0.87 |
| Heavy Tanks, spread out and charging | +0.25 |
| Heavy Walkers, attack-moving | -1.00 |
| Heavy Walkers, spread out and charging | -0.28 |
| Siege Walkers, attack-moving | -0.85 |
| Mobile Heavy Artillery | 0.00 |

It wrecks armies that walk in bunched up, and big T3 units cannot spread far enough to escape its blast; spread-out smaller units, raiders, and air are its answers. Sixteen of it break the shielded T3 position in 21 seconds, losing nothing. To watch in the soak and the playtest scenarios: T3 armies have no ground answer to it but closing in.

## T3 report

```
T3 land from data/balance_targets.toml, against T1 and T2 land frozen:
  line       strength 1.00  Heavy Walker: 360 materiel, 4292 health, 235.7 damage a second, range 525, speed 6.1, sight 580; time to kill a copy 18.2 s
             mass = 360, energy = 1800, build_time = 2160, health = 4292, speed = "6.1", radius = 13, vision = 580
             weapon = { range = 525, reload_ticks = 3, projectile_speed = 95, damage = 70.7, splash_radius = 0, spread = 0 }
  heavy      strength 0.98  Siege Walker: 720 materiel, 11010 health, 357.5 damage a second, range 578, speed 4.3, sight 520; time to kill a copy 30.8 s
             mass = 720, energy = 3600, build_time = 4320, health = 11010, speed = "4.3", radius = 15, vision = 520
             weapon = { range = 578, reload_ticks = 40, projectile_speed = 96, damage = 1429.9, splash_radius = 12, spread = 0 }

Counter-matrix cells, equal materiel (3600), mirrored, score for the first:
  T3 line against T2 line, efficiency: 1.07 (targets: 1.0-1.15)
  T3 line against T2 line, score:      +0.23
  heavy against line:                 -0.06 (even, within 0.15)
  artillery's reach over the heavy point defense's: 1.20 (at least 1.15)
  16 artillery against a heavy shield over three heavy point defenses (9300 materiel): broken in 21 s, losing 0% (targets: within 60 s, nothing lost)
  line against artillery, attack-move: -1.00 (artillery wins)
  line against artillery, with micro:  -0.28 (line wins)
  heavy against artillery, attack-move: -0.85
  T2 assault against T3 artillery:     -1.00
  a raid, 25 T1 raiders against 1 artillery: -0.69 (raiders win)
  ten T3 line against ten:            44.2 s (targets: 35-60 s)

Strength per materiel against T2 and T1 land (above 1, the T3 unit is the better buy):
  Heavy Walker             Heavy Tank 1.07, Grenadier Walker 1.00, Assault Hovercraft 1.20, Mobile Missile Launcher 1.46, Medium Tank 1.13, Light Artillery 1.23
  Siege Walker             Heavy Tank 1.67, Grenadier Walker 2.00, Assault Hovercraft 1.37, Mobile Missile Launcher 1.75, Medium Tank 2.13, Light Artillery 3.17
```
