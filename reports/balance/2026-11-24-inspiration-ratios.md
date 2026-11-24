# The inspiration's ratios between domains, 2026-11-24

How the inspiration balances land, air, and navy against each other, read as ratios for feel,
never as values (BALANCE.md; PROVENANCE.md records the source). A script kept outside the
repository read the inspiration's public unit blueprints for each role below, one unit per faction
where a faction has one, and took the median across factions so that no one faction's quirks set
a ratio. Beside each is ours, computed the same way from `data/units.toml`. No number in the unit
data comes from here; these ratios feed `[domains]` in `data/balance_targets.toml`.

## How the ratios are measured

* **Strength per cost:** √(damage a second × health) over cost, by Lanchester's square law, our cost
  model's measure (`cost_at`: mass and energy, energy at the tier's rate of `[economy]`). Damage is
  what the role is for: anti-air for fighters and mobile anti-air, torpedoes for submarines,
  torpedo boats, and torpedo bombers, and guns and missiles for the rest. Each is a multiple of the
  same faction's T1 line unit, the anchor's role.
* **Damage a second** is each weapon's damage times its salvo and rate of fire, beams by their
  pulses; countermeasures (missile and torpedo defenses) and death weapons are left out.
* Static ratios leave out what only a fight shows: shields, torpedo defense, homing, splash, range,
  and passes. The duels (`2026-11-24-domains.md`) measure those.

## Strength per cost, over the T1 line unit's

| Role | Inspiration (median) | Its factions' range | Ours |
|---|--:|--:|--:|
| T2 line unit | 0.96 | 0.63–0.98 | 1.02 |
| T3 line unit | 1.01 | 0.76–1.08 | 1.00 |
| T1 mobile anti-air | 1.17 | 1.01–1.50 | 0.68 |
| T2 mobile flak | 1.56 | 1.53–2.12 | 0.85 |
| T2 hovercraft | 0.71 | 0.57–0.84 | 0.88 |
| T1 frigate | 0.57 | 0.50–0.68 | 0.28 |
| T1 submarine | 0.24 | 0.21–0.33 | 0.13 |
| T2 destroyer | 0.33 | 0.29–0.48 | 0.23 |
| T2 cruiser | 0.20 | 0.15–0.25 | 0.11 |
| T2 torpedo boat | 0.32 | 0.23–0.37 | 0.21 |
| T3 battleship | 0.40 | 0.30–0.62 | 0.17 |
| T3 battlecruiser | 0.25 | one faction | 0.19 |
| T1 interceptor | 0.31 | 0.30–0.42 | 0.17 |
| T1 bomber | 0.28 | 0.23–0.32 | 0.29 |
| T2 gunship | 0.62 | 0.47–0.68 | 0.31 |
| T2 torpedo bomber | 0.55 | 0.48–0.67 | 0.27 |
| T2 fighter-bomber | 0.45 | 0.00–0.53 | 0.34 |
| T3 air superiority fighter | 0.76 | 0.75–1.04 | 0.58 |
| T3 strategic bomber | 0.23 | 0.22–0.32 | 0.23 |
| T3 heavy gunship | 0.38 | 0.37–0.38 | 0.21 |
| T1 point defense | 0.93 | 0.92–1.28 | 0.51 |
| T2 point defense | 0.61 | 0.49–0.86 | 0.34 |
| T1 torpedo launcher | 0.45 | 0.43–0.58 | 0.24 |
| T2 torpedo launcher | 0.64 | 0.54–0.84 | 0.34 |

What it shows:

* **Land matches.** Our T2 and T3 line units are as strong per cost against T1 as the
  inspiration's, which is what `[tiers]` asked for.
* **Everything not yet derived is about half as strong per cost as the inspiration has it,**
  against the land: every naval role (0.42 to 0.76 of its ratio), the T2 and T3 strike aircraft,
  and the static defenses. Those units still carry first-draft prices from before 2026-11-15, when
  land units were made cheaper for their income for bigger armies (`[decisions]`); nothing has
  re-priced them to match. So navy, and T2 air, are poor buys next to land, whatever they do to
  each other.
* **Navy against air keeps the inspiration's relation,** since both were halved together (below).
* The T1 bomber, the strategic bomber, and the T3 fighter, which have rules of their own (`[air]`,
  `[kills]`, `[air.t3_fighter]`), sit at or near the inspiration's ratios. Our mobile anti-air is
  lower by design: `[air] anti_air_escort_share` sets it so that an army a fifth anti-air comes out
  even with bombers.

## A T2 fleet against torpedo bombers

A fleet of two destroyers, two cruisers, and a shield boat where the faction has one, against
torpedo bombers, by the square law, with only hulls counting, since torpedoes run under shields:

| | Inspiration (median) | Its factions | Ours |
|---|--:|--:|--:|
| Torpedo bombers' cost to come out even, as a multiple of the fleet's | 0.67 | 0.58–0.86 | 0.67 |
| The same against two destroyers alone | 0.06 | | 0.18 |

By its own numbers, the inspiration's torpedo bombers are an efficient answer to a fleet even with
cruisers: about two thirds of the fleet's cost breaks it, before torpedo defense, which shoots
torpedoes down and which these figures leave out. Our duels give the same, 0.60 (see
`2026-11-24-domains.md`).

## Anti-air damage a second per cost, over the T2 mobile flak's

| Ship | Inspiration | Ours |
|---|--:|--:|
| T2 cruiser | 0.30 | 0.29 |
| T2 destroyer | 0.00 | 0.01 |
| T1 frigate | 0.02 | 0.03 |
| T3 battleship | 0.00 | 0.01 |

The cruiser is the fleet's anti-air, as in the inspiration, and at the same rate.

## Reach: a ship's guns over the same tier's point defense

| Ship against | Inspiration | Ours |
|---|--:|--:|
| T2 destroyer, T2 point defense | 1.35 | 1.20 |
| T2 cruiser, T2 point defense | 2.30 | 3.00 |
| T3 battleship, T3 point defense | 1.99 | 2.14 |
| T3 battleship, T2 point defense | 2.78 | 3.00 |
| T1 frigate, T1 point defense | 1.08 | 1.08 |

Ships from T2 up outrange the coastal defenses of their tier in both, as `[domains]` asks.

## The shield boat

Its shield's health per cost, over the T2 destroyer's hull health per cost: 1.85 in the
inspiration, 1.73 ours.

## Factories

Air and naval factories against the land factory of the same tier, cost and build power, are the
same as the inspiration's in every row, as first-draft values: T1 air 1.05 the cost, naval 0.91; T2
headquarters, air 0.72 and naval 1.38 the cost, naval 2.25 the build power; T3 headquarters, air
0.89 and naval 1.75 the cost, air 1.33 and naval 1.67 the build power.
