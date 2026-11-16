# Balance

How this game's numbers are made: from its own design intent, a cost model, and measured tuning,
never seeded from another game's values (GAME_SPECIFICATION.md §20, PROVENANCE.md). The intent is
in [`data/balance_targets.toml`](data/balance_targets.toml). Until the rebaseline is done, most of
today's values in `data/units.toml` still come from elsewhere, as PROVENANCE.md records.

## Decisions

Made on 2026-11-15, with the inspiration's ratios as a reference for feel (PROVENANCE.md):

* **Pace:** about as fast as the game this one succeeds, with fights a little longer: the anchor
  kills a copy of itself in 14 seconds. Matches run about 30 minutes, and the map takes about the
  same time to cross.
* **Micro:** position and composition decide fights. Micro can win an edge in a pinch, such as
  against a larger force, but is never needed to play well. Range is worth a touch less than in
  the inspiration (closing time 0.35 of the time to kill), so kiting belongs to skirmishers and
  artillery. Shells are fast, and direct-fire weapons lead their targets, so dodging takes
  deliberate changes of direction. Scripted micro is worth 10–25% effective strength.
* **Sight** is about the inspiration's: scouts see far, line units a little past their guns,
  heavy units and artillery less than their range.
* **Tiers** are about as efficient per materiel as each other, so lower tiers stay useful, and
  higher tiers win by range, toughness, and concentration.
* **Roles** spread as widely as the inspiration's, as multiples of the anchor's time to kill:
  raiders 0.3–0.5, heavy units 1.6–2.2.
* **Splash** is incidental on line weapons and about twice formation spacing on splash roles.
* **Armies** are bigger for the same income than in the inspiration, at the same pace: units are
  cheaper for their income, and tech costs scale with income.
* **Scale:** maps, engine units, sight, and ranges stay as they are.

## The method

The idea is to have few free numbers and to solve for them, not to search for them.

1. **Targets.** A few dozen numbers of design intent (`balance_targets.toml`): one anchor unit,
   the T1 Medium Tank; fight lengths; tech timings; how much stronger and costlier each tier is;
   how strongly counters win; and how much micro is worth.
2. **Cost model, before any simulation.** By Lanchester's square law, equal strength per materiel
   means cost in proportion to √(effective damage a second × effective health), with factors for
   range (free shots while the enemy closes), speed, splash against clumps, hit rate against
   moving targets, and utility. Each unit is an archetype from the role catalogue with one or two
   identity knobs (a raider: 1.6 times the anchor's speed, 0.6 times its health); the model fills
   in the rest. The first pass is consistent by construction, and price is what is left to tune.
3. **Duel harness.** Headless, deterministic, equal-materiel duels between every pair, in a few
   fixed scenarios: open field, closing from range, clumped, and spread out. Each scores the
   signed share of materiel left, from −1 to 1, rather than a win, so a few runs suffice. For each
   pair it finds the cost ratio that comes out even by scaling one side's health smoothly, since
   whole unit counts make results jump. Fitting that matrix gives each unit an overall strength
   and its matchups against the counter matrix (beats, loses to, depends on micro); a cell off the
   matrix is a bug. Blueprints are overridden in memory, so tuning never touches the committed
   data or the goldens until a result is kept.
4. **Solving.** Prices first: multiply each unit's cost by exp(−α · strength) and measure again,
   which settles in a few rounds, since outcomes are near-linear in log price. Then counters, by
   the stat that makes them (flak damage against aircraft, artillery's least range), not price.
   One tier at a time: T1 land, then freeze it; T2 against the frozen T1; then T3, air, naval,
   and experimentals, so that each step is a small problem.
5. **Whole matches, for the few global knobs.** Economy rates, build power, and upgrade costs
   show only in full matches, so AI strategy variants (T2 rush, T1 spam, air-heavy, naval-heavy)
   play soaks on CI, and a sample-efficient search (Bayesian optimization or CMA-ES over fewer
   than ten knobs, not a grid) tunes them to the pace and strategy targets, with mirror fairness
   a hard gate.

## Guardrails

Checked by CI once the harness exists:

* Every unit loses to something cheaper than itself; experimentals lose to an equal-cost answer.
* No unit is beaten by another of its tier on every axis at once.
* Tier efficiency and fight lengths stay in their bands.
* The duel matrix matches the counter matrix, and soaks meet the pace and strategy targets.

## Records

Every value traces to a target, a formula with its parameters, or a dated tuning run under
`reports/balance/`. Once tuning is done, one comparison against the old values reports which came
out close and why: evidence about the process, not a target.

## Order of work

1. Targets (this draft), cost model, and archetype templates; first-pass T1 land numbers.
2. The duel harness, the break-even solver, and the strength fit; tune T1 land.
3. T2, then T3, air, and naval, each against the frozen tiers below it.
4. AI strategy variants and the global knobs.
5. Remove the remaining "FAF's" comments, write PROVENANCE.md's dated note, and turn on the CI
   gates.

The game stays playable throughout: only one tier is in flux at a time.
