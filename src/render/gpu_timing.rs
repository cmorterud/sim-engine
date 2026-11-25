//! How long the graphics card spends drawing the land, the units, and the sea, from timestamps it
//! writes between them, for test runs (`--autotest`). Only where the adapter can write
//! timestamps inside a render pass, which native Vulkan, Metal, and DirectX 12 mostly can and
//! WebGL2 cannot. Results are read back a frame or more late, without waiting, so that timing a
//! frame does not change how long it takes.

use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;

/// The parts of the scene timed, in the order they are drawn.
pub const SEGMENTS: [&str; 3] = ["terrain", "units", "water"];
/// One timestamp before the first part and one after each.
const STAMPS: u32 = SEGMENTS.len() as u32 + 1;
const STAMP_BYTES: u64 = 8;

const IDLE: u8 = 0;
const MAPPING: u8 = 1;
const READY: u8 = 2;

pub struct GpuTimer {
    queries: wgpu::QuerySet,
    resolve: wgpu::Buffer,
    readback: wgpu::Buffer,
    /// Nanoseconds per timestamp tick.
    period: f64,
    state: Arc<AtomicU8>,
    /// Whether this frame's timestamps are being copied out, so that the copy is mapped once
    /// the frame is submitted.
    copying: bool,
    totals_ms: [f64; SEGMENTS.len()],
    frames: u32,
}

impl GpuTimer {
    /// The features a device needs for timing, which it should ask for if the adapter has them.
    pub const FEATURES: wgpu::Features = wgpu::Features::TIMESTAMP_QUERY.union(wgpu::Features::TIMESTAMP_QUERY_INSIDE_PASSES);

    /// A timer, if the device can write timestamps inside a render pass.
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Option<Self> {
        if !device.features().contains(Self::FEATURES) {
            return None;
        }
        let size = u64::from(STAMPS) * STAMP_BYTES;
        Some(Self {
            queries: device.create_query_set(&wgpu::QuerySetDescriptor { label: Some("gpu timing"), ty: wgpu::QueryType::Timestamp, count: STAMPS }),
            resolve: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("gpu timing resolve"),
                size,
                usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            }),
            readback: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("gpu timing readback"),
                size,
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            period: f64::from(queue.get_timestamp_period()),
            state: Arc::new(AtomicU8::new(IDLE)),
            copying: false,
            totals_ms: [0.0; SEGMENTS.len()],
            frames: 0,
        })
    }

    /// Writes timestamp `index`: 0 before the terrain, then one after each of `SEGMENTS`.
    pub fn stamp(&self, pass: &mut wgpu::RenderPass<'_>, index: u32) {
        pass.write_timestamp(&self.queries, index);
    }

    /// After the scene's pass: copies the timestamps out, unless the last copy is still being read.
    pub fn resolve(&mut self, encoder: &mut wgpu::CommandEncoder) {
        self.copying = self.state.load(Ordering::Acquire) == IDLE;
        if self.copying {
            encoder.resolve_query_set(&self.queries, 0..STAMPS, &self.resolve, 0);
            encoder.copy_buffer_to_buffer(&self.resolve, 0, &self.readback, 0, u64::from(STAMPS) * STAMP_BYTES);
        }
    }

    /// After the frame is submitted: starts reading this frame's copy, and adds up any copy
    /// that has finished.
    pub fn after_submit(&mut self, device: &wgpu::Device) {
        if self.copying {
            self.copying = false;
            self.state.store(MAPPING, Ordering::Release);
            let state = Arc::clone(&self.state);
            self.readback.slice(..).map_async(wgpu::MapMode::Read, move |result| state.store(if result.is_ok() { READY } else { IDLE }, Ordering::Release));
        }
        device.poll(wgpu::Maintain::Poll);
        if self.state.load(Ordering::Acquire) == READY {
            let stamps: Vec<u64> = self.readback.slice(..).get_mapped_range().as_chunks::<8>().0.iter().map(|bytes| u64::from_le_bytes(*bytes)).collect();
            self.readback.unmap();
            self.state.store(IDLE, Ordering::Release);
            // A frame whose timestamps went backwards, as some drivers' do across a power state
            // change, is left out.
            if stamps.windows(2).all(|pair| pair[0] <= pair[1]) && stamps[0] > 0 {
                for (total, pair) in self.totals_ms.iter_mut().zip(stamps.windows(2)) {
                    *total += (pair[1] - pair[0]) as f64 * self.period / 1.0e6;
                }
                self.frames += 1;
            }
        }
    }

    /// The average milliseconds each of `SEGMENTS` took, over the frames read so far.
    pub fn averages_ms(&self) -> Option<[f64; SEGMENTS.len()]> {
        (self.frames > 0).then(|| self.totals_ms.map(|total| total / f64::from(self.frames)))
    }

    pub const fn frames(&self) -> u32 {
        self.frames
    }
}
