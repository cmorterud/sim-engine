# Game Design & Content Specification: Planetary Scale RTS

**Companion Document to:** [`SPECIFICATION.md`](file:///Users/codymorterud/scratch/sim-engine/SPECIFICATION.md) (Engine Technical Specification)  
**Target Engine:** Deterministic Fixed-Point Simulation Core (`sim-engine`)

---

## 1. Executive Vision & Core Pillars

### 1.1 The Vision
A grand-scale, flow-economy real-time strategy game inspired by *Supreme Commander: Forged Alliance*. Players command planetary armies ranging from light scouts to colossal multi-screen Experimentals, orchestrating warfare across land, sea, subsurface, air, and orbit on maps spanning up to 81×81 km (6,500+ sq km).

### 1.2 Core Pillars
1. **The Strategic Zoom:** No artificial camera boundaries. The perspective pans and zooms continuously from millimeter-scale close-ups out to an orbital military theater where units collapse into standardized NATO icons.
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
