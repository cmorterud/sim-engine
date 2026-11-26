# Deriving unit stats from first principles (2026-11-26)

The design owner's rule: every stat not pinned by a target he set is derived from an explicit
model, never borrowed from another game or fitted to today's values. Small shifts in balance are
fine; dramatic ones are not (a frigate stays far below a destroyer). No exceptions. The model's
constants are in `[model]` in data/balance_targets.toml. This is step 1 and 2 of the plan (time to
kill and range); step 3, an ability price list that sets cost, comes next.

## The model

1. **Time to kill** (health over damage a second, between equal groups of one unit) =
   14 s × tier (1, 1.15, 1.3) × domain (land 1, navy 3, air-to-air 0.65) × role.
2. **Reach** = closing share × speed × time to kill. Land closing shares are 0.35, 0.44 and 0.47
   by tier; navy closes at 0.57 of land's. The brawler's share is 0.12 and the sub hunter's 0.35.
   Base speeds: land 72, navy 120.5 (about 85 seconds to cross 10 km).
3. **Strength split** until step 3 prices abilities: health = S√T, damage a second = S/√T, with
   S = √(health × damage a second) held at today's.
4. **Defense window**: artillery reaches 1.7, 3 and 3 times its tier's line reach; a point defense
   reaches the artillery's ÷ 1.2 and must outrange its tier's direct fire by at least 15%.
5. **Missiles** reach twice their role's cannon. The missile cruiser's least range is 15% of its
   reach; cannon cruisers in other factions reach half as far.
6. **Aircraft** guns at ground targets reach 0.7 of their tier's anti-air (T3: the Mobile Flak).
7. **Multi-job units**: the primary weapon carries the role's time to kill, and each sub-role is an
   ability priced in step 3. The destroyer's time to kill is measured on its guns; its torpedo is
   its sub-hunting ability, set by the existing kill-time targets.

Archetypes shared across factions: destroyer (navy line), sub hunter (torpedo boat here), cruiser
(support; missile or cannon).

## What changed

| Unit | Stat | Before | After |
| --- | --- | --- | --- |
| Frigate (brawler) | health / gun reach / gun damage | 2,984 / 560 / 118 | 2,949 / 607 / 119 |
| Attack Submarine | health / torpedo reach / torpedo | 1,163 / 640 / 235 | 1,279 / 404 / 214 |
| Torpedo Boat (sub hunter) | health / reach | 2,800 / 1,100 | 3,040 / 814 |
| Destroyer (line) | health / speed / gun reach / torpedo | 10,231 / 11 / 1,200 / 181 | 10,990 / 10.4 / 1,255 / 190 |
| Cruiser (support) | health / missile reach / missile / gun | 4,358 / 3,000 / 454 / 233 | 4,404 / 2,986 (least 448) / 450 / 226 |
| Battlecruiser | health / speed / lasers | 35,000 / 8.5 / 1,600, 600 | 34,421 / 10.2 / 1,498, 610 |
| Battleship | health / speed / reach / broadside | 107,000 / 5.5 / 3,000 / 16 s | 90,314 / 7.2 / 2,990 / 13.5 s |
| T1 / T2 Point Defense | reach | 520 / 1,000 | 496 / 1,095 |
| Heavy Point Defense | reach | 1,400 | 1,313 |
| T2 Torpedo Launcher | reach | 1,200 | 1,443 (window floor: 1.15 × destroyer guns) |
| Mobile Heavy Artillery | reach / least | 1,680 / 420 | 1,575 / 394 |
| Mobile Missile Launcher | reach / least | 1,295 / 259 | 1,314 / 263 |
| Gunship / Heavy Gunship | reach | 440 / 500 | 448 / 560 |

Land line units are unchanged: the model reproduces them within rounding.

## Targets that move

- The cruiser's missile damage is pinned by its commander target ([95, 105] s): the split's 441
  took 106 s, one volley too many, so the missile is 450 and health 4,404 keeps its strength.

- Six Attack Submarines sink a destroyer in about 39 seconds (was 33): the destroyer took the
  navy line's health.
- The battleship's commander kill drops to about 16 seconds (two broadsides 13.5 s apart), inside
  [15, 40]. The T3 dome still breaks in about 41 seconds, inside [40, 52].

## Still to derive

- The T1 Torpedo Launcher's reach (1,000) meets the window floor (698) but is not derived from it.

- The Mobile Heavy Artillery's reload (11.5 s), from a rule retired 2026-11-25.
- The destroyer's torpedo reach (900), and the escort corvette's shield and charge: step 3.
- Costs, through the ability price list (step 3).

## Battlecruiser: lasers priced (2026-11-26)

The ability price list (step 3, in progress) prices a weapon that cannot be dodged by the damage
shells lose to dodging. Its guns fired as shells at 60 against a T3 fleet that turns back 0.3
seconds after each shell leaves lose 44% of their damage, weighted by cost (battleship 16%,
battlecruiser 35%, cruiser 42%, destroyer 79%, corvette 65%), so the lasers' premium is
√(1 ÷ 0.56) − 1 = 0.34. With today's stats the price list put it at about 3,830 against its 5,302,
so it keeps its cost and its strength rises 1.4×: health 34,421 → 48,189, each laser 610 → 854.

Then the design owner's rules on top (2026-11-26): each laser does at least twice a battleship
turret's damage a second (652 against 326, so 1,304 every 2 seconds), and it sails half again the
battleship's top speed (10.8). Its reach is the navy T3 line's at that speed, 1,586. At its cost of
5,302, the price list leaves health of 31,516: it out-shoots the battleship up close (1,304 a
second against 978) with about a third of its health. It already turns faster (35 against 27,
radius 80 against 100).
