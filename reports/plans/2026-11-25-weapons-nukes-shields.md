# Plan: cruiser volleys, naval kill times, missile defense, nuclear flight, shields

2026-11-25. A plan only: no unit data or code changes yet. It covers eleven asks from the design
owner, in their order, and a twelfth added in review (the T3 SAM), then splits the work into pull requests.

All numbers below come from `data/units.toml` on `main` at 629556f. The sim runs at 10 ticks a
second. There is no armor system, so a hit takes its damage straight off health (or off a
shield that covers the unit). An unprotected commander is the Command Rig with no enhancements:
18,000 health, no regeneration, no shield.

A hit kills when health reaches 0 or below. Every number chosen here leaves some margin, so that
no target relies on an exact tie.

## Summary of proposed numbers

| Item | Unit / weapon | Now | Proposed |
|---|---|---|---|
| 1 | Cruiser anti-air missiles | 1,279 dmg, one every 2 s, splash 80 | **200 dmg × 8 a volley**, fired over ~1 s, a volley every 2 s, splash 80 kept |
| 1 | Gunship (T2) health | 1,796 | **1,590** (decided, see 1.2) |
| 1 | Strategic Bomber health | 7,500 | **6,400** (decided, see 1.2) |
| 2 | Cruiser cruise missiles | 454 dmg, one every 2.5 s | **454 dmg × 4 a volley**, one a second, then 6 s reload (10 s cycle) |
| 3 | Shield Generator (T2) regen | 50 /s | **190 /s** |
| 3 | Heavy Shield Generator (T3) regen | 131 /s | **500 /s** |
| 4 | Tactical Missile Defense (structure) | every 3 s | **every 1.8 s** |
| 4 | Battlecruiser missile defense | 1 mount, every 2 s | 1 mount, **every 1.8 s** |
| 4 | Battleship missile defense | 2 mounts, every 2 s each | 2 mounts, **every 1.8 s each** |
| 4 | Cruiser missile defense (not in the ask) | 2 mounts, every 2 s each | 2 mounts, **every 1.8 s each** (one rule for every mount) |
| 5 | Missile defense targeting | one pooled timer per unit | **one timer per mount**; never two on one missile; always hits (already true, now pinned) |
| 10 | Destroyer guns | 475 dmg × 2 every 4 s | **455** dmg × 2 every 4 s |
| 10 | Destroyer torpedoes | 560 dmg every 4 s | **181** (a commander in ~79 s by five, a T1 sub in ~27 s by one) |
| 11 | Torpedo Boat torpedoes | 100 dmg every 0.8 s | **126** dmg every 0.8 s |
| 12 | SAM Launcher (T3) | 2,400 dmg every 3.2 s | **850** dmg every **1.4 s** |

## New engine feature: volleys (salvos)

Items 1 and 2 both need a weapon that fires several shots spaced out in time and then reloads.
Nothing does that today: every weapon fires one shot per `reload_ticks`. (The Torpedo Boat's
comment says "four-torpedo volleys", but its data is one torpedo every 0.8 s.)

Proposed weapon field in `data/units.toml`:

```toml
weapon = { ..., reload_ticks = 100, salvo = { shots = 4, ticks = 40 } }
```

- `reload_ticks` stays the time from the start of one volley to the start of the next.
- Shot `i` (0-based) fires `floor(i × ticks / shots)` ticks after the volley starts. So 4 shots
  over 40 ticks fire at 0, 1, 2 and 3 s. 8 shots over 10 ticks fire at ticks 0, 1, 2, 3, 5, 6,
  7 and 8, which is "over about one second" in whole ticks.
- Loading checks that `ticks < reload_ticks`, so a volley always ends before the next one.
- If the target dies mid-volley, the shots left in that volley go to the next target the weapon
  would pick. If there is none, the volley ends. Reload still counts from the volley's start.
  Without this, an overkill volley on a T1 scout would waste seven missiles.
- `src/balance.rs` must count `shots` wherever it turns damage and reload into damage per
  second (lines 502, 655, 1241 and 1347 today), or the strength ratios will be wrong by the shot
  count.
- The world checksum covers the new fields, as it covers every weapon field.

## 1. Cruiser anti-air: 8 missiles a volley, every 2 seconds

### 1.1 What the targets ask for

Let V be one volley's damage, 8 × per-missile damage. A unit with health H dies in
`ceil(H / V)` volleys. Two volleys land "just under 4 s" (volley 2 starts at 2 s, its last
missile leaves at 2.8 s, then about 1 s of flight). Six volleys land "just under 12 s" (last
missile at 10.8 s). The cadence matches what was asked.

Each target is a range of allowed V:

| Target | Health | Volleys | V must be |
|---|---|---|---|
| Air Scout (T1) | 35 | 1 | ≥ 35 |
| Interceptor (T1) | 435 | 1 | ≥ 435 |
| Attack Bomber (T1) | 500 | 1 | ≥ 500 |
| Light Air Transport (T1) | 500 | 1 | ≥ 500 |
| Torpedo Bomber (T2) | 1,252 | 1 | ≥ 1,252 |
| **Gunship (T2)** | **1,796** | 1 | **≥ 1,796** |
| **Fighter-Bomber (T2)** | **1,696** | 2 | **848 to 1,695** |
| **Air Transport (T2)** | **1,675** | 2 | **838 to 1,674** |
| Spy Plane (T3) | 2,000 | 2 | 1,000 to 1,999 |
| Air Superiority Fighter (T3) | 4,500 | 3 | 1,500 to 2,249 |
| **Strategic Bomber (T3)** | **7,500** | 4 | **1,875 to 2,499** |
| **Heavy Gunship (T3)** | **8,789** | 6 | **1,465 to 1,757** |

### 1.2 Conflicts: the targets cannot all hold at today's health

No single V satisfies every row above. Three pairs conflict:

1. **Gunship vs Fighter-Bomber and Air Transport.** The Gunship (1,796) has more health than
   the Fighter-Bomber (1,696) and the T2 Air Transport (1,675). It cannot die in one volley
   while they need two.
2. **Strategic Bomber vs Heavy Gunship.** Four volleys for 7,500 needs V ≥ 1,875. Six volleys
   for 8,789 needs V < 1,758. Their health ratio is 1.17, but four against six volleys asks for
   at least 1.25 (5 / 4).
3. **Gunship vs Heavy Gunship.** V ≥ 1,796 for one breaks V < 1,758 for the other.

Health has to change for some units. The fewest changes is two:

**Option A (decided 2026-11-25): V = 1,600 (200 a missile). Lower two health values.**

- Gunship 1,796 → **1,590** (−11%), so one volley kills it.
- Strategic Bomber 7,500 → **6,400** (−15%), so four volleys kill it (4,800 < 6,400 ≤ 6,400).
  6,300 was first proposed. 6,400 was chosen in review (2026-11-25) so that the bomber has
  slightly more than four Gunships' health (6,400 / 1,590 = 4.03). Four volleys meet it exactly,
  and that is enough: a unit dies when its health reaches 0 or below (`world.rs` 3815), and 200
  is exact in fixed point. It takes the same 32 missiles as anything above 6,200, so it is no
  less robust than 6,300. Any later change to the Gunship should keep the bomber just over four
  times it, and at most 6,400 while a volley is 1,600.
- Every other unit keeps its health. Check: Fighter-Bomber 1,696 > 1,600 (2 volleys). T2
  Transport 1,675 > 1,600 (2). Spy Plane 2,000 (2). T3 fighter 4,500, between 3,200 and 4,800
  (3). Heavy Gunship 8,789, between 8,000 and 9,600 (6).

**Option B: V = 1,800 (225 a missile). Change four health values.**

- Fighter-Bomber 1,696 → 1,900, and T2 Air Transport 1,675 → 1,900 (two volleys).
- Heavy Gunship 8,789 → 10,000 (6 volleys: 9,000 < 10,000 ≤ 10,800).
- Strategic Bomber 7,500 → 7,000 (4 volleys: 5,400 < 7,000 ≤ 7,200).

Option A touches fewer units and keeps the per-missile number round.

### 1.3 What Option A affects downstream

- Cruiser anti-air damage per second goes from 640 to 800 (+25%). Its 80 splash now lands eight
  times a volley, so a tight clump of aircraft takes up to 1,600 each per volley, against 1,279
  per shot before. The [domains] fleet cells (`fleet_beats_air_force_*`,
  `torpedo_bombers_to_break_fleet_*` with its upper end of 5.0) need re-running.
- The Gunship's [domains] strength (√(dps × health) per cost) drops about 6%. The Strategic
  Bomber's drops about 8% (√(6,400 / 7,500)). Both need to stay inside `strength_band` [0.80, 1.25].
- The SAM Launcher now has targets of its own against both of these aircraft (item 12).
- Not in the ask: the T3 Heavy Air Transport (4,500 health plus a 3,000 shield) takes 5 volleys.

## 2. Cruiser cruise missiles: 4 over 4 s, then 6 s reload, a commander in about 100 s

Four missiles every 10 s is 0.4 missiles a second. For 18,000 in about 100 s, each missile does
about 18,000 / (0.4 × 100) = 450. The current **454 already fits**: 39 × 454 = 17,706 is not
enough and 40 × 454 = 18,160 is. So it is **40 missiles in 10 volleys**.

- The 40th missile leaves at 9 × 10 + 3 = 93 s.
- Flight time is about 4 s at 1,200 range and about 9.5 s at the full 3,000 (32 a tick along
  the ground, plus the climb to 400).
- **Kill time is about 97 to 103 s**, depending on range.
- Average damage per second is 1,816 / 10 = 181.6, the same as today's 454 / 2.5 s. The
  cruiser's [domains] strength and `coastal_seconds_max` therefore do not move. What changes is
  the burstiness, which matters against missile defense (see 4.2).

Data: `weapon = { kind = "tactical_missile", automatic = true, ..., reload_ticks = 100, damage = 454, salvo = { shots = 4, ticks = 40 } }`.

## 3. T2 Shield Generator holds against one cruiser

One cruiser does 181.6 a second on average to a dome, in 4 s bursts of 454 a second. Regen at or
above 181.6 holds forever. **Proposed: 190 a second** (5% margin; 19 a tick).

- The worst moment is the end of a volley: 1,816 in, about 4 × 190 = 760 back, so the dome dips
  by roughly 1,056 out of 5,000 and is full again before the next volley.
- Against two cruisers: 363 in against 190 back, so the dome falls in about 29 s. One holds, two
  break it.
- A missile's splash can bleed into a second dome it overlaps. It never hits the same dome twice.

Conflicts and knock-on effects:

- **The T3 Heavy Shield Generator would regenerate less than the T2.** It has 131 a second
  against the T2's proposed 190. **Decided 2026-11-25: raise it to 500 a second**, which keeps
  today's ratio to the T2 (131 / 50 = 2.6×; 190 × 2.6 ≈ 500).
- Every other weapon that wears down a T2 dome now needs 3.8 times the fire just to outpace
  regen. For example, it takes about 7 T1 Light Artillery (28 a second each) just to stall it.
  The [siege] and [missiles] checks need re-running. Upkeep (30 energy a second) could rise with
  it. The Escort Corvette (80 a second) is unchanged unless asked.

## 4. Missile defense every 1.8 s, battleship with two

Proposed `reload_ticks = 18` for every tactical mount: the Tactical Missile Defense structure (30
today), the Battlecruiser (one mount, 20 today), and the Battleship (two mounts, 20 each today).
The cruiser was not in the ask. Its comment says every mount is "the same wherever it is
carried", so it moves to 18 too, with its two mounts.

### 4.1 Mounts become separate defenses

Today, a unit's mounts share one pooled timer: `reload_ticks / mounts` (`blueprints.rs` 1087).
So a battleship stops a missile every 1 s, but never two in the same tick. The ask says the
battleship has "2 missile defenses (each shooting once every 1.8 seconds)". So each mount gets
its own timer. If two missiles arrive together, both mounts fire that tick, at different
missiles. `MissileDefense` gains `ready: [u32; N]` (or one entry per mount in the
`missile_defenses` list). The divisibility check on `reload_ticks` goes away.

### 4.2 Effect on the new cruiser volleys

One missile is inside a defense's 620 reach for about 1.4 s as it dives. One 1.8 s mount against
4 missiles a second apart stops about 2 of the 4. A commander behind one T2 missile defense would
take about twice as long to kill as the 100 s of item 2. That seems the intent of missile
defense, and is noted here so it is not a surprise.

## 5. No double-targeting, and always a hit

This already holds today. `intercept_missiles` (`world.rs` 3328) runs once a tick, before
anything moves. It takes defenses in `SimId` order. Each one skips any missile already in this
tick's `destroyed` list, and a destroyed missile is removed before the next tick. Interception is
instant and certain, for tactical defenses and for strategic interceptors. Nothing can miss.

The work here is to keep that true when mounts split (4.1) and to pin it with tests:

- N defenses (structure and ship mounts mixed), all ready the same tick, against M < N missiles:
  exactly M interceptions, each of a different missile, and N − M defenses stay ready.
- The same for strategic defenses: one interceptor spent per nuclear missile, never two.
- A missile once targeted is always destroyed: no missile that was targeted lands.
- If interceptors ever get a visible flight time, a missile must be **claimed** when it is
  targeted rather than when it dies. The plan keeps interception instant, so no claim is needed
  now, but the test above guards it.

## 6. Nuclear missile flight path

Today a nuke uses the cruise-missile path: a 45° climb to 500 above the ground, terrain-following
cruise, and a shallow dive over the last 1,500. Its speed is 25 a tick along the ground, not
along its path. It skips shields already, but it still runs unit and terrain collision near the
ground.

Proposed path, in a new `NuclearFlight` component (separate from `Missile`):

1. **Rise** straight up out of the silo (see 8).
2. **Climb** straight up to an apex height `apex` above sea level. Proposed 2,000, as data
   (`cruise_height` on the strategic weapons becomes the apex).
3. **Turn** through a quarter circle of radius `turn` (proposed 300) to level flight, heading for
   the target.
4. **Cruise** level at the apex height. It is parallel to the ground as a whole, not
   terrain-following.
5. **Turn** through a quarter circle to straight down, starting `turn` before the target.
6. **Dive** straight down and burst at the ground (or water surface) at the target point.

Rules:

- **Constant speed along the path**: 25 a tick in 3D, the same in every leg. Position is a
  function of the distance travelled, `s`, which grows by `speed` each tick. The arcs use the
  existing fixed-point `cos_sin_radians` in `src/math/fp.rs`, so the path stays deterministic and
  mirror-exact.
- **No collision**: a nuke never tests units, structures, terrain or domes. It cannot be stopped
  by anything but a strategic interceptor. It detonates when `s` reaches the path's length, at
  the ground height of the target point, with today's two rings (inner 70,000 within 600, 500
  within 800). Shields are ignored, as now.
- Strategic defense keeps measuring reach across the ground only, so the high flight changes
  nothing there.
- Lifetime becomes the path's length / speed plus a few ticks, instead of the `× 2 + 100` guess.
- **Flight time grows.** A launch across 3,400 (the PR 25 playtest scenarios) goes from about
  14 s to about 28 s with a 2,000 apex. The scenarios' time limits and the audio launch warning
  timing need checking.

## 7. Glowing nuclear icon in flight

Draw it in two parts, so it is visible at every zoom:

- **An icon in the HUD layer**, drawn with egui where unit icons are drawn (`src/icons.rs`
  style, vector shapes, no image files). It is a radiation trefoil, black on a yellow disc, at
  the missile's screen position. It has a fixed minimum size (proposed 28 px, well above unit
  icons) and is always on top. A soft halo pulses at about 1.5 Hz: two or three
  translucent rings fading out. The same mark shows on the minimap.
- **A glow on the missile itself**: the `NUKE` color in `instances.rs` drawn as glowing (as built
  units glow), a larger exhaust flame, and a short glowing trail.
- Possible follow-up, not asked: a screen-edge arrow when an enemy nuke is in flight off-screen.

Tests: an icon-sheet example (`examples/icon_sheet.rs`) entry, and a unit test that the icon's
size never drops below the minimum at the farthest zoom.

## 8. Silo doors and the launch sequence

The launch timing matters to gameplay (when the missile appears, when the next can fire), so the
sequence lives in the sim, and the renderer only draws it.

On a fire order, the Strategic Missile Launcher runs `SiloLaunch`:

| Phase | Ticks (proposed) | What happens |
|---|---|---|
| Opening | 30 (3 s) | Two doors open. No missile yet. |
| Rising | until the missile's tail clears the roof plus 10 | The missile appears inside the silo, below the roof, and rises straight up at its constant speed. This is leg 1 of the path in 6. |
| Closing | 30 (3 s) | Doors close. |
| Closed | — | Ready for the next order. Today's 10 s reload covers the whole sequence. |

- The view gives each silo a door openness from 0 to 1. The `NukeSilo` mesh's flat top becomes
  a hatch frame plus two door leaves drawn as separate parts, as turrets are. They swing open
  about their outer hinges, or slide apart (to settle in review). The renderer interpolates
  between ticks.
- The Strategic Missile Submarine does the same with one small deck hatch (proposed, same
  timings).

**"Slowly flies out" against "constant speed".** At the nuke's 25 a tick, it would clear a
60-unit silo in about 3 ticks (0.3 s), which does not read as slow. **Decided 2026-11-25: a slow
lift inside the silo only**, at about 3 a tick for 2 s, then the constant 25 for the whole
flight. The Rising phase above runs at the lift speed.

## 9. Shield shimmer

Today `fs_dome` (`shaders.wgsl` 531) draws a uniform hex grid, a faint face and a bright rim, all
steady. Proposed, all in the fragment shader, using wall time (`globals.water.x`):

- **Roaming glints**: about 6 soft bright spots per dome. Each sits at a direction on the sphere
  picked by an integer hash of (dome seed, glint index, cycle number). Each fades in and out over
  its own 2 to 4 s cycle, with staggered phases, then moves to a new hashed spot. Brightness falls
  off with the angle between the fragment's normal and the glint direction (a Gaussian on the dot
  product). Inside a glint, the hex lines brighten more than the face, so the shimmer reads as
  light running along the lattice.
- **Slow drift**: a low-frequency value-noise field scrolls across the dome and gently modulates
  hex-line brightness (±25%), so the grid is never perfectly uniform.
- **Strength cue**: glints grow more frequent and flicker as the dome's health falls (the dome's
  health share passes in the instance color's spare channel or a new instance value).
- **Optional: hit ripples.** A short bright ring where a shell struck. This needs impact points
  from `pending_shield_hits`, which the view does not carry today.
- Each dome gets a per-instance seed (from its `SimId`), so neighbouring domes do not shimmer in
  step.
- Cost: a few hash and exp calls per dome fragment. On the Low quality setting, use 3 glints and
  no drift.
- Check: before and after landscape captures (`--landscape`) at three zoom levels, and an effect
  sheet frame series for the fade cycle.

## 10. Destroyer against an unprotected commander

### 10.1 Guns, commander on land, about 80 s

The destroyer's two gun mounts each fire one shell every 4 s, so 2 shells a volley. A shell
flies about 2 s at 1,200. With n volleys, the time is (n − 1) × 4 + 2. Twenty volleys is 78 s
and 21 is 82 s, so the target is 40 shells.

That needs 39d < 18,000 ≤ 40d, so 450 ≤ d < 461.5. **Proposed: 455** (from 475, −4%). That is
40 shells, 20 volleys, **about 78 s**. The anti-air gun cannot target land, and torpedoes cannot
hit a land unit. A spread of 7 against the commander's radius of 16 means a commander standing
still takes every shell.

### 10.2 Torpedoes: five destroyers against a commander in about 80 s, one against a T1 sub in about 25 s

The guns cannot target a seabed walker or a submerged submarine (already pinned by
`target_category_test.rs`). A destroyer fires one torpedo every 4 s, with about 3 s of flight.

Two targets, both on the same torpedo (the second added 2026-11-25):

| Target | Health | Torpedoes | Seconds | Damage d must be |
|---|---|---|---|---|
| Commander, five destroyers | 18,000 | 100 (20 volleys of 5) | 19 × 4 + 3 ≈ 79 | 180 to 181.8 |
| | | 95 (19 volleys) | 18 × 4 + 3 ≈ 75 | 189.5 to 191.4 |
| T1 Attack Submarine, submerged, one destroyer | 1,163 | 7 | 6 × 4 + 3 ≈ 27 | 166.2 to 193.8 |
| | | 6 | 5 × 4 + 3 ≈ 23 | 193.9 to 232.6 |

**Both hold with one value. Proposed: 181** (from 560). The commander dies to the 100th
torpedo at about 79 s (99 × 181 = 17,919; 100 × 181 = 18,100). The submarine dies to the 7th at
about 27 s (6 × 181 = 1,086; 7 × 181 = 1,267). The 4 s reload means the submarine's time can
only be about 23 s or about 27 s. 194 would give 23 s for the submarine but 75 s for the
commander, so 181 sits closer to both targets.

The Attack Submarine has no anti-torpedo charge, so every torpedo lands.

This replaces the earlier idea of a commander-only damage value. The submarine target sets how
strongly a destroyer hunts submarines, so the [domains] cell `destroyers_beat_submarines = 0.40`
is now a check rather than what sets the torpedo. A rough estimate says it still holds:

- An equal cost of Attack Submarines is about 6 (by mass, 1,658 / 265).
- Six submarines fire 1.5 torpedoes a second. The destroyer's anti-torpedo charge decoys one a
  second, so about 0.5 a second get through, about 93 damage a second.
- The destroyer kills one submarine about every 27 s. Leaks stop once 4 submarines are left (one
  torpedo a second, all decoyed).
- So the destroyer takes about 93 × 27 + 46 × 27 ≈ 3,750 of its 10,231 and wins with over 60%
  left. The fight takes about 2.5 minutes, against about 1 minute today.
- PR 3 re-runs the cell in the sim to confirm.

## 11. Two torpedo boats against an unprotected commander, about 60 s

Each boat fires one torpedo every 0.8 s, with about 3 s of flight. In 60 s, the boats fire
roughly 143 or 144 torpedoes between them (72 and 71). That needs 142d < 18,000 ≤ 143d, so
125.9 ≤ d < 126.8. **Proposed: 126** (from 100, +26%). One boat's 72nd torpedo leaves at 56.8 s,
for a **kill at about 60 s**.

- Side effect: the Torpedo Boat's [domains] strength rises about 12% (√1.26). It sits at 0.32 of
  the anchor within a band of [0.80, 1.25] times that, so re-run [domains].
- If that pushes it out, the band or the boat's cost is the thing to revisit, since the 60 s
  target is fixed.
- Separately, the "four-torpedo volleys" comment could become a real `salvo` (4 shots every
  3.2 s gives the same damage per second). That is not needed for the 60 s target, and is left as
  a question.

## 12. T3 SAM Launcher: a Strategic Bomber in about 11 s, a Gunship in about 2 s

Added in review on 2026-11-25, against the decided health values (Strategic Bomber 6,400,
Gunship 1,590). The SAM's missile flies 90 a tick and steers itself, so it reaches an aircraft in
range in about 1 s. With n hits and a reload of R seconds, the kill time is (n − 1) × R + 1.

**Today's SAM (2,400 every 3.2 s) misses both targets.** The Gunship falls to 1 hit in about 1 s.
The Strategic Bomber falls to 3 hits in about 7.4 s.

**Clash with a short reload.** Two seconds for the Gunship leaves about 1 s of reload. With
R = 1 s, the bomber's 11 s would need 11 hits against the Gunship's 2. But the bomber has only
4.03 times the Gunship's health, so with 2 hits for the Gunship it can take at most 9. A longer
reload resolves it:

| Target | Health | Hits | Time with R = 1.4 s | Damage d must be |
|---|---|---|---|---|
| Gunship (T2) | 1,590 | 2 | 1.4 + 1 ≈ 2.4 s | 795 to 1,589 |
| Strategic Bomber (T3) | 6,400 | 8 | 7 × 1.4 + 1 ≈ 10.8 s | 800 to 914 |

**Proposed: 850 damage every 14 ticks (1.4 s).**
- Gunship: 850 is short and 1,700 kills, so 2 hits, about 2.4 s at full range and about 2 s
  closer in.
- Bomber: 5,950 is short and 6,800 kills, so 8 hits, about 10.8 s.

The other way is one hit for the Gunship (1,800 every 3.3 s, 4 hits on the bomber, about 11 s).
That kills a Gunship in about 1 to 1.3 s, which is half the asked time, so it is not proposed.

Knock-on effects:

- The SAM's damage per second falls from 750 to 607 (−19%). Against the new bomber's health,
  today's SAM would have killed it in 7.4 s (3 hits), so the 11 s target makes the SAM slower against
  bombers on purpose.
- The SAM's comment, "one badly damages a strategic bomber in a pass, two bring it down", still
  holds. At 400 a second, a bomber crossing the SAM's 1,200 reach is in range for about 6 s. In
  that time one SAM lands about 5 hits (4,250 of 6,400), and two SAMs land about 10, which kills
  it.
- Splash 30 and homing stay. `[premiums] static_anti_air_over_mobile` [1.5, 2.0] counts static
  anti-air's strength per cost, and a 19% cut in damage per second could push it below 1.5. It
  needs re-running. If it falls short, the SAM's health or cost is the place to make it up, since
  the kill times are fixed.
- [kills] gains `t3_sam = { t2_gunship = 2, t3_strategic_bomber = 8 }`.

## Tests that pin all of this

1. **New `[kill_times]` and `[volleys]` sections in `data/balance_targets.toml`**, read by tests
   the way [kills] is read by `tests/kills_test.rs`:

   ```toml
   [volleys]
   # Volleys of the cruiser's anti-air to destroy each aircraft at its base health.
   t2_cruiser = { t1_air_scout = 1, t1_interceptor = 1, t1_bomber = 1, t1_light_transport = 1,
                  t2_torpedo_bomber = 1, t2_gunship = 1, t2_fighter_bomber = 2, t3_spy_plane = 2,
                  t2_transport = 2, t3_air_superiority = 3, t3_strategic_bomber = 4, t3_heavy_gunship = 6 }

   [kill_times]
   # Seconds for these attackers, alone, to destroy a Command Rig with no enhancements that holds still.
   cruiser_missiles_commander = [95, 105]
   destroyer_guns_commander_on_land = [75, 85]
   five_destroyers_torpedoes_commander_under_water = [75, 85]
   # One destroyer's torpedoes against a submerged T1 Attack Submarine.
   destroyer_torpedoes_t1_sub_submerged = [22, 28]
   two_torpedo_boats_commander_under_water = [55, 65]
   # And the cruiser's anti-air volleys in flight time.
   cruiser_two_volleys_seconds_max = 4
   cruiser_six_volleys_seconds_max = 12
   # The T3 SAM Launcher against one aircraft held in its range.
   sam_t2_gunship = [1.5, 3.0]
   sam_t3_strategic_bomber = [9.5, 12.5]
   ```

2. **Arithmetic tests** (fast, no sim): every [volleys] entry holds exactly,
   `ceil(health / (damage × shots))`. Every [kill_times] shot count matches. `salvo` timing
   matches `floor(i × ticks / shots)`.
3. **Sim tests**, new `tests/kill_times_test.rs`, built on the `run_until` helpers in
   `missile_test.rs`. Each places the attacker and a commander in an open world. That is a coast
   for the destroyer and cruiser on land, and deep water with the commander on the seabed for the
   torpedoes. Each runs until the commander dies and asserts the seconds are within range. A
   second set does the same for the anti-air: one Gunship, Fighter-Bomber, and so on, held in
   the air over open water, killed in the expected number of volleys and in time.
4. **Shield**: one cruiser against a T2 dome for 300 s never collapses it. Two cruisers collapse
   it within 20 to 40 s.
5. **Missile defense**: mount count and 1.8 s cadence per unit, simultaneous missiles taken by
   separate mounts, no double-targeting (item 5's tests), every targeted missile destroyed.
6. **Nuclear flight**: the apex reached, speed per tick constant to within 1 unit in every leg,
   ending within 1 unit of the target at ground height. A dome, a tall structure and an
   aircraft placed in the path do not stop or detonate it. The silo phases run in order, with
   the missile appearing only once the doors are fully open.
7. **Goldens**: the world checksum hashes all of `data/units.toml`, so each data PR regenerates
   `tests/golden` (`UPDATE_GOLDEN=1`, release `--test determinism_test`). The nuclear playtest
   scenarios from PR 25 get re-checked for the longer flight.

## Pull requests, in order

Each PR is reviewable alone and leaves `main` playable.

1. **Volleys in the engine.** The `salvo` field, its schedule, retargeting mid-volley, damage per
   second in `balance.rs` counting shots, and the checksum. No unit uses it yet. Includes the
   arithmetic test harness and the `[kill_times]` / `[volleys]` loaders (empty).
2. **Cruiser and anti-air rework (items 1, 2, 12).** Both cruiser weapons onto volleys. Anti-air
   at 200 a missile. The SAM at 850 every 1.4 s, with its [kills] rule and kill-time tests.
   The Gunship and Strategic Bomber health values (Option A). `[volleys]` filled
   in, the cruiser kill-time sim tests, a [domains] re-run report in `reports/balance/`, and new
   goldens.
3. **Naval kill times (items 10, 11).** Destroyer guns at 455, destroyer torpedoes at 181,
   Torpedo Boat at 126, the kill-time sim tests (the submarine one too), a [domains] re-run
   (`destroyers_beat_submarines` above all), and goldens.
4. **Shields and missile defense (items 3, 4, 5).** T2 regen at 190 and T3 Heavy at 500,
   per-mount timers, 1.8 s everywhere, the no-double-targeting tests, the shield-holds tests, a
   [siege] and [missiles] re-run, and goldens.
5. **Nuclear flight and silo sequence, sim side (items 6, 8).** `NuclearFlight`, the
   no-collision ground burst, the `SiloLaunch` phases (and the submarine hatch), door openness in
   the view, updated PR 25 scenarios, and goldens.
6. **Nuclear visuals (items 7, 8).** The trefoil icon and halo, the missile glow and trail, the
   silo door parts and animation, and icon and effect sheet updates.
7. **Shield shimmer (item 9).** Shader-only, with before and after captures. It can go at any
   point, in parallel with the rest.

PRs 1 to 4 are about balance and should go in order (2 depends on 1; 3 and 4 only on 1). PR 5
depends on nothing above. PR 6 depends on 5. PR 7 is independent.

## Questions to settle before code

The four big ones are settled. Only the small defaults in 5 remain.


1. ~~Anti-air health conflict (1.2)~~. Settled 2026-11-25: Option A (Gunship 1,590, Strategic
   Bomber 6,400, 200 a missile).
2. ~~Destroyer torpedoes (10.2)~~. Settled 2026-11-25 by the new submarine target: 181 for
   everything.
3. ~~Nuke exit speed (8)~~. Settled 2026-11-25: a slow lift in the silo, then constant speed.
4. ~~T3 Heavy Shield regen (3)~~. Settled 2026-11-25: raise it to 500 a second.
5. Smaller ones, with defaults the plan uses unless told otherwise: cruiser missile defense also
   at 1.8 s; nuke apex 2,000; silo doors swing (not slide); the submarine's hatch animates too;
   the Torpedo Boat stays one torpedo at a time.
