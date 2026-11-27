# How balance works

Most strategy games are balanced by hand: someone changes a number, people play, and it changes again. In sim-engine, a unit's stats and cost come out of a small mathematical model instead. The designer sets a few dozen targets, such as "a tank kills a copy of itself in 14 seconds", and every other number is worked out from them. No number is copied from another game or fitted to make an old value come out right.

That means you can read why a unit costs what it does, and a new unit can be priced before anyone has played it.

This page is the short version. [`BALANCE.md`](../BALANCE.md) is the long one, and [`data/balance_targets.toml`](../data/balance_targets.toml) holds every target and constant, with a comment on each.

## 1. One anchor unit

Everything is measured against one unit, the T1 Medium Tank:

| | |
| --- | --- |
| Health | 420 |
| Damage a second | 30 |
| Cost | 40 materiel |
| Range | 350 |

Its health over its damage a second is 14 seconds: the time it takes to destroy a copy of itself. That one number sets the pace of the whole game.

## 2. Strength and cost

Two groups that fight by aiming at each other follow Lanchester's square law: a group's fighting strength grows with its health times its damage. So a unit's strength is

    strength = √(health × damage a second)

and a fair cost is proportional to strength. For the tank, √(420 × 30) = 112.2, and 112.2 ÷ 2.806 = 40 materiel. The 2.806 is the anchor's strength per materiel, and every unit is priced on the same scale.

## 3. How long fights last

Each unit's time to kill comes from its tier, its domain and its role:

    time to kill = 14 s × tier × domain × role

| | Factor |
| --- | --- |
| Tier | T1 1.0, T2 1.15, T3 1.3 |
| Domain | land 1.0, navy 3.0, aircraft against aircraft 0.65 |
| Role | line 1.0, sub hunter 0.4, and so on |

So sea battles last about three times as long as land battles, higher tiers are tougher for their damage, and a role decides how its strength splits between health and firepower.

## 4. Range from speed

A unit's reach is the distance an enemy covers while closing in, as a share of the fight:

    range = closing share × speed × time to kill

For the tank, 0.35 × 72 × 14 ≈ 350. Faster units and longer fights mean longer reach. Artillery, point defenses and missiles hang off the same numbers: artillery outranges its tier's line units by a fixed multiple, a point defense reaches a little short of artillery, and a missile reaches twice the cannon of its role.

## 5. An ability price list

Strength is not the whole story. A unit that outranges its enemies, moves faster, hits a crowd at once, or can't be shot by cannons is worth more than its raw strength. Each of those is a price, a percentage added to its cost:

| Ability | Price |
| --- | --- |
| Reach past the line's | grows with how far past |
| Speed | 0.3 × (1 − line speed ÷ its speed) |
| Splash | √n − 1, where n is how many times its listed damage one shot does to a formation, measured in the simulation |
| Can't be dodged (lasers) | 0.34, from how much shell fire a fleet dodges, measured |
| Submerged, out of cannons' reach | 0.65 at T1 |
| Hits more than one kind of target | 0.41 for a torpedo, which hits ships and submarines |

Then the full formula is

    cost = Σ over each kind of target it fights of √(health × damage a second) ÷ σ
    σ = 2.806 × domain base ÷ (1 + its prices)

The domain base puts ships and aircraft on the same scale as land units.

## 6. Checked in the simulation

The model makes predictions, and a headless, deterministic harness checks them. It runs equal-cost fights between every pair of units, sieges against defended positions, and fights between land, sea and air, and scores each from −1 to 1 by the share of materiel left. If a cheaper unit wins a fight it should lose, the test fails. These run on every push to `main`.

A few rules are measured rather than calculated, because only the simulation knows them: how much damage splash does to a real formation, how many shells a turning ship dodges, how long one anti-air unit takes to bring down a bomber.

## Where it stands

The navy, the T2 and T3 aircraft, and the land units' reach and fight times come from the model today. Land costs are within about 10% of the price list, and the solver's targets hold them. T1 aircraft, protection like shields, and some special units are still being worked through, and [`PROVENANCE.md`](../PROVENANCE.md) records which numbers are still starting values and which have been replaced. The goal is that every unit, including new experimental ones, can be priced from the model alone.
