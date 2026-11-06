# Game Design & Content Specification: Planetary Scale RTS

**Companion Document to:** [`SPECIFICATION.md`](file:///Users/codymorterud/scratch/sim-engine/SPECIFICATION.md) (Engine Technical Specification)  
**Target Engine:** Deterministic Fixed-Point Simulation Core (`sim-engine`)

---

## 1. Executive Vision & Core Pillars

### 1.1 The Vision
A grand-scale, flow-economy real-time strategy game inspired by *Supreme Commander: Forged Alliance*. Players command planetary armies ranging from light scouts to colossal multi-screen Experimentals, orchestrating warfare across land, sea, subsurface, air, and orbit on maps spanning up to 81×81 km (6,500+ sq km).

### 1.2 Core Pillars
1. **The Strategic Zoom:** No artificial camera boundaries. The perspective pans and zooms continuously from close-ups just above the ground out to an orbital military theater where units collapse into standardized NATO icons.
2. **The Flow-Based Economy:** An economy governed by continuous streaming rates rather than static stockpiles. Players balance resource inflow versus drain in real-time differential equations.
3. **True Physical Ballistics:** Every bullet, laser bolt, missile, and artillery shell exists as a simulated 3D physical object governed by mass, velocity, gravity, and terrain collision.
4. **Radical Faction Asymmetry:** Four factions built around fundamentally distinct tactical doctrines (heavy kinetic brute force, stealth and ambush cybernetics, extreme doctrinal zealotry and hovering, and durable alien power with death mechanics).
5. **Mass-Scale Wreckage Recycling:** War is funded by the graveyard of past battles. Debris is physical, harvestable, and strategically contested.

---

## 2. The Command Units (ACU & Support Commanders)

### 2.1 The Armored Command Unit (ACU)
The player’s physical avatar on the battlefield. The ACU is a multi-story walking brawler, primary engineer, and high-yield resource generator.

* **Assassination Condition:** Under standard match rules, when an ACU’s health reaches 0, its quantum reactor collapses into a massive nuclear explosion (dealing ~15,000 damage in a 25-meter radius), vaporizing surrounding bases and instantly eliminating the player from the match.
* **Overcharge Cannon:**
  * Consumes a substantial pulse of stored Energy (e.g., 3,000–5,000 Energy per shot).
  * Fires a devastating plasma burst that instantly vaporizes virtually any Tier 1, 2, or 3 mobile unit.
  * Ineffective against Tier 4 Experimentals and structures (deals scaled cap damage).
* **Modular Field Enhancements:**
  The ACU features three physical upgrade slots:
  * **Right Arm:** Heavy offensive upgrades (focused beam lasers, extended range, rapid-fire cannons, or microwave weapons).
  * **Left Arm:** Construction upgrades (expanded Build Power, automated nanite repair drones, engineering suites).
  * **Back / Sub-Systems:** Strategic utility (heavy personal bubble shields, optical cloaking / radar stealth, quantum teleportation drives, or high-yield resource generators).
* **Quantum Teleportation:**
  * Upgraded ACUs and sACUs can instantly teleport anywhere on the map within sensor coverage.
  * *Mechanics:* Requires charging an immense pre-requisite buffer of stored Energy. If energy stalls during the charge cycle, the teleport sequence aborts. Landing on enemy units triggers a "telefrag" that destroys the occupied chassis.
* **Self-Destruct (Ctrl+K):** Initiates a 5-second countdown timer leading to intentional reactor detonation. Used strategically to deny enemy reclaim, trigger tactical suicide nukes, or free up unit capacity.

### 2.2 Support Commanders (sACUs)
Constructed in the late game via the **Quantum Gateway (Tier 3)**:
* Function as autonomous sub-commanders with high baseline Build Power and heavy armor.
* Do not end the game upon death (no game-ending reactor detonation).
* Fully modular with their own faction-specific upgrade trees (combat presets, stealth suites, shield generators, or specialized engineering hives).

---

## 3. The Streaming Economy

The game operates on a continuous rate-of-flow system measured in **Resource / Second**.

```
Net Mass Rate   = Total Mass Inflow - Total Mass Outflow
Net Energy Rate = Total Energy Inflow - Total Energy Outflow
```

```
┌─────────────────────────────────────────────────────────────┐
│                      RESOURCE PIPELINE                      │
│                                                             │
│   INFLOW SOURCES                    DRAIN CONSUMERS         │
│   • Mass Extractors (Mexes)         • Factories / Upgrades  │
│   • Power Generators (PGens)        • Engineers (Assisting) │
│   • Hydrocarbon Vents               • Shield Dome Upkeep    │
│   • Mass Fabricators (Energy->Mass) • Radar / Omni Upkeep   │
│   • Battlefield Reclaim             • Weapons (Artillery)   │
│                                                             │
│             ▲                               ▲               │
│             │                               │               │
│             └───────► [ STORAGE POOL ] ─────┘               │
│                       (Buffer Overflow)                     │
└─────────────────────────────────────────────────────────────┘
```

### 3.1 Resource Types
1. **Mass:** The structural matter used to construct physical chassis, weapons, and buildings. Gathered from fixed Mass deposits, battlefield reclaim, or energy conversion.
2. **Energy:** The operational fuel used to power shields, sensors, weapons, construction nanite streams, and mass fabricators. Gathered from Power Generators and Hydrocarbon Plants.

### 3.2 Storage Buffers & Stalling Penalties
Small baseline storage pools buffer transient rate spikes. When storage hits zero and consumption exceeds production, the empire **stalls**:

* **Mass Stall:** When Mass hits 0, all active construction, factory queues, and upgrades slow down proportionally to match incoming revenue:
  $$\text{Progress Rate} = \frac{\text{Total Inflow}}{\text{Total Demand}}$$
* **Energy Stall:** When Energy hits 0, the consequences are immediate and catastrophic:
  * Factory and engineering build rates slow to a near halt.
  * Radar, Sonar, and Omni sensors go blind (blinking off).
  * Active energy shield domes rapidly bleed power and **completely collapse**, leaving bases defenseless.
* **Production Pausing:** Individual factories, shields, mass fabricators, and construction projects can be manually paused (hotkey toggle) to instantly cut their drain from the network without canceling queued progress.

### 3.3 Mass Fabrication (The Post-Scarcity Transition)
* **Tier 1 & Tier 3 Mass Fabricators:** Stationary structures that continuously convert excess Energy into raw Mass.
* Highly volatile: Explode violently if destroyed while active.
* Allows economies to scale infinitely beyond the physical mass points on the map.

### 3.4 Hydrocarbon Plants
Placed on specialized map vents. Provide high early-game energy output (equivalent to ~10–12 T1 power generators) at a fraction of the build cost, creating high-value early-game territorial objectives.

---

## 4. Base Geometry & Adjacency Networks

Structures built directly touching one another share physical power and resource connections, providing passive efficiency bonuses.

```
   [PGen]                  [Storage]
     │                         │
[Factory] ◄── Energy Discount [Mex] ◄── +12.5% Mass Boost
     │                         │
   [PGen]                  [Storage]
```

* **Factory + Power Generators:** Reduces the continuous Energy cost of unit production by up to 25%.
* **Factory + Mass Storage / Mass Fabricator:** Reduces the Mass cost of unit construction by up to 15%.
* **Mass Extractor + Mass Storage:** Surrounding a Mass Extractor with Mass Storage buildings on all four cardinal edges increases extractor output by **+50%** (+12.5% per storage unit).
* **Power Generator + Energy Shields / Radar:** Reduces the continuous operational Energy upkeep by up to 25%.
* **Power Generator + Heavy Artillery:** Reduces Energy drain per shot and **decreases reload times**, directly increasing the artillery's rate of fire.
* **Volatility Hazard:** Higher-tier generators (T3 PGens) and mass fabricators contain unstable plasma cores. If destroyed, they explode with devastating kinetic force, triggering chain reactions that can vaporize adjacent base structures.

---

## 5. Engineering Dynamics & Production Scaling

### 5.1 Build Power (BP) & The Assisting Mechanic
Every constructor (ACU, Engineer, Factory, Support Commander) has a distinct **Build Power** attribute.
* **Factory Assisting:** Instead of building 20 separate factories, players can command 40 engineers to "Assist" a single factory.
* The factory's production queue drains the economy at the pooled speed of all 41 units combined, printing high-tier units or Experimentals in seconds.
* **Upgrade Vulnerability:** Upgrading a factory or Mass Extractor to the next tech tier temporarily halts its unit production or resource generation until the upgrade finishes.

### 5.2 Factory Automation & Infinite Loops
* **Repeat Mode:** Factories can toggle infinite build queue looping (e.g., repeating a strike group of 5 tanks, 2 flak, and 1 mobile missile).
* **Dynamic Rally Orders:** Rally points can be anchored to friendly units (auto-guard), transport aircraft (auto-ferry), or other factories (auto-assist upon completion).

### 5.3 The Capture Mechanic (Nanite Subversion)
* Engineers, ACUs, and sACUs can fire capture beams onto enemy structures or mobile units.
* If completed without the engineer dying or the target being destroyed, ownership of the asset permanently transfers to the capturing player.
* Allows capturing enemy technology to unlock opposing faction tech trees (e.g., UEF building Cybran stealth shields).

---

## 6. The Wreckage Economy & Denial Tactics

### 6.1 Battlefield Reclamation
Destroyed units, downed aircraft, shattered base buildings, rocks, and trees leave physical 3D wreckage on the terrain.
* **Mass Recovery:** Wreckage retains **80–84% of its remaining health value in Mass**.
* **Graveyard Flipping:** After a major mid-map clash, the victor can move mobile engineers or ACUs into the graveyard to vacuum up thousands of Mass units in moments, immediately funding higher-tier tech or Experimentals.
* **Environmental Reclaim:** Trees provide fast early Energy (and minor Mass); rocks provide pure, dense early Mass.

### 6.2 Wreckage Blasting (Denial of Reclaim)
* Wreckage possesses its own collision box and hit-point pool.
* Players can command tanks or artillery to **Force-Fire (Ground Attack)** on wreckage graveyards.
* Explosive damage vaporizes the wreckage, permanently deleting the mass from the map to prevent the enemy from reclaiming it.

---

## 7. Physics, Ballistics & Weapon Trajectories

### 7.1 Ballistic Simulation
* **True Trajectory:** Shells, plasma charges, missiles, and rockets travel with real muzzle velocity and gravitational drop.
* **Elevation & Masking:** Cliffs, hills, and base walls physically block low-trajectory fire. Artillery placed on ridges gains increased line-of-sight and clearance.
* **Physical Dodging:** Agile units (light assault bots, interceptors) can change heading to evade incoming slow-moving artillery shells or unguided rockets.
* **Friendly Fire:** Stray rounds, premature detonations, and artillery spread inflict full damage on friendly units and shields.
* **Beam Weapons:** Continuous raycast energy vectors (e.g., CzAR death rays or microwave lasers) apply damage instantly along line-of-sight without projectile flight delays.

---

## 8. Multi-Layered Defense Networks

Static defenses must be deployed in coordinated layers to resist high-tier assaults:

| Defense System | Ordnance Targeted | Core Mechanics |
| :--- | :--- | :--- |
| **Point Defense (PD)** | Direct ground fire | High DPS direct-fire turrets (Gauss, pulse lasers). Blocked by terrain. |
| **Anti-Air (AA)** | Aircraft (Interceptors, Gunships, Bombers) | **Flak:** High AoE burst against light clusters.<br>**SAMs:** High-damage tracking missiles targeting heavy air. |
| **Energy Shields** | All ballistic & explosive shells | Projects a 3D hemispherical dome with its own HP pool. Absorbs incoming fire until depleted. Overlapping shields suffer AoE splash bleed-through. |
| **Tactical Missiles & TMD** | Ground installations, shields | **Missiles:** High-burst cruise missiles designed to collapse shields.<br>**TMD:** Precision lasers/counter-missiles shooting down incoming warheads in flight. |
| **Strategic Nukes & SMD** | Map-wide base obliteration | **ICBM:** Enormous nuclear warhead wiping out entire bases.<br>**SMD:** Anti-nuke silos that must manually build and store interceptor missiles. |

### 8.1 Shield Mechanics & Stacking Rules
* **Directional Shield Permeability:** Friendly units inside a shield bubble can freely fire outward. Hostile projectiles coming from outside are blocked by the dome.
* **Shield Overlap Hazard:** Overlapping multiple shield generators does not create invulnerability. Explosive area-of-effect (AoE) shells that strike the outer shield will bleed splash damage into all overlapping inner shield generators simultaneously.

---

## 9. Information Warfare (Sensors & Intel)

Combat occurs far beyond visible optical range. Controlling the intel war dictates weapon accuracy and target acquisition.

```
┌─────────────────────────────────────────────────────────────┐
│                     THE INTEL SPECTRUM                      │
│                                                             │
│  [ OPTICAL LOS ]  Reveals unit models & direct sight lines  │
│         ▲                                                   │
│  [ RADAR / SONAR ] Identifies targets as generic blips      │
│         ▲                                                   │
│  [ RADAR STEALTH ] Masks unit blips from radar/sonar        │
│         ▲                                                   │
│  [ OPTICAL CLOAK ] Complete visual & radar invisibility     │
│         ▲                                                   │
│  [ OMNI SENSORS ] Pierces Stealth, Cloak, and Jamming       │
└─────────────────────────────────────────────────────────────┘
```

* **Line of Sight (LOS):** Direct optical vision. Only units within direct LOS reveal their 3D models and exact health bars.
* **Radar & Sonar:** Identifies surface/air and subsurface targets as generic triangular blips through the Fog of War.
* **Radar Stealth:** Eliminates a unit's radar signature (it produces no blip), but remains visible to optical eyesight or direct scouts.
* **Optical Cloaking:** Completely conceals the unit visually and electronically. Requires proximity or Omni detection to reveal.
* **Radar Jamming:** Deploys dozens of false decoy blips on enemy radar screens to mask true troop numbers and trigger false artillery bombardment.
* **Omni Sensors:** Integrated natively into all ACUs/sACUs and built as Tier 3 Omni Sensor towers. Strips away Stealth, Cloaking, and Jamming within its spherical detection radius.
* **Advanced Orbital & Quantum Optics:**
  * **UEF Novax Center (T3):** Deploys an un-targetable orbital satellite providing high-altitude persistent vision and precision orbital laser fire.
  * **Aeon Eye of Rhianne (T3 Optics Facility):** Drains continuous high Energy to project a movable remote optical sensor probe anywhere on the map through Fog of War.

---

## 10. Multi-Domain Warfare (Air, Sea & Subsurface)

* **Aviation Logistics & Fuel:** Aircraft utilize fuel reserves during continuous flight. Exhausted fuel causes aircraft to lose altitude and crawl at minimum speed. Players build **Air Staging Facilities** where aircraft land to rapidly refuel, rearm, and repair.
* **Subsurface Warfare:** Submarines and underwater structures are immune to surface artillery, plasma cannons, and direct lasers. Combating them requires dedicated Sonar networks, Torpedo Bombers, Sub-hunters, and Naval Depth Charges.
* **Amphibious Dynamics:**
  * **Seabed Walkers:** Heavy tanks and assault mechs walk along the ocean floor, immune to surface fire but vulnerable to torpedoes and depth charges.
  * **Hovercraft:** Skim seamlessly across water and land, bypassing naval choke points to flank shorelines.

---

## 11. The Veterancy System

Units are not static; as they destroy hostile forces, they accumulate experience equal to the Mass value of their targets, progressing through **5 Veterancy Ranks**:

```
[Rank 0: Recruit] ──► [Rank 1] ──► [Rank 2] ──► [Rank 3] ──► [Rank 4] ──► [Rank 5: Elite]
```

* **XP Thresholds:** Each rank requires destroying an amount of enemy mass equal to a multiplier of the unit's own mass cost (typically 1×, 2×, 3×, 4×, 5×).
* **Veterancy Rewards:**
  * **Max Health Increase:** Each rank boosts maximum hit points (up to +100% total HP at Rank 5).
  * **Instant Partial Healing:** Reaching a new rank triggers an immediate percentage health restoration.
  * **Passive Health Regeneration:** Unlocks permanent health regeneration per second, scaling higher with each rank.
* **Commander & Experimental Dominance:** A Rank 5 ACU, Support Commander, or Experimental unit regenerates hundreds of HP per second, turning surviving veteran units into nearly unkillable juggernauts.

---

## 12. Exotic Weaponry, EMP & Status Effects

* **EMP & Stun Weapons:** Cybran EMP missiles, specialized artillery, and Aeon Chrono Dampeners do not deal lethal damage. Instead, they discharge electromagnetic pulses that freeze unit firing mechanisms, disable radar, and shut down shield domes for fixed durations.
* **Gravity Tractor Claws:** The Aeon Galactic Colossus uses localized gravity beams to pick up enemy tanks, hold them immobilized in mid-air, and crush them in its internal maw.
* **Persistent Death Storms:** Upon destruction, the Seraphim Ythotha's containment core collapses into an autonomous **Dark Energy Storm** that roams the battlefield for 60 seconds, shocking and vaporizing any units in its path.

---

## 13. Unit Formations, Movement & Macro Automation

### 13.1 Cohesive Formations
* When a diverse fleet (slow artillery + medium tanks + fast scouts) is given a formation move command, the engine **locks the entire army's speed to the slowest unit**.
* Prevents fast scouts from sprinting ahead and dying piecemeal before heavy fire support arrives.
* Giving a non-formation move order breaks the lock, letting fast units sprint at maximum speed.

**Status:** Today, a group order ends in an orderly grid centered on the goal, but each unit travels to it on its own, at its own speed (§18.4).

**Planned:** Formation travel, where a group keeps its orderly shape all the way to its destination, rather than only forming up on arrival. This builds on the speed lock above.

### 13.2 Automated Patrol Loops
Issuing a `Patrol` order across waypoints turns units into automated patrol guardians:
* **Combat Units:** Patrol the route, automatically engage enemies within sensory range, pursue within a leash radius, and return to the patrol loop.
* **Engineers:** Automatically repair damaged friendly buildings, assist active construction, and vacuum up wreckage debris within their patrol radius.
* **Aircraft:** Patrol air lanes, dogfight hostile air, and automatically divert to the nearest Air Staging Facility when fuel drops below 20%, returning to patrol once refueled.

### 13.3 Advanced Logistics
* **Coordinated Attacks (Time-on-Target):** Orders multiple strike groups from across the map to arrive and strike a target at the exact same synchronized second.
* **Automated Ferry Routes:** Air transports can be assigned continuous ferry loops. Newly constructed ground units walk to the pickup beacon, board transports, fly to the drop zone, and march to the front autonomously.
* **Custom Geometric Build Templates:** Players can save exact base layouts (e.g., optimized factory-pgen-storage rings) and place the blueprint with a single click.

---

## 14. Faction Asymmetry & Iconic Rosters

### 14.1 United Earth Federation (UEF)
* **Design Philosophy:** Industrial, conventional military doctrine. Heavy armor, kinetic Gauss cannons, layered static defense, and heavy frontline attrition.
* **Unique Mechanics:** Resilient mobile shield generators, defensive fortress structures, and naval cruisers equipped with long-range cruise missiles for coastal bombardment.
* **Iconic Experimentals:**
  * **Fatboy (T4 Mobile Factory):** A massive tracked, amphibious fortress with four dual Gauss turrets that manufactures land units on the move and docks repair aircraft.
  * **Mavor (T4 Strategic Artillery):** A colossal fixed artillery piece that fires shells across the entire map with zero shot dispersion (pinpoint accuracy).
  * **Atlantis (T4 Submersible Aircraft Carrier):** A nuclear submarine carrying up to 150 aircraft, shielded from surface detection and naval bombardments while underwater.

### 14.2 Cybran Nation
* **Design Philosophy:** Asymmetric warfare, ambush tactics, cybernetics, radar stealth, lasers, and multi-functional adaptability.
* **Unique Mechanics:**
  * **In-Place Shield Upgrades:** Cybran base shields upgrade sequentially in place through five tiers (ED1 to ED5) instead of requiring new footprints.
  * **Amphibious Walkers:** The *Salem-class* T2 Naval Destroyer sprouts mechanical legs and walks out of the ocean to assault land targets directly.
  * **Engineering Hives:** Static stations deploying multiple automated repair/assist nanite pods.
* **Iconic Experimentals:**
  * **Monkeylord (T4 Spiderbot):** A fast, low-cost walking spider-bot equipped with an internal **Radar Stealth Field** and a focused microwave laser.
  * **Megalith (T4 Heavy Assault Crab):** A colossal amphibious walker armed with dual proton cannons, torpedo launchers, and the ability to fabricate T2/T3 combat units on the field.
  * **Scathis (T4 Mobile Artillery Platform):** An experimental tracked vehicle capable of rapid-fire sustained artillery bombardment.

### 14.3 Aeon Illuminate
* **Design Philosophy:** Religious zealotry, single-purpose unit specialization, weapon precision, and high mobility.
* **Unique Mechanics:**
  * **Extensive Hover Capabilities:** A large portion of their light and support ground forces (light tanks, hover tanks, mobile flak, mobile TMD, and mobile artillery) glide smoothly across water.
  * *Critical Doctrinal Limitation:* Aeon's primary mainline combat brawlers—the **T2 Obsidian Heavy Tank**, **T3 Harbinger Mark IV Heavy Assault Bot**, and **T3 Sprite Striker Sniper Bot**—are tracked or walking units and **cannot hover**.
* **Iconic Experimentals:**
  * **Galactic Colossus (T4 Assault Bot):** A towering, heavily armored monolith armed with optic lasers and gravity tractor beams that vacuum up enemy tanks and crush them.
  * **CzAR (T4 Flying Fortress):** A colossal hovering saucer that acts as an aerial carrier and fires a continuous underside death ray that incinerates everything beneath it.
  * **Paragon (T4 Resource Generator):** Once completed, provides practically infinite Mass and Energy by matching any expenditure rate. Possesses extremely fragile housing and detonates with catastrophic force if destroyed.

### 14.4 The Seraphim
* **Design Philosophy:** Advanced alien invaders featuring high unit costs, massive health pools, advanced energy shields, and battlefield regeneration.
* **Unique Mechanics:** Units feature the highest raw HP and damage per tier, backed by the strongest static energy shields and regenerative auras on their ACU/sACUs. Units like the *Selen* T1 Scout possess passive cloak and radar stealth when stationary.
* **Iconic Experimentals:**
  * **Ythotha (T4 Assault Bot):** A massive bipedal walker armed with heavy energy weapons and an EMP lightning arc. Upon destruction, its unstable reactor spawns a **Dark Energy Storm**—an autonomous, destructive ball of energy that roams the battlefield damaging nearby units.
  * **Ahwassa (T4 Strategic Bomber):** A massive experimental flying wing that drops an orbital-class sub-atomic bomb capable of flattening a T3 base cluster in a single run.
  * **Yolona Oss (T4 Strategic Missile Launcher):** Fires a specialized **Super-Nuke**. Unlike standard nuclear missiles, the Yolona Oss warhead possesses hardened shielding that **requires two (2) separate Strategic Missile Defense (SMD) interceptors** to shoot down, rapidly overwhelming conventional anti-nuke grids.

---

## 15. Tech Tier Progression

```
[ Tier 1: Expansion & Scouting ]
├── Light Assault Bots, Interceptors, Frigates, Light Point Defense
└── Objective: Secure territory, claim mass deposits, scout tech choices
       │
[ Tier 2: Specialization & Containment ]
├── Heavy Tanks, Mobile Shields, Flak AA, Cruisers, Tactical Missiles & TMD
└── Objective: Mass extractor upgrades, shield lines, tactical missile siege
       │
[ Tier 3: Strategic Domination & Siege ]
├── Heavy Assault Bots, Strategic Bombers, Battleships, Heavy Artillery, Omni
└── Objective: Mass fabrication grids, strategic nuclear silos, map control
       │
[ Tier 4: Game-Ending Experimentals ]
└── Colossal walkers, mobile factories, planetary artillery, infinite generators
```

---

## 16. Game Modes & Victory Conditions

1. **Assassination (Standard Competitive Mode):** The match ends when a player’s ACU is destroyed. All structures and units belonging to that player immediately self-destruct or go neutral.
2. **Annihilation:** The match ends only when all enemy mobile units, factories, and combat structures are completely eliminated. Losing an ACU does not end the match.
3. **Supremacy:** A player is eliminated if they lose both their ACU and all Support Commanders or all factories.
4. **Shared Armies & Reclaim Permissions:** Teammates can dynamically transfer units, economic income percentages, or full army control to allies.

---

## 17. Opponents & Multiplayer

* **First Playable:** One human against a scripted AI. The AI issues commands through the same `SimCommand` interface as a player and sees only what its team's intel allows.
* **Automated Matches:** AI-vs-AI matches run headlessly, serving as full-match regression tests and as the first tool for balance tuning.
* **Multiplayer (Later):** Lockstep over the network, exchanging only commands and comparing checksums to detect desyncs. Matchmaking, lobbies, and transports are not yet specified.

---

## 18. Vertical Slice (First Playable Scope)

The first playable build proves the core loop before the full design is attempted. It runs on the [engine specification](SPECIFICATION.md) after its PoC gates pass.

### 18.1 Included
* **Factions:** Two mirrored factions, identical apart from color. Asymmetry comes later.
* **Roster:** About 10 land-only units and structures:

| Entry | Role |
| :--- | :--- |
| ACU | Commander, engineer, and assassination target |
| T1 Engineer | Builds, assists, and reclaims |
| T1 Light Assault Bot | Cheap early combat |
| T1 Light Artillery | Slow-shell indirect fire that can be dodged |
| T2 Heavy Tank | Mainline combat |
| Land Factory (T1, upgradable to T2) | Unit production with repeat mode |
| Mass Extractor | Mass income on fixed deposits |
| Power Generator | Energy income |
| Mass Storage | Storage and extractor adjacency bonus |
| T1 Point Defense | Static direct-fire defense |

* **Systems:** Flow economy with stalling (§3.1–3.2), Mass Extractor and factory adjacency (§4), Build Power and assisting (§5.1), wreckage and reclaim (§6.1), ballistics with terrain masking and friendly fire (§7.1), line of sight with fog of war (§9), and Assassination mode (§16).
* **Interface:** Selection, move, attack, patrol, build placement, a build menu, resource rates and storage, unit health bars, and NATO-style icons at far zoom.
* **Presentation:** Placeholder geometry and no required audio.

### 18.2 Deferred
Naval, subsurface, and air warfare (§10); shields and missile defense (§8); radar, stealth, and cloaking (§9); capture (§5.3); veterancy (§11); exotic weapons (§12); Time-on-Target, ferry routes, and build templates (§13.3); T3 and T4; faction asymmetry (§14); ACU upgrades and teleportation (§2.1); and the remaining game modes (§16).

### 18.3 Exit Criteria
* A human can win or lose a full match against the AI.
* 100 headless AI-vs-AI matches finish without a crash or checksum mismatch when replayed. Check with `cargo run --release -- --soak 100`.
* The engine's tick and frame-rate gates still pass during a late-game match. Check the frame rate with `cargo run --release -- --autotest 60 --fast-forward 10000`.

### 18.4 Implementation Decisions
These are starting points for tuning, recorded because they interpret or adjust the design above.

* **Scale:** One terrain cell (20 units) is the game's basic grid square. Unit sizes, speeds, and ranges in `data/units.toml` are set in proportion to it, and the skirmish map is 10 km across.
* **Commander Explosion:** §2.1 gives a 25-meter radius, which is barely more than one grid square and could not "vaporize surrounding bases". The slice uses 300 units, as a data value.
* **Friendly Fire:** A shell ignores allies until it reaches its aim point, so that rear ranks can fire through front ranks. Rounds that overshoot can hit allies, and splash damages everyone, which keeps the stray rounds and artillery spread of §7.1.
* **Fog of War:** Sight is a simple radius, not blocked by terrain. Units cannot target what their team cannot see. Structures are not remembered once out of sight.
* **Upkeep:** Nothing in the slice has an energy upkeep, so an energy stall only slows building. Sensor and shield effects from §3.2 arrive with those systems.
* **Orders During Construction:** A factory takes queue, repeat, rally, and upgrade orders while it is under construction. They wait at no cost, and production starts once it is built.
* **Work Beams:** A builder faces what it is working on, joined to it by an animated beam, which covers building, assisting, repairing, and reclaiming. A team with nothing to spend shows no beams, so a beam always means progress. This is presentation only, and not part of the checksum.
* **Control Groups:** Ctrl or Command plus a number saves the selection, and with nothing selected clears that group. The number alone selects it. A bar on the left lists the groups in use and what is in them, and clicking one selects it. Dead units drop out.
* **Turning:** Each mobile unit has a turn rate, in degrees per second, and may have a turn radius, in units. A unit only moves forward along its facing. With a turn radius it turns like a car: only while moving, in an arc no tighter than its radius, slowing to half speed in a tight turn but never stopping, so a U-turn swings out to the side. A goal inside the circle it would turn in cannot be reached by driving on, so it pivots slowly on the spot instead, as a tracked vehicle would. Without a turn radius it turns on the spot, as a walker does. The commander's legs and the engineer's hull follow the facing, while the body and arm above turn freely to face their work. Values: commander 180 degrees a second on the spot, engineer 240 with a 10-unit radius, light bot 360 on the spot, light artillery 120 with 30, heavy tank 90 with 40. Stress test units turn instantly. Planned: naval units turn slowly, and air units cannot stop or turn on the spot, so they must fly in arcs and circle; their turn rate matters most.
* **Turrets:** Each weapon has a turn rate, in degrees per second, and swivels towards its target by at most that much each tick. It only fires when it points within about 6 degrees of the target. Without a target it turns back over the hull. The commander's gun arm, the bot's upper body, the tank's turret, the artillery's barrel, and the point defense's gun are drawn from it. Rates: commander 180, light bot 300, light artillery 90, heavy tank 60, point defense 120. Stress test units fire in any direction at once.
* **Unreachable Orders:** An order that cannot be reached is cancelled as soon as a unit's up-to-date flow field shows no way from its cell to the goal. That covers walls and structures today, and impassable mountains, water for land units, and land for ships once those exist, since each is only more cells a unit cannot cross. Builders drop the task, patrols end, queued units move on, and the player is told. A goal that can be reached by a long way round is never cancelled.
* **Moving Orders:** While Shift is held, each order that can move has an icon, and dragging it puts the order somewhere else, for every unit sharing it. Moves and rally points can go anywhere. Builds follow the usual placement rules, so an extractor still snaps to a free deposit, and a build dropped where it cannot go stays where it was. A site that has started, a patrol, an attack, and a follow order cannot be moved.
* **Selection Rates:** The command panel shows what a selected structure or commander makes, including adjacency bonuses, and what a factory is really spending, including help and stalls.
* **Queued Orders:** Shift queues an order after what the selection is already doing, in any mix of moves, attack-moves, builds, assists, repairs, and reclaims. A group shares one route per queued order, and forms up at each stop. A builder finishes a walk it is on before starting what was queued, even if that job is already within reach. A queued attack is an attack-move to where the target was, and follow orders and patrols have no end to wait for: following is not queued, and queuing an order ends a patrol.
* **Quick Select Bar:** A bar on the right selects the commander or factories from anywhere, without moving the view. Factories are grouped by kind, then tier, with counts. A tier selects the factory closest to the center of the view, a double-click selects all of them, and Shift adds to the selection. Factories still being built count, as they take orders.
* **Sound:** Generated in code, and never read by the simulation. Picking units by any means gives a short tick, including picking the same units again. Builders make a light hum and factories a slightly harsher buzz, only for work in view when zoomed in, and only while work is really happening. An alarm and a text alert show when the commander takes damage, then wait 30 seconds, wherever the camera is.
* **Unit Order Bar:** Anything that can walk shows Move, Attack-move, Patrol, Assist, and Stop in the command panel. Assist means staying with a friendly unit and firing at what comes into range, without chasing it. Builders also lend their build power if the unit has work that can use it. An armed order is shown on its button, and is cancelled by right-click or Esc.
* **Command Panel Hotkeys:** Every option in the command panel shows a key, which does the same as its button. Each option has one key whatever is selected, so that a builder and a factory selected together never compete for a key. Keys are mostly on the left of the keyboard, because the other hand is on the mouse.
* **Queuing Ahead of an Upgrade:** Once a factory has an upgrade queued, it also takes what the upgrade unlocks. New items go to the end of the queue, so they can only come out after the upgrade has finished. Removing the upgrade removes what depended on it.
* **Removing Queued Orders:** The command panel lists the selection's queue, and right-clicking an item removes it. Orders are named rather than numbered, because a queue can move on between the click and the command arriving. A site that was already started stays where it is, without a refund, and can be finished by assisting it. A factory loses its last matching item, so that work under way is only lost if it is the only one.
* **Arrival Formations:** A group ordered to a location forms a grid about twice as wide as it is deep, centered on the goal and lined up across the direction of travel. Units follow a shared route, then walk to their own spots, with a time limit so that a blocked spot cannot trap a unit.
* **Production Pausing:** A factory or construction site can be paused with O or the panel button, and resumed the same way. It spends nothing and makes no progress until resumed, keeps its queue and progress, and builders helping it wait beside it. Shields and mass fabricators will pause the same way when they exist.
* **Overcharge:** The commander's Overcharge spends 3,000 stored energy on one shot of 12,000 damage, which destroys any current mobile unit, but at most 800 to a structure. It has its own 5-second reload, and fires through the commander's gun arm, so it has to aim first. Ordered with L and a click on an enemy, the commander closes to range and fires as soon as it has the energy, and the order ends if the target dies. Values are in the unit data. The AI does not use it yet.
* **Veterancy:** The unit that destroys an enemy earns its mass as experience, or as much of it as had been built, and splash damage counts. Nothing is earned for destroying its own side. Rank n is reached at n times the unit's own mass cost, up to rank 5. Each rank adds 20% of base health, so rank 5 doubles it, heals 25% of the new maximum at once, and adds regeneration of 0.2% of maximum health a second, so a rank 5 commander heals about 120 a second. Stress test units have no mass, so they earn nothing.
* **Blocked Shots:** A unit does not fire at a target it picked for itself if the ground would stop the shell, which it checks by flying the shell's arc against the terrain. It passes over that target for 5 seconds, so it looks for another or walks on, and an attack-move routes around the hill or wall. A target the player ordered is fired at regardless.
* **AI Waves:** A wave grows by a quarter each time, up to 50 units, but an army that has not grown for a minute goes with what it has, at least 10 units, since a stalled economy may never field the next wave. With nothing in sight, a full-size wave attacks the enemy's base, and smaller waves mostly raid deposits more than 2,000 units from it, away from its defenses. Between equal AIs on a fair map, a defended base beats an equal wave, so without raids to open up a lead, some matches went on past 67 minutes. Gathering each wave short of the enemy's base before attacking was tried and made this worse.
* **Factory Exits:** A finished unit comes out of the factory side that faces its rally point, or the map center if there is no rally. When two sides face it equally, the one clockwise of it wins.
* **A Fair Skirmish Map:** The skirmish map turns into itself about its center, and the AI plays it that way. It lays out its base from Blue's side, then turns each site for Red, footprint included. Both commanders start at the center of a terrain cell, never on a cell edge, because a point on an edge belongs to the cell above and to the right of it, so its mirror image would be in the wrong cell. Both AIs think on the same tick, since a side that always thought later would always be that far behind, and they draw the same random numbers. Until the armies meet, Red plays Blue's game turned around. What is left is noise: fixed-point rounding is not symmetric under negation, and paths of equal cost can bend differently on each side.
* **Left for Later:** Trees and rocks as reclaim (§6.1), and formation travel (§13.1) are not in the slice.

---

## 19. Data & Simulation Requirements

* **Unit Data:** Unit and structure stats live in versioned data files, not code, using fixed-point-safe values and loaded identically on every instance. Each entry defines at least cost (Mass, Energy, Build Time), health, speed, footprint, movement class, build options, weapons, intel ranges, and resource rates.
* **Economy Precision:** Economy totals, storage, and veterancy XP use 64-bit fixed-point, because their values can exceed the range of the engine's `Fp` type. When demand exceeds supply, resources are shared among consumers using integer math in a fixed order, so stalling stays deterministic.
* **Large Footprints:** Pathfinding for wide units needs cost grids inflated by unit size, in addition to one grid per movement class. Experimentals cannot be added without this.
* **Open Numbers:** Values this document leaves undefined must be set in the data files before their feature is built. Examples include the exact energy-stall build rate, the veterancy regeneration curve, and all T1–T4 unit stats.

---

## 20. Naming & Licensing

Faction, unit, and structure names in §2 and §14 are working references taken from *Supreme Commander: Forged Alliance*. They must be replaced with original names, and designs reviewed for similarity, before any public release. Code and data files use neutral identifiers (for example `faction_a`, `t2_heavy_tank`) so that renaming does not touch game logic.
