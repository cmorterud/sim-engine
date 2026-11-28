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
  Stronger defenses lengthen sieges: a Scattered Isles 4v4 ran to 73 minutes, past the soak's
  67-minute limit, finishing. Team matches of three or more a side now count as unfinished only
  past 100 minutes (the user's decision), and since the fighters were repriced, so do matches on
  maps 20 km across or more: a mirrored Scattered Isles 1v1 ran to 74 minutes, finishing in a draw.
* 2026-11-25: the fighters by the inspiration's ratios (`[domains] strength_over_anchor`): the
  Interceptor and Air Superiority Fighter were set by `[air]` rules alone and sat at about half and
  three-quarters of the inspiration's strength per cost against the anchor. They take its price
  and build time against its T1 tank (and the interceptor its health), keeping the damage those
  rules fix (the bomber's health comes from the interceptor's; the T3 fighter has ten times it and
  kills one with a shot) and the T3 fighter's health, which it needs to beat an equal cost of
  interceptors. A duel of escorted against unescorted strike aircraft was
  not used as the target: strike aircraft cannot shoot at aircraft, so it is won whatever the
  fighters cost.
* 2026-11-25: torpedoes, from playtesting. Every torpedo tops out at 30, two and a half times the
  fastest ship's speed, as the inspiration's top out at two and a half to three times their
  ships'. It leaves at a third of that, about a ship's speed, as the inspiration's leave at about
  their launcher's, and gathers speed over two seconds, as theirs do. (For a day they topped out at
  18 after one second, from comparing our top speed with the inspiration's launch speed; that
  raised the torpedo bombers it takes to break an escorted fleet to three times its cost.) Ships,
  submarines, and launchers fire theirs from just under the water, not from their guns' height;
  torpedoes keep under the water as they home. A torpedo bomber's still homes from the bomber
  straight onto its ship (the user's call: one falling from the bomber's height was carried past
  the ship). Anti-torpedo charges drop a decoy into the water that the torpedo strikes instead of
  destroying it outright, still one torpedo each reload. Every `[domains]` target holds: the
  torpedo bombers it takes to break a fleet are 2.1 times its cost for T2 and 2.2 for T3 (the band
  is 2 to 5), and a battleship-heavy fleet breaks to 0.52 of its cost.
* 2026-11-25: kill times, the user's (`[kill_times]` and `[volleys]`,
  `reports/plans/2026-11-25-weapons-nukes-shields.md`). The cruiser fires its anti-air eight
  missiles a volley and its cruise missiles four, and each aircraft takes a set number of volleys
  (Gunship health 1,590 and Strategic Bomber 6,400 to fit). The T3 SAM destroys a Strategic Bomber
  in about 11 seconds and a Gunship in about 2. The destroyer's guns destroy a commander on land in
  about 80 seconds, and its torpedo a submerged Attack Submarine in about 25 and, five of them, a
  commander under the water in about 80; two torpedo boats destroy one in about 60. The T2 dome
  heals as fast as one cruiser's missiles strike it, and the T3 dome faster than a battleship's
  guns. Two `[domains]` cells moved with them. One ship can no longer break its tier's coastal
  position, by design, so two are measured (`coastal_ship_count`): two cruisers break the T2 one
  in 63 seconds and two battleships the T3 one in 80. And submarines now beat destroyers alone,
  as the user has it: six T1 submarines, about a destroyer's cost, sink one in about 33 seconds,
  losing one and with a second damaged (`six_t1_subs_sink_a_destroyer`). The destroyer's
  anti-torpedo charge is back to the inspiration's one torpedo every 10 seconds, from one a second,
  and the submarine's torpedo is up from 185 to 235, inside its strength band (1.13). The
  `[domains]` cell turned with it: submarines beat an equal cost of destroyers
  (`submarines_beat_destroyers`), where destroyers had beaten submarines.
* 2026-11-25: the battleship and the domes, the user's (`[broadsides]`, `[kill_times]`, and
  `[domains.cells]`). The battleship's three turrets now fire as one broadside
  (`WeaponDef::broadside`): the main gun waits until all three point at their targets, and the
  other two fire once each after it, 0.3 seconds apart. Three shells of 4,400 every 16 seconds,
  from 3,000 each every 20 on their own, nearly doubling its damage a second (450 to 825; its
  strength per cost is 1.21 of the inspiration's ratio, inside the band). One broadside destroys a
  T3 extractor and takes down a T2 dome, exactly; two destroy a commander, in about 19 seconds
  (the user's 26, then anything up to 40). It outranges a T2 Artillery Installation by 1.30, and
  from between the two reaches destroys four of them under a dome in about 48 seconds,
  unanswered, though that hangs on how close they stand (17 to 65 seconds). Domes come back 20
  seconds after they fall, at full health: the two shield generators', the Escort Corvette's
  (from 30) and the Mobile Shield Generator's (from 24), and the command rig's Field Shield (from
  140) and Area Shield (from 45). The T2 dome heals its health in 70 seconds: 13,020 at
  186 a second, which still holds off one cruiser. The T3 dome heals a little faster, 200 a
  second, on 36,000, so more slowly for its size; one battleship breaks it in about 48 seconds,
  and the broadside that does destroys the generator. That replaces the 2026-11-24 rule that set
  a turret's damage a second against the artillery's, and each battlecruiser laser at twice a
  turret's; and one battleship now breaks a coastal T3 position on its own.
* 2026-11-26: stats derived from a model, the user's rule (`[model]`,
  `reports/plans/2026-11-26-derived-stats.md`): no number borrowed or fitted, and no exceptions.
  Time to kill = 14 s × tier × domain × role, and reach = closing share × speed × time to kill,
  with health and damage split from it at today's strength until an ability price list sets
  cost. Land is unchanged. Navy closes at 0.57 of land's share: the destroyer is the navy line
  (measured on its guns, its torpedo a sub-hunting ability), the frigate a brawler, the torpedo
  boat the sub hunter archetype, the cruiser support. A defense window places artillery and point
  defenses; missiles reach twice their role's cannon; aircraft guns 0.7 of their tier's
  anti-air. The battleship broadsides every 13.5 seconds, and six submarines now take about 39
  seconds to sink a destroyer.
* 2026-11-27: the ability price list as code (`src/price_list.rs`, `[price_list]`). It prints
  any unit's price job by job (`--balance-derive unit <key>`, or every unit with `list`), and
  tests/price_list_test.rs fails when a unit it prices drifts more than `drift_max` from it,
  unless `[price_list.held]` names what else sets its cost. Worked as the list was by hand: each
  tier's line in its domain for reach and speed (aircraft against the T1 Interceptor's speed), a
  bomb's first-volley gap between passes, and a ship's sensors shared across its tier's fleet.
  The aircraft come within 1% of their costs, the ships within 7% (the Battlecruiser furthest,
  the Destroyer at 3% with its decoys now counted), until the navy base is solved from it.
  Landed shares measured again with turrets that fire only once laid: the T1 Bomber 0.54, the
  Fighter-Bomber's bomb 0.92 and its anti-air 0.98, the Strategic Bomber 0.90.
* 2026-11-27: the Interceptor's size, the user's rule (`[air] interceptors_clear_bombers_seconds`):
  an equal cost of Interceptors clears an equal cost of T1 Bombers in about the time it takes,
  10.3 seconds once guns fire only when laid (8.9 before). Every T1 air number scales with that
  size. Its health over its damage a second is now the [model] time to kill in air-to-air fights,
  9.1 seconds (8.7): 445 health and 48.9 a second, from 435 and 50. With it the Bomber's health
  (ten seconds of that gun) is 489, Mobile Anti-Air's gun 20.4, and the Bomber's cost 253. The
  Interceptor's cost is the list's at the T1 air base, set in `balance::derive_t1_air`.
* 2026-11-27: Mobile Heavy Artillery from the model. Gun artillery takes the Light Artillery's
  factor on the line's time to kill (`[model] role_time_to_kill artillery = 1.15`), so its health
  over its damage a second is 18.2 x 1.15 = 20.9 seconds: it fires every 13.8 seconds, from 11.5,
  with the shell as it was. Its cost is the middle of the costs at which the T3 line's micro cell
  holds (Heavy Walkers attack-moving into it lose by 0.3 or more, and with micro win by 0.3 or
  more): against ten, five to seven of it, 720 to 514 each, so 600, as it was.
* 2026-11-27: secondary reaches by rule (`[model]`, tests/reach_test.rs), each one already met at
  two tiers or the same rule a tier up. Static anti-air reaches 1.25 times its tier's mobile
  anti-air (the Anti-Air Turret 880 to 800), a ship's anti-air 1.2 times its tier's static (the
  battleship 1,200 to 1,440); a sub-hunting torpedo the sub hunter's reach at its tier (the
  destroyer's 900 to 814, the battlecruiser's 1,200 to 921); the T1 Torpedo Launcher the defense
  window's floor over the frigate's gun (1,000 to 698), as the T2 launcher does the destroyer's;
  the cruiser's gun its tier's navy line (1,200 to 1,255). An aircraft's anti-air reaches its
  tier's fighter (the Heavy Gunship 880 to 600, the Heavy Air Transport 1,000 to 600), and the
  transports' guns 0.7 of the anti-air they fly against, as the gunships' do (400 to 448, 500 to
  560). Frigates still lose to the T1 launcher, and torpedo boats still beat submarines.
* 2026-11-27: air bases, the design owner's picks. At T2 the standard land army, about a third
  Mobile Flak by materiel (labelled a fifth since 2026-11-24, which it never was), and an equal
  cost of the T2 strike aircraft break even (`[air] escort_forces`, `balance::derive_t2_flak`):
  the Mobile Flak's shell 58 to 45, with the aircraft as they are. Measured as the mean
  break-even over armies of three to six squadrons, each unit in its own block, shortest reach in
  front and the anti-air behind. The Flak Cannon keeps its shell, 1.85 times as strong per cost as
  the Mobile Flak, inside `[premiums]`; massed flak now takes nine times a Strategic Bomber's mass
  to all but bring one down. At T1 the air base is now the one that prices the Bomber at its cost,
  0.294 (`balance::t1_air_base`): the Interceptor costs 175, from 162, and an equal cost of them
  clears an equal cost of bombers in 11.2 s (the band is 9 to 12 s). Each air tier has its own base
  (`air_t1`, `air_t2`, `air_t3`); T3 stays at T2's for now, since against a fifth flak the answer
  turned on the army's other units and the order they march in (Heavy Gunships 0.94 to 1.66 times
  the better buy).
* 2026-11-27: shields. A dome costs the strength it adds to the group it covers
  (`[price_list.shield_groups]`, `price_list::shield_price`): the group's cost times
  √(1 + E / H) − 1, with H the group's health and E the dome's health, its regeneration over the
  model time to kill at its tier in its domain, and its hull's; a dome that cannot move is worth
  half again its cost (`[price_list] immobile`), as a point defense is. The Mobile Shield over the
  standard T2 army comes to 218, as it costs; the Escort Corvette over the T2 fleet's destroyers
  and cruisers 733 to 913 (its anti-torpedo charge not priced yet); the Shield Generator over the
  T2 position's three point defenses 629 to 932; the Heavy Shield Generator over the T3 position's
  3,667 to 3,104. Every shield now comes back 20 seconds after it falls, the design owner's rule
  for the domes and the commander's shields carried to the rest: the Field Rig's two (170 and 215
  seconds until now), the Heavy Walker's own (12) and the Heavy Air Transport's (35). The dearer
  Corvette made the T2 fleet dearer for the same strength, so torpedo bombers broke it at 1.98
  times its cost; the Torpedo Bomber's cost, the least that keeps it at twice, is 243, from 240.
* 2026-11-27: counters priced by the duel they win, in code (`[counters] counter_squadrons`,
  `balance::derive_torpedo_boat`, `derive_torpedo_bomber`, `--balance-derive counters`). The
  Torpedo Boat costs the most it can and still keep 0.70, the top of `winner_survivors`, against
  an equal cost of submarines, as the mean over fleets of four to seven of it: 819 to 794. One
  size alone turns on how the submarines round, a step of a tenth of its cost, and at four it
  read 0.71. The Torpedo Bomber costs the least that needs twice the T2 fleet's cost to break it,
  243 as it is. Both re-solve when what they fight changes, and a test holds them to it.
* 2026-11-27: build power in the price list, a utility job: its points at the least any unit that
  only builds pays for one, over the domain base, each engineer priced by the others. The T1 and
  T3 Engineers pay 10.4 a point and come out at their costs; the Combat Engineer, its 17.5
  points with its gun and radar, 224 to 253. The T2 Engineer lists at 125 and was held at 130 at
  first: at 125 one saddleback soak match (seed 3) ran to 4077 s, past the 4000 s limit, where
  both sides' strategic missiles flatten both bases at about 2,100 s and both rebuild. The Field
  Rig and the Command Rig carry it in their lists and stay held.
* 2026-11-27: the T2 Engineer at its list's 125 (the design owner's call), and the soak's 1v1
  limit 4000 s to 4500 s, since the match that ran long finished: the limit is for stalemates.
  The Escort Corvette's anti-torpedo charge taken off (the design owner): the T2 fleet still takes
  2.06 times its cost of torpedo bombers to break, so the Torpedo Bomber and the Torpedo Boat
  re-solve to their costs.
* 2026-11-27: the air transports' health from the design owner's kill times: the T2 Flak Cannon
  destroys a Light Air Transport in about 4 seconds (500, as it was), an Air Transport in about 11
  (1,675 to 1,810), and a Heavy Air Transport, through its shield, in about 55 (4,500 to 9,800).
* 2026-11-27: cargo in the price list (the design owner's pick): room for a full load of the
  tier's land line unit, at T2 energy prices, at the share the Light Air Transport pays for its
  own, 0.667, since it only carries. A transport's weapons do a quarter of its tier's gunship's
  damage a second (the Air Transport 48 to 18.8; the Heavy Air Transport 330 to 94, shared as
  before), and a shield of a unit's own counts in its health. The Air Transport 440 to 553
  (cargo 480, guns 73), the Heavy Air Transport 1,833 to 2,056 (cargo 1,440, weapons 616).
* 2026-11-27: game-ending weapons in minutes of a standard T3 economy, eight T3 extractors, 144 a
  second (`[missiles] strategic_minutes`): the Strategic Missile 2.98, its launcher 2.27, the
  Strategic Interceptor 0.79, today's costs written down; and each missile defense a share of the
  missile it stops, today's (`defense_over_missile`). Nothing moves. The Missile Submarine is left
  for the design owner.
* 2026-11-27: T4 constants, for experimentals (`balance::MAX_TIER`): a time to kill 1.45 of T1's,
  continuing the steps; a closing share of 0.48; tier efficiency 1.0; T3's energy price; and, in
  the air, T3's base until a T4 anti-air unit exists. A unit made T4 is measured against T3's line.
  No unit is T4 yet, so nothing moves. The size and overkill prices and the equal-cost-answer check
  wait for the first experimental.
* 2026-11-27: structure reach priced as a unit's is (the design owner's pick), at a closing share
  of its own (`[price_list] structure_closing_share`, `balance::derive_structure_closing_share`,
  `--balance-derive structures`), and over `immobile` as the rest of a structure's list is. It
  is priced for structures whose every weapon is a surface shell that outreaches the line, so the
  point defenses. The share, 0.535 at every tier, is the one at which the T3 Heavy Point Defense
  lists at the most it can cost and still hold, three against half again their cost in Heavy
  Walkers: 2,072 to 2,437. The T2 Point Defense lists at 638, from 550, and holds there. The T1
  Point Defense lists at 429, more than holding allows, so it costs the most that still holds,
  404, from 325, and is held. The shield generators follow their groups: the Shield Generator 932
  to 1,080, the Heavy Shield Generator 3,104 to 3,650. The playtest positions take 45 Medium
  Tanks (from 37) and 30 Heavy Walkers (from 26) to break, and the two coast duels' cost ratios
  are written down again. Easy's land cap 16 to 15 (GAME_SPECIFICATION.md, Difficulty), so that
  Normal still fields half as many units again on every test map.
* 2026-11-27: the battle line (the design owner's pick; `[battle_line]`, tests/battle_line_test.rs,
  `--balance-derive battle-line`): a force standing in fixed ranks by rule (direct fire in front,
  shortest reach first; anti-air and gunless shields in the middle; artillery and lobbing guns
  behind), and an equal cost of aircraft coming at it from the front, a flank and the rear, from
  four layouts each, the teams each way round, at four sizes. It is steady where the march was
  not: aircraft's efficiency falls smoothly as the army grows, 1.46 to 1.04 for Heavy Gunships over
  two to five squadrons. A standing army does worse than a marching one: the T2 strike aircraft are
  1.35 times the better buy against the T2 army in it, even on the march. The T3 air base is solved
  to that deal for the Heavy Gunships against the standard T3 army (the AI's T3 mix, a fifth flak,
  `[domains.forces] t3_land_army`): 0.902 to 1.005, so the Heavy Gunship 756 to 678, Air
  Superiority 879 to 790, the Strategic Bomber 1,254 to 1,138 and the Heavy Air Transport 2,056 to
  1,993. The navy base is solved so that the T2 fleet, its ships on the list, scores +0.6 against
  an equal cost of the T2 air force (+0.56 before): 0.669 to 0.702. The ships move -6% to +2%
  (the Battlecruiser, which had drifted 6% under its list, 4,987 to 5,070; the Cruiser 1,608 to
  1,529), the Escort Corvette 913 to 882 with its group, and the T3 fleet scores +0.88 against the
  T3 air force. The counters re-solve: the Torpedo Boat 794 to 748, the Torpedo Bomber 243 to 234.
  The cheaper Attack Submarine (402 to 379) tipped the AI on Scattered Isles from four yards to one
  by the twentieth minute (any cost under about 385 did), its mass store full all the while, so the
  AI now builds the yards and air factories its income wants before more extractors when its store
  runs over with no land route (GAME_SPECIFICATION.md).
* 2026-11-27: every sensor priced over its unit's domain base, as every other job is (the design
  owner's pick), so that one radar is priced by one rule on a scout and on a warship; a unit that
  only sees costs its sensors. The Air Scout lists by its radar past its vision: 78 to 62. The
  Spy Plane lists at 42 by its radar alone, and keeps its 257 as the design owner's cost until a
  price for seeing anywhere exists (`[price_list.held]`). The Bomber's radar now goes with the T1
  air base, so the base that prices the Bomber at its cost is 0.315 (0.294), and the Interceptor
  costs 164 (175). The T2 base that holds the strike aircraft at their cost is 0.904 (0.902); no
  T2 aircraft moves. The navy base re-solves on the battle line to 0.709 (0.702): ships move -1%
  to +1.3%, the Torpedo Boat 748 to 758 by the duel it wins. T3 air, at a base of about one,
  does not move.
* 2026-11-27: the Strategic Missile Submarine priced by the list (the design owner's pick): its
  strategic launching is half the Strategic Missile Launcher's cost by its share of the launcher's
  build power (1,080 of 1,500) and half by its share of its stock (3 of 5), 12,964, not over the
  navy base, since the launcher is priced in minutes of a T3 economy; its cruise missiles, which
  fire from under the water, pay the submerged price at T1's, 750; and its sonar, 37. 10,495 to
  13,751. The AI does not build it.
* 2026-11-27: shared constants (`[constants]`, tests/constants_test.rs), each at the number it has,
  so no unit moves: family rules every unit of a family follows (ship guns at 60 a tick but the
  battleship's lobbed shells, torpedoes at 30, tactical missiles by their launcher, 28 from a
  structure, 32 from a ship and 40 from a vehicle, strategic missiles at 25, fighter guns at 180,
  and the Interceptor, Bomber and Fighter-Bomber on one 64-unit turn radius), and veterancy and
  wreckage, the simulation's rules, written down and checked against the code. Each hull's turn
  rate, each unit's speed, each weapon's spread and each ship's and aircraft's split of mass and
  energy stay identity knobs. Not made one: the T3 fighter's turn radius (87.6, 37% wider than the
  family's) and one energy share a tier for aircraft (the T3 fighter spends 114 energy a materiel,
  the Interceptor 45); each would be a dramatic shift, so each stays its own.
* 2026-11-27: the Field Rig priced by the list (the design owner's plan): its gun, 677; its build
  power at the cheapest builder's rate, 582; and its income, a new utility job, what it makes a
  second, energy at its tier's rate, paid back in its tier's payback (`[economy] payback_seconds`,
  55, 250 and 400 seconds, about what each tier's extractors and generators pay back in), 457.
  2,240 to 1,716. The Command Rig's gun is a rule (`[commander]`, tests/commander_test.rs):
  holding its ground, it comes out ahead of 16 Medium Tanks attack-moving into it and behind 18,
  at the least damage that does, a tank in six shots, 70, as it has; and Discharge's least, 1,250,
  destroys any T1 land unit in one. The commander's cost stays the design owner's: it is never
  built, so its cost sets only its wreck and what destroying it is worth.
* 2026-11-27: every extractor and generator costs its tier's payback of what it makes, an upgrade
  of what it adds, energy at the tier's rate (`[economy] payback_seconds`, tests/payback_test.rs):
  T1's extractor 108 to 110 and generator 225 to 220; T2's extractor 954 to 1,000 and generator
  1,320 to 1,250; T3's extractor 4,916 to 4,800, in materiel with energy at the tier's rate. The
  T3 Power Generator keeps its 153-second payback, the design owner's pick (400 seconds would
  make it 10,000, from 3,816), and the Geothermal Plant its 16, since a vent is scarce
  (`payback_held`). Build time stays each unit's own, an identity knob (the design owner's pick:
  one number a domain would have had the T1 Bomber build 75% slower and the Gunship 46% faster). The T3 Mass Fabricator turns energy into materiel at T3's rate, as the T2 one
  does at T2's: 1,600 energy a second for its 16, from 1,500.
* 2026-11-27: the rigs' enhancements priced by the list (the design owner's pick;
  `[price_list.enhancements]`, `--balance-derive enhancements`): each costs what it adds to its
  rig, at T2 prices for the commander's (T3 for its T3 pack) and T3 for the field rig's. Weapons,
  sensors and build power cost what they add to the rig's price on the list; health, regeneration
  and shields the snipe price, the Strategic Bomber's cost over its bomb, 0.22 a point; hiding
  from radar the cheapest Stealth Field Generator; income its tier's payback; one that builds on
  another only what it adds. The commander's: Heavy Cannon 990 to 312, T2 pack 962 to 801, T3 pack
  2,762 to 853, Damage Control 990 to 451, Field Shield 3,262 to 4,248, Area Shield 1,714 to
  1,278, Signature Suppression 752 to 832, Onboard Reactor 6,429 to 9,286. The field rig's: Long
  Barrel 1,048 to 169, Capacitor Bank 1,378 to 273, Rapid Fabricator 1,243 to 437, Sensor Mast 762
  to 281, Field Shield 2,859 to 5,887, Area Shield 6,770 to 6,086, Onboard Reactor 5,143 to 7,619.
  Mass, energy and build time go together. Orbital Redeploy (both), the Radar Jammer and Thermal
  Camouflage are out of the roster (the design owner's pick); the engine keeps teleporting,
  cloaking and jamming, tested with tests/fixtures/retired_enhancements.toml. As a check, the Heavy
  Cannon lets the commander hold against 22 Medium Tanks rather than 16, about 240 of tanks.
* 2026-11-27: the Field Workshops (T2 and T3) out of the roster (the design owner's pick): a
  station is an engineer that cannot walk to its work, and cost 26 to 35 a point of build power
  against the engineers' 10.4, so the list would have halved it into an engineer that needs no
  minding. The AI's idle engineers assist its factories instead. The engine keeps assisting
  stations, tested with tests/fixtures/retired_units.toml.
* 2026-11-27: every factory priced as its build power, its hull and its tech (the design owner's
  pick; `[price_list.factories]`, `--balance-derive factories`). Build power at the engineers'
  10.4 a point wherever the point sits, health at the Wall Section's 0.007 a point, and for a
  headquarters and the Rig Yard a tech price, what opening its tier costs: the design owner's
  knob, kept at a headquarters' cost over its support factory's (T2 land 851, air 486, naval 900;
  T3 land 3,941, air 3,080, naval 7,276; the Rig Yard 1,825), to be tuned against [pace] with the
  AI's upgrade rules. T1 factories 20 to 32% cheaper (land 330 to 236), support factories 10 to
  33%, headquarters 3 to 9%; the Rig Yard unchanged. Mass, energy and build time go together.
  Today's soaks miss [pace]: matches run about 45 minutes against 30, the first T3 factory comes
  at about 33 against 16 to 22; the soak now reports both (`pace:`).
* 2026-11-28: every unit names what sets its cost (`price_list::cost_source`,
  `--balance-derive sources`, tests/price_list_test.rs): the list's price or what holds it, a
  dome's group, the factory recipe, minutes of a T3 economy, a share of the missile it stops, its
  tier's payback, or, for a unit no rule prices, a reason in `[price_list.unlisted]`. An anchor is
  the design owner's number that a rule prices others from: the Wall Section for a structure's
  hull, the Radar and the T1 Sonar for sensors, the Stealth Field Generator for stealth, the Light
  Air Transport for cargo. Storage and the fabricators stay the design owner's (the plan's pick).
  Written down by rule, today's numbers: the Heavy Artillery Installation, 9.95 minutes of a T3
  economy, and the Strategic Missile Defense, 0.94 (`[missiles] strategic_minutes`); a Medium Tank
  breaks a Wall Section in 17 shots (`[kills]`). The Tactical Missile Launcher is a factory for its
  missiles, which carry the damage: its build power and hull, 800 to 838. Twelve units are not
  derived yet, each named with the rule to come: the torpedo launchers, the static anti-air and the
  Mobile Flak, the T2 Artillery Installation, the Tactical Missile, the T2 Radar, the T2 Sonar,
  the Deep Scan Array and the Airfield.
* 2026-11-28: the torpedo launchers priced by the list (the design owner's pick): at the navy base,
  since they fight ships, their torpedoes' reach priced past the guns of the ships that close on
  them at the structure closing share, over the immobile premium, as a point defense is. The T1
  Torpedo Launcher 431 to 315, the T2 one 821 to 879, at the navy base of 0.670. Three of either
  hold against half again their cost in frigates or destroyers (`[premiums]`). Their damage and
  health stay as first set, written down as hit counts (`[kills]`): a T1 launcher sinks a frigate
  in 7 volleys and a frigate breaks it in 26 shells; a T2 launcher sinks a destroyer in 49
  torpedoes and a destroyer breaks it in 27 shells.
* 2026-11-28: T2's siege weapons in minutes of a standard T2 economy, as many T2 extractors as the
  T3 one has T3 extractors, 48 a second (`[missiles] t2_minutes`), today's costs written down: the
  T2 Artillery Installation 0.673, its reach past what the list's closing share prices; the
  Tactical Missile 0.0946, which the Tactical Missile Defense is priced from. The Deep Scan Array
  is the deep scan anchor, the only building with it; the Airfield's cost stays the design
  owner's, its repair slots a mechanic rule. Nothing moves.
* 2026-11-28: static anti-air priced at the floor of its premium over mobile (the design owner's
  pick of the plan's 1.5; `[premiums] static_anti_air`): each costs what puts its strength per
  cost against aircraft at 1.5 times its tier's mobile anti-air's, the SAM's over the Mobile Flak
  at T3 prices. The AA Turret 225 to 234, the Flak Cannon 419 to 516 (it stood at 1.85), the SAM
  838 to 620 (it stood at 1.11). The list, pricing them directly at the ground anti-air premium
  below and over the immobile premium, agrees within 2%. Ground anti-air: the list prices the
  Mobile Anti-Air, whose health per mass sets its cost, at 0.68 of that cost, and the Mobile Flak
  at the same share of its own, so the Mobile Flak costs its list price times what the Mobile
  Anti-Air pays over its own, 1.48: 160, as it was.
* 2026-11-28: sensor buildings priced by the sensor rule every mobile sensor follows (the design
  owner's pick; `price_list::sensor_building_price`): what each sensor sees past the building's
  vision at the price per area of the cheapest building with it, not over a domain base, upkeep
  left out as it is for a mobile sensor. The T2 Radar 206 to 328, the T2 Sonar 354 to 644. The
  Radar, the Sonar and the Deep Scan Array are the anchors the rule prices from.
* 2026-11-28: every unit names what sets each of its other stats (the design owner's "Yes" to an
  audit of every stat, then deriving them in passes; data/stat_sources.toml,
  `src/stat_sources.rs`, `--balance-derive stat-sources`, tests/stat_sources_test.rs). Its
  numbers fall in ten families: health, body, damage, range, speed, turning, timing, sensors,
  economy and special. A family is derived only when every number in it is: by a rule or a
  target, cited in data/balance_targets.toml or the code, or by an anchor of the design owner's.
  The rest say why not: held, the inspiration's, kept inside a band that does not pin it, today's
  value written down, or an identity knob ([constants]). Of 572 unit families, 111 are derived and
  461 are not: no turning, timing or economy family yet, since a turret's swivel, a hull's turn and
  every income are set by no rule. The test checks the rules no other test did: the land
  templates, the defense window's reaches, the navy line's and the brawler's reach, the navy's
  times to kill and speeds, and the battlecruiser's targets, written as `[model]`
  battlecruiser_speed_over_battleship and battlecruiser_laser_over_turret_min. The transports'
  weak weapons are `[air] transport_weapons_over_gunship`. Nothing moves. Checked against the
  data, some the first reading counted derived are not: the cruiser's hull and its missiles'
  reach, the Attack Submarine's torpedo and reach, which no `[model]` rule reproduces today; where
  the bombers let go; the Mobile Heavy Artillery's shell; and the domes, whose sizes are held.
* 2026-11-28: what counts as derived where no fight sets a stat, the design owner's pick: a rule,
  plus one named anchor per family that sets its scale, with the rest following by multiples; a
  per-unit number written down is not derived. The Interceptor's size, √(health × damage a
  second), is the anchor for T1 air's scale (`[air] interceptor_size` = 147.5, its size then),
  since every T1 air number scales with it and no fight pins it: sizes a tenth either side clear
  an equal cost of bombers in 8.0 to 9.2 s, jumping with whole units, so the 9 to 12 s band stays
  a check. Nothing moves; 459 unit families are not derived yet.
* 2026-11-28: turret swivel by rule, the design owner's pick. Every gun at the ground turns at the
  least rate that stays laid on anything it shoots at (`balance::turret_follow`, `[turrets]`):
  the quickest surface unit passing it at its least range, or brushing its hull where it has
  none, while the gun drives the other way, in whole degrees a second and at most a right angle a
  tick (`components::MOST_DEGREES_A_SECOND`). Direct-fire turrets turn 3 to 9 times as fast (the
  Medium Tank 90 to 826, the battleship's guns 30 to 238, the Light Walker and Land Scout at the
  900 cap), and artillery follows from its least range (the Light Artillery 90 to 39, T2
  Artillery 6 to 9, Mobile Heavy Artillery 30 to 28, T3 Artillery 3). Every gun at aircraft turns
  at one anchor, `[turrets] anti_air_degrees_a_second` = 180, as it did, above the 102 to 137
  that following an aircraft straight overhead needs. Anti-air is the part of a turn the fights
  feel: the follow rule there moved the T3 air base by 22% and the navy base by 9%, and a half
  turn a reload by 16% and 9%. With the ground guns by rule, the battle line does not move. The
  point defenses reprice: the closing share 0.535 to 0.5306, the T1 Point Defense 404 to 413, the
  T2 638 to 617, the Heavy 2,437 to 2,357, and the domes over them (the Shield Generator 1,080 to
  1,044, the Heavy 3,650 to 3,531). Massed raiders attack-moving into massed artillery now draw
  (-0.14 to +0.05), the Light Artillery slower to swing between them. Nine land templates'
  timing is now derived, so 450 unit families are not derived yet.
* 2026-11-28: shell speeds by family, and reload left to the hit sizes. Reload proved
  load-bearing: halving or doubling every held reload, at the same damage a second, moved the T3
  air base by 9 to 15%, the navy base by 5 to 15% and the T1 Point Defense's price by 9 to 15%,
  since kills come in whole hits. The design owner's pick: it waits for Pass 9, where it becomes
  hit size over damage a second. Shells (`balance::shell_speed`, `[shells]`): a direct-fire gun at
  the ground reaches its full range in the anchor's flight time, `[anchor] range` over `[anchor]
  projectile_speed`, as a line weapon's shell does (the commander 50 to 80, the T1 Point Defense
  70 to 91, the T2 70 to 200, the Heavy 64 to 240, the Gunship 160 to 82, the Heavy Gunship 70 to
  102, the Combat Engineer 120 to 66, the transports 120 to 82 and 102). Ships' guns, torpedoes
  and fighters' guns keep their `[constants]` speeds. Every other gun at aircraft flies at one
  anchor, `[shells] anti_air_speed` = 90, and flak at `[shells] flak_speed` = 40: a quarter slower
  or a third quicker on every anti-air shell moved the T3 air base from 0.99 to 1.55 or 0.80, while
  bringing the destroyer's, the battleship's and the heavy transport's to 90 moved nothing. The
  closing share re-solves 0.5306 to 0.5350: the T1 Point Defense 413 to 404, the T2 617 to 638,
  the Heavy 2,357 to 2,437, the Shield Generator 1,044 to 1,080, the Heavy 3,531 to 3,650; and the
  T3 air base 0.988 to 0.946, the T3 aircraft 4% dearer (the transport 1%). The commander's timing
  is now derived, so 449 unit families are not derived yet. The Mobile Anti-Air's gun now takes
  its kill time over its own reload, not over a one-second one (the same today).
* 2026-11-28: sight for units that close on what they fire at (`balance::gun_sight`). A mobile
  land unit whose guns all fire straight at the ground, and an aircraft armed only with guns, see
  their longest gun's reach times the land line's 1.1, inside `[ratios] sight_over_range_line`:
  the Command Rig 520 to 485, the Field Rig 520 to 550, the Combat Engineer 400 to 395, the
  Interceptor 560 to 550, the Air Superiority Fighter, Heavy Gunship and Heavy Transport 640 to
  660, the Gunship and the Transport 640 to 495. Duels run without fog, so no fight moves, and the
  Combat Engineer's radar costs 0.1 more. The same rule for every armed unit would let the
  Destroyer, the Battleship, the defenses and the artillery see 1.5 to 3 times farther in fog
  games, where they lean on radar and the Cruiser to spot past their sight; their sight is left
  for the radar and sonar pass. 443 unit families are not derived yet.
* 2026-11-28: the navy's sizes by hit counts (Pass 4, the navy batch). Each held size takes one
  count of hits, `[kills]`, `[secondary_kills]` or `[broadsides]`, and its number is set in the
  middle of what meets it, from what is already set: the Medium Tank sets the Frigate's gun (4
  shells, 119 to 122.5) and the Frigate's gun the Attack Submarine's health (11, 1,279 to 1,286);
  the commander sets the Destroyer's guns (the middle of `[kill_times]`
  destroyer_guns_commander_on_land, 455 to 439), which set the Cruiser's health (11, 4,404 to
  4,610), the Escort Corvette's hull (13 through its dome, 1,500 to 1,488) and the torpedo
  launchers'; the Battleship's broadside sets the Battlecruiser's health (3, 31,516 to 33,000).
  The secondary weapons: the Cruiser's gun (a Heavy Tank in 7, 226 to 213.7), the Destroyer's
  anti-air (a T1 Bomber in 32, 15.6 to 15.5) and torpedoes (a Missile Submarine in 22, its health
  4,000 to 4,085), the Battleship's anti-air (a Gunship in 17, 96 to 96.5), the Battlecruiser's
  torpedo (an Attack Submarine in 7, 200 to 199) and the Attack Submarine's deck gun (a Medium
  Tank in 19, 23 to 22.7). The navy base re-solves 0.670 to 0.664; the ships reprice by 3.3% or
  less (the Frigate 411 to 424, the Destroyer 1,655 to 1,623, the Cruiser 1,627 to 1,673, the
  Battleship 6,542 to 6,605, the Battlecruiser 5,313 to 5,483), the T1 launcher 315 to 329, the
  T2 879 to 857, the Torpedo Boat 801 to 809 and the Torpedo Bomber 132.5 to 131. Every fleet,
  submarine and torpedo-bomber check holds. 427 unit families are not derived yet.
* 2026-11-28: the air's and the land's sizes by hit counts (Pass 4, the air and land batches,
  the design owner's picks), in `[kills]` and `[secondary_kills]`, each in the middle of what meets
  it. Aircraft from the anti-air already set: a Mobile Anti-Air brings an Air Scout down in 2 (35
  to 31), a SAM a Spy Plane in 3 (2,000 to 2,125), an Interceptor a Fighter-Bomber in 35 (1,696 to
  1,687) and a Destroyer's anti-air a Torpedo Bomber in 81 (1,252 to 1,248). Their weapons from
  what they strike: a Gunship breaks a Heavy Tank in 62 (22.6 to 22.5), a Fighter-Bomber's bombs
  a commander in 15 (1,256 to 1,242.9), a Torpedo Bomber's torpedoes a Destroyer in 57 (189 to
  187.6), and the Heavy Gunship's gun a commander in the middle of `[kill_times]` (126 shots, 144
  to 143.4). The land's from the line unit of their tier: the engineers in 5, 4 and 12 shots (150
  to 135, 400 to 360, 800 to 813), the Combat Engineer in 11 (1,040 to 1,080; its gun a Heavy Tank
  in 93, 15 to 14.9), the Mobile Shield through its dome in 31 (its hull 150 to 138), the Mobile
  Flak in 10 (1,000 to 978) and the Field Rig in 227 (16,000 to 16,013; its gun a Heavy Walker in
  17 through its shield, 300 to 302.8); Mobile Heavy Artillery's shell breaks a Siege Walker in 13
  (859.1 to 882.2, its reload 13.8 to 14.2 s by its time to kill). Measured together, the T3 air
  base stays 0.946 and the navy base 0.664 (apart, each batch moved the T3 air base by 3 to 4%: the
  battle line's measure moves that much with small changes). The Torpedo Bomber's duel is now the
  mean over the battle line's fleet sizes (at two squadrons alone it swung from 1.6 to 2.7 as its
  cost moved a few percent), which prices it 131.2 to 139.6. 410 unit families are not derived
  yet.
* 2026-11-28: structures' health by hit counts (Pass 8). Each of the 49 structures takes today's
  hit count from the line unit of its tier in `[kills]`, a Medium Tank at T1, a Heavy Tank at T2
  and a Heavy Walker at T3, and its health sits in the middle of what meets it; a shield generator
  counts through its dome. No count changes. Most move under 3%; the most are the Radar (10 to
  15, one shot either way), the Stealth Field Generator (225 to 257, three), the Airfield (500 to
  463, five) and the Shield Generator's hull (250 to 203: 129 shots through its 13,020 dome, as
  before, but 2 rather than 3 once its dome is down). Only the T2 Point Defense is repriced (637.9
  to 631.8, to the list); the domains, the battle line and the counters come out the same. 361
  unit families are not derived yet.
* 2026-11-28: ship and land speeds by the role each plays (Pass 5, the design owner's pick). Every
  ship and land unit with a role moves at that role's share of its domain's base, the land's
  `[anchor] speed` 7.2 and the navy's `[model] speed_navy` 12.05 a tick, rounded to a tenth, as
  the Destroyer and the Battleship already did: the Frigate, the Attack Submarine and the Torpedo
  Boat (the sub hunter, at the assault role's 1.0) 12 to 12.1, the Escort Corvette (the T2 line)
  10 to 10.4, the Cruiser (artillery, for its missiles) 9 to 9.6, the Missile Submarine
  (artillery, T2's as T3 has none) 6.6 to 9.6, Mobile Anti-Air (the T1 line) 6.6 to 7.2, the
  Mobile Shield and the Combat Engineer (the T2 line) 7 and 6 to 6.2, and Mobile Heavy Artillery
  (artillery) 4.3 to 5.8; the Mobile Flak stays at 6.2. Five turrets re-derive by their rule (the
  Frigate's 757 to 760, the Cruiser's 516 to 529, the Attack Submarine's deck gun 841 to 844, the
  Combat Engineer's 835 to 844, Mobile Heavy Artillery's 28 to 30). Prices follow their rules:
  the Cruiser 1,673 to 1,708 (the list), the Escort Corvette 946 to 956 (its dome over the
  fleet), the Torpedo Bomber 139.6 to 145.4 (the massed T3 fleet fell to 1.90 times its cost in
  torpedo bombers, under the 2.0 floor; 2.00 at 145.4), the Mobile Flak 158 to 154 and the Flak
  Cannon and SAM 2.5% with it (Mobile Anti-Air's list price rose with its speed, so what it pays
  over the list fell from 1.48 to 1.44), and the Mobile Shield 219 to 217 (its dome). Mobile
  Anti-Air keeping up with the tanks it escorts brings the T1 Bomber's escort rule down from 247
  to 231, and so the T1 air base 0.323 to 0.345: the Interceptor 159 to 149 and the Air Scout
  61.8 to 56.3, both the list's. The Bomber's rule settles at 231 from any start between 226 and
  238, but from 244 it gives 222 and from 222 it gives 240, as whole bombers make its fights jump.
  The T2 strike aircraft now do a little worse against the T2 army (1.382 to 1.367), so the T3
  air base re-derives 0.946 to 0.922 and the T3 aircraft cost 2.6% more (the Heavy Gunship 719 to
  738, the T3 fighter 840 to 861, the Strategic Bomber 1,209 to 1,240, the Heavy Transport 2,027
  to 2,043). The Strategic Bomber is the snipe price, so the enhancements that add only health or
  shields fell to 8.2% under the list, past its 7%, and are repriced to it (Damage Control 451 to
  491, the commander's Field Shield 4,249 to 4,629 and Area Shield 1,277 to 1,392, the Field Rig's
  5,887 to 6,414 and 6,086 to 6,631). Every domains check still holds. 350 unit families are not
  derived yet.

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
