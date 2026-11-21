# Artillery lobs its shells, 2026-11-21

Every artillery shell now flies in a high arc, rising two fifths of the distance across, and comes down where it was aimed: where its target will be, as every shell leads (GAME_SPECIFICATION.md §7, Artillery). Its flight takes as long as before, so its lead and timing are unchanged. Before, shells flew almost flat, a couple of dozen units up, and could skim past what they were aimed at and on beyond; now they land, and with their target gone they burst on the ground there.

Shells that land made artillery stronger. With the unit data as it was, T3 artillery kept 0.85 of its army more than Heavy Walkers attack-moving into it, against a target of 0.4 to 0.7, and the T1 cells moved:

| Cell | Target | Before | With arcs |
|---|---|---|---|
| 4 Light Walkers charging 2 Light Artillery | raiders win | +0.25 | -0.21 |
| massed Light Walkers against Light Artillery, with micro | raiders keep at least 0.3 | +0.62 | +0.08 |
| Medium Tanks against Light Artillery, with micro | tanks keep at least 0.3 | +0.61 | +0.78 |

Decisions, with the reasons in data/balance_targets.toml:

* Raiders are no longer one of artillery's counters without micro, nor a micro cell. A few small units charging artillery from the front die on the way, which is right; raiders are for artillery left alone and caught close. Artillery's counters without micro are other artillery, mobile shields, heavy units that walk through its fire, and bombers. T1, before mobile shields, is the thin spot, to watch in the soak.
* The Mobile Missile Launcher is priced at 0.8 of T1 artillery's strength per materiel (`[missiles]`), not by a duel against it, which arcs moved by half (from 0.83 strength to 1.20) for a reason that had nothing to do with it. Artillery is the better unit: missiles miss what keeps moving, and missile defense stops them. At 1.2 of T1 artillery's strength, as its drawbacks first suggested, Heavy Tanks walking into launchers came out even and the launchers broke a T2 position in 20 seconds: their reach, more than twice T1 artillery's, outranges point defenses, which is worth more than their drawbacks cost them. The siege's length could not set the share: twelve launchers break that position in a few volleys at every strength tried, so their reload sets it.
* Missile launchers that pick their own targets look over their whole reach, for structures first, then units that have stopped, and only then what is moving.
* Everything else is re-derived against the same targets. T1 came out unchanged; T3 artillery re-priced to 2,305 health and 859 a shell, a quarter less of each.

## T1

```
T1 land, first pass from data/balance_targets.toml (anchor efficiency 2.806 a materiel):
  line       strength 1.00  Medium Tank: 40 materiel, 420 health, 30.0 damage a second, range 350, speed 7.2, sight 385; time to kill a copy 14.0 s
             mass = 40, energy = 200, build_time = 240, health = 420, speed = "7.2", radius = 9, vision = 385
             weapon = { range = 350, reload_ticks = 10, projectile_speed = 64, damage = 30, splash_radius = 0, spread = 0 }
  raider     strength 0.84  Light Walker: 24 materiel, 150 health, 21.4 damage a second, range 210, speed 13.0, sight 335; time to kill a copy 7.0 s
             mass = 24, energy = 120, build_time = 144, health = 150, speed = "13", radius = 7, vision = 335
             weapon = { range = 210, reload_ticks = 5, projectile_speed = 42, damage = 10.7, splash_radius = 0, spread = 0 }
  artillery  strength 0.83  Light Artillery: 48 materiel, 451 health, 28.0 damage a second, range 595, speed 5.8, sight 385; time to kill a copy 16.1 s
             mass = 48, energy = 240, build_time = 288, health = 451, speed = "5.8", radius = 9, vision = 385
             weapon = { range = 595, reload_ticks = 50, projectile_speed = 28, damage = 140.1, splash_radius = 16, spread = 20, artillery = true, min_range = 298 }
  scout      strength 0.50  Land Scout: 12 materiel, 40 health, 7.1 damage a second, range 210, speed 14.4, sight 695; time to kill a copy 5.6 s
             mass = 12, energy = 60, build_time = 72, health = 40, speed = "14.4", radius = 6, vision = 695
             weapon = { range = 210, reload_ticks = 10, projectile_speed = 53, damage = 7.1, splash_radius = 0, spread = 0 }

Counter-matrix cells, equal materiel (480), mirrored, score for the first:
  line against raider:                +0.58 (line keeps 0.4-0.7)
  line against artillery, attack-move: -0.58 (artillery keeps 0.4-0.7)
  line against artillery, with micro:  +0.78 (line keeps at least 0.3)
  massed raiders against artillery:   -0.14 attack-moving, +0.08 with micro (artillery beats raiders; micro helps)
  4 raiders charging 2 artillery:     -0.21 (artillery may win: raiders are no counter of it)
  ten anchors against ten:            27.2 s (targets: 25-45 s)
```

## T2

```
T2 land from data/balance_targets.toml, against T1 land frozen:
  line       strength 1.02  Heavy Tank: 120 materiel, 1381 health, 85.7 damage a second, range 438, speed 6.2, sight 480; time to kill a copy 16.1 s
             mass = 120, energy = 600, build_time = 720, health = 1381, speed = "6.2", radius = 11, vision = 480
             weapon = { range = 438, reload_ticks = 12, projectile_speed = 80, damage = 102.9, splash_radius = 0, spread = 0 }
  skirmisher strength 0.75  Grenadier Walker: 144 materiel, 1131 health, 80.8 damage a second, range 529, speed 6.2, sight 555; time to kill a copy 14.0 s
             mass = 144, energy = 720, build_time = 864, health = 1131, speed = "6.2", radius = 10, vision = 555
             weapon = { range = 529, reload_ticks = 20, projectile_speed = 44, damage = 161.6, splash_radius = 19.25, spread = 10 }
  assault    strength 0.88  Assault Hovercraft: 240 materiel, 3129 health, 111.8 damage a second, range 438, speed 7.2, sight 415; time to kill a copy 28.0 s
             mass = 240, energy = 1200, build_time = 1440, health = 3129, speed = "7.2", radius = 13, vision = 415
             weapon = { range = 438, reload_ticks = 16, projectile_speed = 80, damage = 178.8, splash_radius = 10, spread = 4 }
  artillery  strength 0.67  Mobile Missile Launcher: 180 materiel, 1353 health, 84.0 damage a second, range 1295, speed 5.8, sight 650; time to kill a copy 16.1 s
             mass = 180, energy = 900, build_time = 1080, health = 1353, speed = "5.8", radius = 11, vision = 650
             weapon = { range = 1295, reload_ticks = 50, projectile_speed = 40, damage = 420.1, splash_radius = 10, spread = 0, artillery = true, min_range = 259 }

Counter-matrix cells, equal materiel (1440), mirrored, score for the first:
  T2 line against T1 line, efficiency: 1.07 (targets: 1.0-1.15)
  T2 line against T1 line, score:      +0.21
  skirmisher against line:            +0.43 (skirmisher keeps 0.4-0.7)
  assault against line:               -0.11 (even, within 0.15)
  assault against skirmisher:         +0.09 (about even, assault ahead)
  siege's strength over T1 artillery's: 0.80 ([missiles] strength_over_artillery)
  12 siege against a shield over three T2 point defenses (2220 materiel): broken in 21 s, losing 0% (targets: within 60 s, nothing lost)
  T1 artillery against siege:         +0.24 (artillery is the better unit)
  line against siege, attack-move:    +0.37 (what keeps moving wins)
  a raid, 6 T1 raiders against 1 siege: +0.75 (raiders win)
  a raid, 6 T1 raiders against 1 skirmisher: -0.91 (its splash shreds a clump of raiders)
  massed T1 raiders against skirmishers, with micro: -0.36
  line against skirmisher, with micro: +0.50 (line keeps at least 0.3: a micro cell)
  assault against siege, attack-move: +0.57
  skirmisher against siege:           +0.33
  ten T2 line against ten:            34.5 s (targets: 30-50 s)

Strength per materiel against T1 land (above 1, the T2 unit is the better buy):
  Heavy Tank               Medium Tank 1.07, Light Walker 1.36, Light Artillery 0.82
  Grenadier Walker         Medium Tank 1.92, Light Walker 2.99, Light Artillery 1.79
  Assault Hovercraft       Medium Tank 1.29, Light Walker 1.67, Light Artillery 1.24
  Mobile Missile Launcher  Medium Tank 1.00, Light Walker 0.55, Light Artillery 0.93
```

## T3

```
T3 land from data/balance_targets.toml, against T1 and T2 land frozen:
  line       strength 1.00  Heavy Walker: 360 materiel, 4292 health, 235.7 damage a second, range 525, speed 6.1, sight 580; time to kill a copy 18.2 s
             mass = 360, energy = 1800, build_time = 2160, health = 4292, speed = "6.1", radius = 13, vision = 580
             weapon = { range = 525, reload_ticks = 3, projectile_speed = 95, damage = 70.7, splash_radius = 0, spread = 0 }
  heavy      strength 0.98  Siege Walker: 720 materiel, 11010 health, 357.5 damage a second, range 578, speed 4.3, sight 520; time to kill a copy 30.8 s
             mass = 720, energy = 3600, build_time = 4320, health = 11010, speed = "4.3", radius = 15, vision = 520
             weapon = { range = 578, reload_ticks = 40, projectile_speed = 96, damage = 1429.9, splash_radius = 12, spread = 0 }
  artillery  strength 0.34  Mobile Heavy Artillery: 600 materiel, 2305 health, 143.2 damage a second, range 1680, speed 4.3, sight 1010; time to kill a copy 16.1 s
             mass = 600, energy = 3000, build_time = 3600, health = 2305, speed = "4.3", radius = 12, vision = 1010
             weapon = { range = 1680, reload_ticks = 60, projectile_speed = 56, damage = 859.1, splash_radius = 22.75, spread = 25, artillery = true, min_range = 420, setup_ticks = 10 }

Counter-matrix cells, equal materiel (3600), mirrored, score for the first:
  T3 line against T2 line, efficiency: 1.07 (targets: 1.0-1.15)
  T3 line against T2 line, score:      +0.23
  heavy against line:                 -0.06 (even, within 0.15)
  artillery's reach over the heavy point defense's: 1.20 (at least 1.15)
  16 artillery against a heavy shield over three heavy point defenses (9300 materiel): broken in 26 s, losing 0% (targets: within 60 s, nothing lost)
  line against artillery, attack-move: -0.50 (artillery wins)
  line against artillery, with micro:  +0.68 (line wins)
  heavy against artillery, attack-move: +0.31
  T2 assault against T3 artillery:     +0.17
  a raid, 25 T1 raiders against 1 artillery: +0.70 (raiders win)
  ten T3 line against ten:            44.2 s (targets: 35-60 s)

Strength per materiel against T2 and T1 land (above 1, the T3 unit is the better buy):
  Heavy Walker             Heavy Tank 1.07, Grenadier Walker 1.00, Assault Hovercraft 1.20, Mobile Missile Launcher 1.18, Medium Tank 1.13, Light Artillery 1.23
  Siege Walker             Heavy Tank 1.67, Grenadier Walker 2.00, Assault Hovercraft 1.37, Mobile Missile Launcher 1.32, Medium Tank 2.13, Light Artillery 3.17
  Mobile Heavy Artillery   Heavy Tank 1.02, Grenadier Walker 2.28, Assault Hovercraft 0.82, Mobile Missile Launcher 1.47, Medium Tank 0.10, Light Artillery 1.50
```

In the playtest scenarios, six T3 artillery standing alone now kill about half of an equal materiel of Medium Tanks charging them, against three quarters before: each shell still kills a tank and its neighbours, but the artillery needs a line in front of it to win. With seven Heavy Walkers in front of two, the army keeps about two thirds against 90 Medium Tanks.
