# The ability price list (step 3, 2026-11-26)

Step 3 of the derived-stats model (reports/plans/2026-11-26-derived-stats.md): a unit's cost from
what it can do. The constants are in `[price_list]` in data/balance_targets.toml. This first pass
sets navy costs; land comes out within about 10% of today's costs, which the land solver's targets
already pin, and air keeps its costs until its missing prices are found.

## The formula

    cost = Σ over jobs √(health × damage a second at the job) / σ_job
    σ_job = 2.806 × tier efficiency (1.0) × domain base / (1 + Σ prices)

A job is a kind of target: surface, air or submerged. Separate weapons firing at once each do their
own job (the destroyer's guns and its torpedo). One weapon that can hit several kinds of target
counts once, at its best job, plus a flexibility price.

Cost is in materiel, mass plus energy over the economy's rate, so a land unit's cost is its mass.
Mass, energy and build time scale together when a cost moves.

## Prices

| Ability | Price |
| --- | --- |
| Reach past the line's | x·h / (1 − x·h), x = c·(r / r_line − 1); h ≈ 1 for shells, 0 for missiles |
| Reach short of the line's | −c·(1 − r / r_line)·(v_line / v) |
| Speed | 0.3·(1 − v_line / v) |
| Splash | √n − 1, n measured on a standing or marching formation |
| Undodgeable (lasers) | √(1 / (1 − d)) − 1 = 0.34, d = 0.44 dodged by the T3 fleet |
| Submerged, out of cannons' reach | √(1 / (1 − f)) − 1 = 0.65 at T1, f = 0.63 |
| More than one kind of target | √(1 / largest single share) − 1 = 0.41 for a torpedo (navy half ships, half subs) |
| Torpedo decoys (sub hunter) | as undodgeable, 33% of a sub group's damage stopped: 0.22 |
| Sensors | (range² − vision²) / building range² × building cost |

The domain base for the navy, 0.648, holds the T2 fleet (two destroyers, two cruisers) at its cost.
A ship's radar is shared by its tier's standard fleet's radar ships, whose cover overlaps, so it
pays the area price over their number (2026-11-26): the Frigate's 65 falls to 22, the Cruiser's
about 260 to 70.

## Measured

- Splash: Grenadier and Light Artillery 1.5×, Mobile Heavy Artillery about 6.5×; bombers on their
  tier's line formation 1.88 (T1), 2.18 (Fighter-bomber), 3.03 (Strategic).
- Dodging: against the T3 fleet turning back 0.3 s after each shell leaves, shells at 60 lose 44%
  of their damage, cost-weighted (battleship 16%, battlecruiser 35%, cruiser 42%, destroyer 79%).
- Decoys: the Torpedo Boat's stop 33% of three attack subs' damage, 9% of two destroyers' and 4% of
  a mixed T2 fleet's. The destroyer's stop 7% of six subs'.
- Duels at today's stats: one Torpedo Boat beats two subs keeping 86%, three keeping 51%; it loses
  to three frigates, and two lose to a destroyer. A hard counter.

## What changes

| Unit | Cost before | After | |
| --- | --- | --- | --- |
| Frigate | 304 | 398 | +31% |
| Attack Submarine | 345 | 411 | +19% |
| Destroyer | 1,684 | 1,625 | −3% |
| Cruiser | 1,529 | 1,588 | +4% |
| Battleship | 6,920 | 6,298 | −9% |
| Battlecruiser | 5,302 | 5,097 | −4% |

(First pass, PR #44: sub 386, destroyer 1,530, cruiser 1,684, battleship 5,905, battlecruiser
4,785. The radar overlap rule moved the navy base from 0.691 to 0.648.)

The Frigate is set by the design owner's rule against T1 bombers (2026-11-26), in place of an
equal-cost fight that held its cost: its anti-air destroys one in about 21 seconds (23.8 a second,
from 27), and six sink it in two passes and lose none, which its health of 2,949 meets (between
six bombs, 2,460, and twelve). Its cost is then the list's.

Other stats are unchanged. The Torpedo Boat's decoys fire every 3.3 s at 600 (were 2.5 s at 500): three
times the destroyer's, as its torpedoes do three times the destroyer's damage a second.

## Held, with reasons

- **Torpedo Boat, 819** (list: 676 after the radar rule, 634 before). The list averages a unit over an enemy mix, which undervalues a
  hard counter. A new cell holds it instead: Torpedo Boats beat an equal cost of submarines
  (`torpedo_boats_beat_submarines`). Pricing counters by the duel they win is a follow-up.
- **Light Artillery (+23%), Mobile Missile Launcher (+29%)**: counters held by the land solver's
  targets; the list charges reach in every matchup, which pays only against the line.
- **Land**: within about 10% (Light Walker −10%, scout +5%, Siege Walker −9%), costs the land
  solver's targets pin. The list is a check on them here.
- **Air**: swings of −63% to +78% mean flight itself (only anti-air reaches a plane) and air-to-air
  fights are unpriced. Air keeps its costs until its own pass.
