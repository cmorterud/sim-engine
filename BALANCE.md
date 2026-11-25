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
  artillery. Shells are fast, and every shell leads its target, so dodging takes deliberate
  changes of direction. Scripted micro is worth 10–25% effective strength, except where a
  matchup is meant to turn on it: massed line units against massed artillery lose attack-moving
  and win spread out and charging, while raiders, artillery, mobile shields, and bombers beat
  artillery with no micro at all.
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
   data or the goldens until a result is kept. Artillery is also checked in a siege, attack-moving
   onto a defended position of finished, powered structures (`[siege]`): what a field duel cannot
   measure, since a position it outranges cannot fight back.
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

## Status

* 2026-11-15: T1 land (the Medium Tank, Light Walker, Light Artillery, and Land Scout) derived and
  priced, and in the unit data (`reports/balance/2026-11-15-t1-land.md`). Every counter-matrix
  cell among them meets its target.
* 2026-11-20: T2 land (the Heavy Tank, Grenadier Walker, Assault Hovercraft, and Mobile Missile
  Launcher) derived against T1 land frozen, priced, and in the unit data
  (`reports/balance/2026-11-20-t2-land.md`). The Heavy Tank is 1.07 times the anchor per
  materiel; the grenadier beats it attack-moving and loses to it with micro; the hovercraft is
  about even with it; the missile launcher beats artillery firing in place and loses to what keeps
  moving. Left for later passes: the Mobile Flak (air), the Mobile Shield (mixed armies), and the
  T2 engineers (economy).
* 2026-11-21: T3 land (the Heavy Walker, Siege Walker, and Mobile Heavy Artillery) derived
  against T1 and T2 land frozen, priced, and in the unit data
  (`reports/balance/2026-11-21-t3-land.md`). The Heavy Walker is 1.07 times the Heavy Tank per
  materiel; the Siege Walker is about even with it; the artillery outranges the T3 heavy point
  defense by a fifth, breaks a shielded position of it in 27 seconds without loss (`[siege]`),
  beats the Heavy Walker attack-moving, and loses to it with micro, and to what closes in. Left
  for later passes: the Field Rig and the T3 Engineer (economy).
* 2026-11-21: T3 artillery sets up for a second before it fires, and moving packs it up, so that
  it shells positions rather than armies in the field. Re-priced against the same targets, it
  came out 2% weaker, within a shot-to-kill step: 3,038 health and 1,132 a shell. It breaks the
  shielded position in 23 seconds without loss; against Medium Tanks that close on it, its
  strength per materiel fell from 0.64 to 0.27.
* 2026-11-21: artillery lobs its shells, which come down where they were aimed
  (`reports/balance/2026-11-21-artillery-arcs.md`). Shells that land made artillery stronger:
  raiders charging it from the front now lose, so they are no longer one of its counters without
  micro, nor a micro cell. T3 artillery re-priced to 2,305 health and 859 a shell. The Mobile
  Missile Launcher is now priced at 0.8 of T1 artillery's strength per materiel (`[missiles]`)
  rather than by a duel against it, which arcs had moved by half: 1,353 health, 420 a missile.
  T1 came out unchanged.
* 2026-11-21: snipes and heavy splash, to play as the inspiration does
  (`reports/balance/2026-11-21-snipes-and-splash.md`). Splash in three sizes, narrow, medium, and
  wide (`[splash]`); the snipe weapons set by exact hits to kill (`[kills]`), with the commander
  up to 18,000 health. Mobile Heavy Artillery, wide and a glass cannon, is set by hits, not by the
  walk-in duel, which a wide blast broke: 1,300 health, 859 a shell, a spread of 60.
* 2026-11-24: big guns set against each other by damage a second: each of the battleship's three
  turrets does twice a Mobile Heavy Artillery's and one and a half times a T2 Artillery
  Installation's, and each of the battlecruiser's two lasers twice a battleship turret's. The
  mobile artillery was slowed to fit, from a shell every 6 seconds to one every 11.5, at its old
  cost. It no longer wins a walk-in fight against bunched Medium Tanks of its cost (-0.49), and
  that target is dropped: it is a siege piece that loses to what reaches it. It still wrecks
  Heavy Walkers, Heavy Tanks, and Assault Hovercraft that attack-move into it, and breaks the T3
  position at equal cost in 21 seconds. (Its cost was halved for a day to keep the old target.)
* 2026-11-22: T1 air (the Attack Bomber, Interceptor, and Mobile Anti-Air) set by rules against
  T1 land frozen (`reports/balance/2026-11-22-t1-air.md`), each rule one number: a tank and a
  Mobile Anti-Air take two bombs; one interceptor downs a bomber in about 10 seconds; an army a
  fifth anti-air comes out even with bombers; five bombers destroy a commander in 79 seconds.
  Anti-air is a hard counter. Fighters turn half again as fast as bombers, interceptors fire only
  ahead and tail what they chase, flying heights differ, and anti-air guns miss fast, high
  aircraft.
* 2026-11-22: energy is valued by the tier a fight is at (`[economy] energy_per_materiel`): 5 to
  a materiel at T1, and 100 from T2, the fabricators' rate. At T1's rate everywhere, the T3
  fighter, whose cost is mostly energy, counted as 21 interceptors rather than 13, and spamming
  interceptors against it looked the better buy, which the inspiration's players never found. At
  T3 prices six T3 fighters beat as much in interceptors by +0.22 (`[air.t3_fighter]`); its turn
  went to 340 degrees a second, a quarter quicker than the interceptor's. Land units and T1 air
  come out the same. Next: the rest of T2 and T3 air.

* 2026-11-24: fights between domains measured for the first time (`reports/balance/
  2026-11-24-domains.md`), against `[domains]`: each domain strongest in its own medium, the map
  deciding how much of each medium there is, and each naval and air role's strength per cost set
  against the inspiration's ratio (`reports/balance/2026-11-24-inspiration-ratios.md`). The duels
  gained a water ground and a shore ground, prices at any tier, and forces of several units;
  soaks gained an account of each side's mass by domain. No unit data changed. Found: every naval
  role, and the T2 and T3 strike aircraft, are about half as strong per cost against the land as
  the inspiration has them, having kept first-draft prices through the land rebaseline; a composed
  T2 fleet loses to an equal cost of the AI's air force (-0.57), and torpedo bombers costing 0.60
  times it break it; submarines beat destroyers, and hovercraft beat a fleet on the water. Decided: a
  massed mixed fleet is all but impenetrable from the air at equal resources, so torpedo bombers
  need at least twice a T2 fleet's cost and three times a massed T3 fleet's to break it, where today
  they need 0.60 and 0.18; the inspiration's static numbers (0.6 to 0.7) leave out torpedo defense,
  anti-air splash, and shields.
* 2026-11-24: T1 navy (the Frigate, Attack Submarine, and T1 Torpedo Launcher) set against T1 land
  and air frozen (`reports/balance/2026-11-24-t1-navy.md`): each given the inspiration's cost,
  health, damage, and build time per mass against its own T1 tank, applied to the Medium Tank,
  which brought each from about half of its strength-per-cost ratio to within the band. Past the
  ratios, within the band: the frigate's railgun at half again, so that frigates hold even with an
  equal cost of T1 bombers, and the launcher's health at 3,000, so that frigates closing on it lose.
* 2026-11-24: T2 air and navy against T1 frozen (`reports/balance/2026-11-24-t2-air-navy.md`): the
  Destroyer, Cruiser, Torpedo Boat, T2 Torpedo Launcher, Gunship, Torpedo Bomber, and Fighter-Bomber
  by the inspiration's ratios against its T1 tank, then what keeps a fleet alive: the cruiser's SAMs
  burst over massed aircraft (a blast of 80, from 6), the corvette's and destroyer's anti-torpedo
  charges keep up, the destroyer hunts submarines, and the T2 launcher fires its four torpedoes
  each on its own. A composed T2 fleet now beats an equal cost of the AI's air force (+0.65, from
  -0.62), torpedo bombers need 2.45 times its cost to break it (from 0.65), destroyers beat
  submarines, and hovercraft lose to it. The Mobile Flak is set by the anti-air share rule at T2.
* 2026-11-24: T3 air and navy (`reports/balance/2026-11-24-t3-air-navy.md`): the Battleship,
  Battlecruiser, and Heavy Gunship by the inspiration's ratios, the battleship's turrets and the
  battlecruiser's lasers keeping their decided damage. A massed T3 fleet is kept from the air by
  its escorts, as in the inspiration, whose battleships have next to no anti-air and no
  anti-torpedo charge: escorts seven-tenths of its cost, it beats an equal cost of the T3 air force
  (+0.95) and needs 2.04 times its cost in torpedo bombers to break, while a battleship-heavy fleet
  breaks at 0.34. The AI's T3 naval factories build that escorted mix. Coastal sieges are judged
  at an equal cost. Every `[domains]` cell is met.
* 2026-11-24: aircraft obey orders over open sea (same report). On the harness's sea, with no land
  anywhere, aircraft had dropped their orders and circled, so every fleet-against-air fight since
  the first `[domains]` report was measured with idle aircraft, which favored ships. Measured again:
  the frigate's railgun went from 20.8 to 27 to hold its own against T1 bombers, the massed T3
  fleet took more escorts (12 cruisers, 4 destroyers, 4 corvettes), and torpedo bombers break a T2
  fleet at 2.03 times its cost, not 2.45. The T1 and T2 navy reports' fights over open sea predate
  this.
* 2026-11-25: what a unit can do that its counterpart cannot, it pays for in strength per cost
  (`[premiums]`, the user's rule, everywhere). Immobile defenses are worth more: three point
  defenses of a tier hold against half again their cost in its line unit (T1 health and damage
  three times, T3 damage two and a half times; T2 already held), and static anti-air is half again
  as strong per cost against aircraft as mobile (mostly in health, so that fast T3 aircraft still
  pass over T1 and T2 anti-air, as `[kills]` has it). Amphibious units are worth less: the
  hovercraft is three-quarters as strong per materiel as the Heavy Tank on land, from about even
  (health and damage x0.81). The hovercraft was tested first as the cause of Naval losing every
  match to Balanced on the Islands maps; it was not, and Balanced still won every one.

## Order of work

1. Targets (this draft), cost model, and archetype templates; first-pass T1 land numbers.
2. The duel harness, the break-even solver, and the strength fit; tune T1 land.
3. T2, then T3, air, and naval, each against the frozen tiers below it. Navy and the T2 and T3
   strike aircraft go by `[domains]` (2026-11-24): T1 navy; then T2 air and navy; then T3; then,
   only if whole matches still favor a domain, factory costs and build rates.
4. AI strategy variants and the global knobs.
5. Name no other game in comments, write PROVENANCE.md's dated note, and turn on the CI gates.
   Done on 2026-11-24 for the names: comments say "the inspiration's" next to a borrowed value,
   PROVENANCE.md keeps the list, and the CI name check covers the abbreviation too. Left for after
   step 4: re-deriving the values still borrowed (PROVENANCE.md, Starting numbers) and the
   Guardrails' CI checks.

The game stays playable throughout: only one tier is in flux at a time.
