//! Application entry point, window loop, and mode switch (headless vs windowed).
#![allow(clippy::disallowed_types)]

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use sim_engine::driver::{CommandSource, DriverResult, SimDriver, TICK_DURATION};
use sim_engine::input::selection::{order_command, Selection};
use sim_engine::math::fp::Fp;
use sim_engine::render::camera::{Camera, HeightField, MIN_ALTITUDE};
use sim_engine::render::instances::{self, UnitInstanceRaw};
use sim_engine::render::pipeline::{FrameScene, Renderer};
use sim_engine::sim::components::Team;
use sim_engine::sim::config::TERRAIN_CELL_UNITS;
use sim_engine::sim::scenario::{self, advance_scripted};
use sim_engine::sim::terrain::HEIGHT_FRAC_BITS;
use sim_engine::sim::world::SimSnapshot;
use winit::dpi::{LogicalSize, PhysicalPosition};
use winit::event::{ElementState, Event, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::WindowBuilder;

const DEFAULT_SEED: u64 = 0x5EED_2026;
const PLAYER_TEAM: Team = Team::Blue;
/// Frames ignored at the start of a test run, while shaders and buffers warm up.
const WARMUP_FRAMES: usize = 120;
/// At 60 Hz, a frame interval this long means a vsync was missed. Intervals near
/// the nominal 16.67 ms carry timestamp jitter, so they cannot be compared directly.
const DROPPED_FRAME_MS: f64 = 25.0;
/// Height of each wall segment. Deep enough to reach below the terrain's raised samples.
const OBSTACLE_DRAW_HEIGHT: f64 = 18.0;

const USAGE: &str = "\
Usage: sim-engine [options]

  (no options)         Play: 2,500 Blue vs. 2,500 Red
  --headless <ticks>   Run scripted orders with no window and print the checksum
  --autotest <secs>    Run the same scripted orders in a window, sweep the zoom,
                       print frame times, and print the checksum
  --capture <dir>      With --autotest, save screenshots at several zoom levels
  --seed <number>      Scenario seed

Controls: drag to select, right-click to move, Alt+right-click to attack-move,
scroll to zoom, WASD or arrow keys to pan, Esc to clear the selection.";

#[derive(Default)]
struct Options {
    headless_ticks: Option<u32>,
    autotest_seconds: Option<u32>,
    capture_dir: Option<PathBuf>,
    seed: Option<u64>,
}

fn parse_options() -> Result<Options, String> {
    let mut options = Options::default();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let mut value = |name: &str| args.next().ok_or_else(|| format!("{name} needs a value"));
        match arg.as_str() {
            "--headless" => options.headless_ticks = Some(value("--headless")?.parse().map_err(|_| "--headless needs a tick count")?),
            "--autotest" => options.autotest_seconds = Some(value("--autotest")?.parse().map_err(|_| "--autotest needs a number of seconds")?),
            "--capture" => options.capture_dir = Some(PathBuf::from(value("--capture")?)),
            "--seed" => options.seed = Some(value("--seed")?.parse().map_err(|_| "--seed needs a number")?),
            "--help" | "-h" => return Err(USAGE.to_string()),
            other => return Err(format!("unknown option {other}\n\n{USAGE}")),
        }
    }
    Ok(options)
}

fn main() {
    env_logger::init();
    let options = match parse_options() {
        Ok(options) => options,
        Err(message) => {
            println!("{message}");
            return;
        }
    };
    let seed = options.seed.unwrap_or(DEFAULT_SEED);
    if let Some(ticks) = options.headless_ticks {
        run_headless(seed, ticks);
        return;
    }
    run_windowed(seed, options.autotest_seconds, options.capture_dir);
}

fn run_headless(seed: u64, ticks: u32) {
    let scenario = scenario::default_battle(seed);
    let opening = scenario.opening_commands;
    let mut world = scenario.world;
    let started = Instant::now();
    for _ in 0..ticks {
        advance_scripted(&mut world, &opening, seed);
    }
    println!(
        "headless: tick {} checksum {:016x} ({} units, {} projectiles, {:.2} ms per tick)",
        world.current_tick,
        world.compute_checksum(),
        world.units.len(),
        world.projectiles.len(),
        started.elapsed().as_secs_f64() * 1000.0 / f64::from(ticks.max(1)),
    );
}

/// What a test run measures and which screenshots it still owes.
struct Autotest {
    duration: Duration,
    started: Option<Instant>,
    frame_times: Vec<Duration>,
    /// Frame time with the wait for a drawable removed: the work this program actually does.
    work_times: Vec<Duration>,
    /// Frame time paired with altitude, for finding which zoom levels are slow.
    frames_by_altitude: Vec<(f64, Duration)>,
    capture_dir: Option<PathBuf>,
    capture_points: Vec<f64>,
    peak_projectiles: usize,
    peak_instances: usize,
}

impl Autotest {
    fn new(seconds: u32, capture_dir: Option<PathBuf>) -> Self {
        Self {
            duration: Duration::from_secs(u64::from(seconds)),
            started: None,
            frame_times: Vec::new(),
            work_times: Vec::new(),
            frames_by_altitude: Vec::new(),
            capture_dir,
            // Progress through the run at which to take screenshots.
            capture_points: vec![0.02, 0.15, 0.22, 0.30, 0.42, 0.50],
            peak_projectiles: 0,
            peak_instances: 0,
        }
    }

    /// Zooms from the minimum to the maximum altitude and back over the whole run.
    fn altitude_at(progress: f64, max_altitude: f64) -> f64 {
        let sweep = 1.0 - (progress * 2.0 - 1.0).abs();
        (MIN_ALTITUDE.ln() + (max_altitude.ln() - MIN_ALTITUDE.ln()) * sweep).exp()
    }

    fn report(&mut self, result: DriverResult) {
        let mut times = self.frame_times.clone();
        times.sort_unstable();
        if times.is_empty() {
            println!("autotest: no frames recorded");
            return;
        }
        let at = |percent: usize| times[(times.len() * percent / 100).min(times.len() - 1)].as_secs_f64() * 1000.0;
        let total: f64 = times.iter().map(Duration::as_secs_f64).sum();
        let average_fps = times.len() as f64 / total;
        let dropped = times.iter().filter(|time| time.as_secs_f64() * 1000.0 > DROPPED_FRAME_MS).count();
        let mut work = self.work_times.clone();
        work.sort_unstable();
        let work_p99 = work[(work.len() * 99 / 100).min(work.len() - 1)].as_secs_f64() * 1000.0;
        let verdict = |passed: bool| if passed { "PASS" } else { "FAIL" };
        println!("autotest: {} frames over {total:.1} s", times.len());
        println!("  average FPS {average_fps:.1}   limit 59.5: {}", verdict(average_fps >= 59.5));
        println!(
            "  frame interval p50 {:.2} ms   p99 {:.2} ms   max {:.2} ms   p99 limit {DROPPED_FRAME_MS:.0} ms: {}",
            at(50),
            at(99),
            times[times.len() - 1].as_secs_f64() * 1000.0,
            verdict(at(99) <= DROPPED_FRAME_MS),
        );
        println!("  dropped frames (over {DROPPED_FRAME_MS:.0} ms): {dropped}");
        println!("  work per frame p99 {work_p99:.2} ms   limit 16.70 ms: {}", verdict(work_p99 <= 16.7));
        let bands = [(0.0, 100.0), (100.0, 1_000.0), (1_000.0, 10_000.0), (10_000.0, f64::MAX)];
        for (low, high) in bands {
            let mut band: Vec<Duration> = self
                .frames_by_altitude
                .iter()
                .filter(|(altitude, _)| *altitude >= low && *altitude < high)
                .map(|(_, time)| *time)
                .collect();
            band.sort_unstable();
            if !band.is_empty() {
                let at = |percent: usize| band[(band.len() * percent / 100).min(band.len() - 1)].as_secs_f64() * 1000.0;
                println!("  altitude {low:>6.0}+  {:>5} frames   p50 {:>5.2} ms   p99 {:>5.2} ms", band.len(), at(50), at(99));
            }
        }
        println!("  peak {} instances, {} projectiles", self.peak_instances, self.peak_projectiles);
        println!("autotest: tick {} checksum {:016x}", result.tick, result.checksum);
    }
}

fn raw_to_world(raw: i32) -> f64 {
    f64::from(raw) / 4096.0
}

fn run_windowed(seed: u64, autotest_seconds: Option<u32>, capture_dir: Option<PathBuf>) {
    let scenario = scenario::default_battle(seed);
    let world = &scenario.world;
    let map_units = f64::from(world.terrain.map_units());
    let heights = HeightField::new(
        world.terrain.samples_per_side(),
        world.terrain.heights().to_vec(),
        f64::from(TERRAIN_CELL_UNITS),
        1.0 / f64::from(1 << HEIGHT_FRAC_BITS),
    );
    // Walls are drawn one terrain cell at a time, so that they follow the hills.
    let cell = f64::from(TERRAIN_CELL_UNITS);
    let mut obstacles: Vec<([f64; 2], f64)> = Vec::new();
    for rect in &world.obstacles {
        let raw = |value: Fp| raw_to_world(value.to_bits());
        let mut y = (raw(rect.min.y) / cell).floor() * cell;
        while y < raw(rect.max.y) {
            let mut x = (raw(rect.min.x) / cell).floor() * cell;
            while x < raw(rect.max.x) {
                let top = heights.height_at(x + cell * 0.5, y + cell * 0.5) + 1.0;
                obstacles.push(([x, y], top));
                x += cell;
            }
            y += cell;
        }
    }

    let mut autotest = autotest_seconds.map(|seconds| Autotest::new(seconds, capture_dir));
    let source = match autotest_seconds {
        Some(seconds) => CommandSource::Script { seed, ticks: seconds * 10 },
        None => CommandSource::Player,
    };
    let mut driver = Some(SimDriver::start(scenario, source, true));

    let event_loop = EventLoop::new().expect("event loop starts");
    let window = Arc::new(
        WindowBuilder::new()
            .with_title("sim-engine")
            .with_inner_size(LogicalSize::new(1440.0, 900.0))
            .build(&event_loop)
            .expect("window opens"),
    );
    let size = window.inner_size();
    let mut renderer = Renderer::new(Arc::clone(&window), &heights);
    let mut camera = Camera::new(map_units, [f64::from(size.width), f64::from(size.height)]);

    let mut selection = Selection::default();
    let mut previous = Arc::new(SimSnapshot::default());
    let mut current = Arc::new(SimSnapshot::default());
    let mut current_arrived = Instant::now();
    let mut last_frame = Instant::now();
    let mut frames_seen = 0usize;
    let mut held_keys = [false; 4];
    let mut alt_held = false;
    let mut unit_instances: Vec<UnitInstanceRaw> = Vec::new();
    let mut projectile_instances: Vec<UnitInstanceRaw> = Vec::new();
    let mut obstacle_instances: Vec<UnitInstanceRaw> = Vec::new();
    let mut title_timer = Instant::now();
    let mut title_frames = 0u32;

    event_loop.set_control_flow(ControlFlow::Poll);
    event_loop
        .run(move |event, target| match event {
            Event::AboutToWait => window.request_redraw(),
            Event::WindowEvent { event, .. } => match event {
                WindowEvent::CloseRequested => target.exit(),
                WindowEvent::Resized(new_size) => {
                    renderer.resize(new_size.width, new_size.height);
                    camera.set_viewport([f64::from(new_size.width.max(1)), f64::from(new_size.height.max(1))]);
                }
                WindowEvent::ModifiersChanged(modifiers) => alt_held = modifiers.state().alt_key(),
                WindowEvent::CursorMoved { position, .. } => selection.move_cursor([position.x, position.y]),
                WindowEvent::MouseWheel { delta, .. } => {
                    let steps = match delta {
                        MouseScrollDelta::LineDelta(_, y) => f64::from(y),
                        MouseScrollDelta::PixelDelta(PhysicalPosition { y, .. }) => y / 40.0,
                    };
                    let anchor = camera.screen_to_ground(selection.cursor(), &heights);
                    camera.zoom(steps, anchor);
                }
                WindowEvent::MouseInput { state, button, .. } => match (button, state) {
                    (MouseButton::Left, ElementState::Pressed) => selection.begin_drag(),
                    (MouseButton::Left, ElementState::Released) => {
                        selection.end_drag(&current.units, &camera, PLAYER_TEAM);
                    }
                    (MouseButton::Right, ElementState::Pressed) if !selection.selected().is_empty() => {
                        if let (Some(ground), Some(driver)) =
                            (camera.screen_to_ground(selection.cursor(), &heights), driver.as_ref())
                        {
                            driver.send(order_command(selection.selected(), ground, alt_held));
                        }
                    }
                    _ => {}
                },
                WindowEvent::KeyboardInput { event, .. } => {
                    let pressed = event.state == ElementState::Pressed;
                    match event.physical_key {
                        PhysicalKey::Code(KeyCode::KeyW | KeyCode::ArrowUp) => held_keys[0] = pressed,
                        PhysicalKey::Code(KeyCode::KeyS | KeyCode::ArrowDown) => held_keys[1] = pressed,
                        PhysicalKey::Code(KeyCode::KeyA | KeyCode::ArrowLeft) => held_keys[2] = pressed,
                        PhysicalKey::Code(KeyCode::KeyD | KeyCode::ArrowRight) => held_keys[3] = pressed,
                        PhysicalKey::Code(KeyCode::Escape) if pressed => selection.clear(),
                        _ => {}
                    }
                }
                WindowEvent::RedrawRequested => {
                    let now = Instant::now();
                    let frame_time = now - last_frame;
                    last_frame = now;
                    let dt = frame_time.as_secs_f64().min(0.1);

                    // Take the newest snapshots. The frame draws between the last two.
                    if let Some(driver) = driver.as_ref() {
                        while let Some(snapshot) = driver.try_snapshot() {
                            previous = std::mem::replace(&mut current, snapshot);
                            current_arrived = now;
                        }
                    }
                    selection.retain_alive(&current.units);
                    let alpha = ((now - current_arrived).as_secs_f64() / TICK_DURATION.as_secs_f64()).clamp(0.0, 1.0);

                    let pan_x = f64::from(u8::from(held_keys[3])) - f64::from(u8::from(held_keys[2]));
                    let pan_y = f64::from(u8::from(held_keys[0])) - f64::from(u8::from(held_keys[1]));
                    camera.pan(pan_x, pan_y, dt);

                    // Test runs follow a fixed zoom sweep.
                    let mut progress = None;
                    if let Some(test) = autotest.as_mut() {
                        let started = *test.started.get_or_insert(now);
                        let fraction = ((now - started).as_secs_f64() / test.duration.as_secs_f64()).min(1.0);
                        camera.set_altitude(Autotest::altitude_at(fraction, camera.max_altitude()));
                        progress = Some(fraction);
                    }
                    camera.update(dt, &heights);

                    let eye = camera.eye();
                    instances::build_unit_instances(&previous.units, &current.units, alpha, eye, selection.selected(), &mut unit_instances);
                    projectile_instances.clear();
                    instances::build_projectile_instances(&previous.projectiles, &current.projectiles, alpha, eye, &mut projectile_instances);
                    obstacle_instances.clear();
                    for &(min, top) in &obstacles {
                        let max = [min[0] + cell, min[1] + cell];
                        obstacle_instances.push(instances::obstacle_instance(min, max, top - OBSTACLE_DRAW_HEIGHT, OBSTACLE_DRAW_HEIGHT, eye));
                    }
                    let scene = FrameScene {
                        camera: &camera,
                        units: &unit_instances,
                        projectiles: &projectile_instances,
                        obstacles: &obstacle_instances,
                        selection_box: selection.drag_box(),
                    };

                    if let (Some(test), Some(fraction)) = (autotest.as_mut(), progress) {
                        if test.capture_points.first().is_some_and(|&point| fraction >= point) {
                            let point = test.capture_points.remove(0);
                            if let Some(directory) = &test.capture_dir {
                                let path = directory.join(format!("zoom_{:03}.bmp", (point * 100.0) as u32));
                                match renderer.capture(&scene, &path) {
                                    Ok(()) => println!(
                                        "captured {} at altitude {:.0}, {} units, {} projectiles",
                                        path.display(), camera.altitude(), current.units.len(), current.projectiles.len()
                                    ),
                                    Err(error) => println!("capture failed: {error}"),
                                }
                                // A capture is not part of the frame being measured.
                                last_frame = Instant::now();
                            }
                        }
                    }
                    // Measured frame times span from the last frame's end to this one's,
                    // so they include the drawable wait from the frame before.
                    let previous_acquire = renderer.last_acquire();
                    renderer.render(&scene);

                    frames_seen += 1;
                    if let Some(test) = autotest.as_mut() {
                        if frames_seen > WARMUP_FRAMES {
                            test.frame_times.push(frame_time);
                            test.work_times.push(frame_time.saturating_sub(previous_acquire));
                            test.frames_by_altitude.push((camera.altitude(), frame_time));
                        }
                        test.peak_projectiles = test.peak_projectiles.max(current.projectiles.len());
                        test.peak_instances = test.peak_instances.max(unit_instances.len() + projectile_instances.len());
                        if driver.as_ref().is_some_and(SimDriver::is_finished) {
                            let result = driver.take().expect("driver is present").finish();
                            test.report(result);
                            target.exit();
                        }
                    }

                    // The FPS counter.
                    title_frames += 1;
                    let elapsed = title_timer.elapsed();
                    if elapsed >= Duration::from_millis(500) {
                        let fps = f64::from(title_frames) / elapsed.as_secs_f64();
                        let tick_ms = driver.as_ref().map_or(0.0, |driver| f64::from(driver.last_tick_micros()) / 1000.0);
                        let blue = current.units.iter().filter(|unit| unit.team == Team::Blue).count();
                        let red = current.units.len() - blue;
                        window.set_title(&format!(
                            "sim-engine | {fps:.0} FPS | tick {tick_ms:.1} ms | Blue {blue} vs Red {red} | {} shells | {} selected",
                            current.projectiles.len(),
                            selection.selected().len(),
                        ));
                        title_timer = Instant::now();
                        title_frames = 0;
                    }
                }
                _ => {}
            },
            _ => {}
        })
        .expect("event loop runs");
}
