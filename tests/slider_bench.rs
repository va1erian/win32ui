//! Measures the slider while a drag is driven with synthetic mouse input:
//! input-to-presented latency, allocations per frame and paints per input.
//!
//! Ignored by default because it takes several seconds and prints numbers
//! rather than asserting them:
//!
//! ```text
//! cargo test --release --test slider_bench -- --ignored --nocapture
//! ```
//!
//! "Presented" is the moment the `on_change` for the frame reaches
//! `App::update`: the widget flushes it right after Direct2D presents the frame,
//! so the figure includes one trip through the app's message queue.

#![cfg(windows)]

mod common;

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use common::run_app_spec_with_watchdog_ms;
use win32ui::column;
use win32ui::prelude::*;
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    SendMessageW, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_PAINT,
};

/// Counts every allocation the process makes.
struct Counting;

static ALLOCATIONS: AtomicU64 = AtomicU64::new(0);

// SAFETY: forwards every call to the system allocator unchanged and only adds
// a relaxed counter increment.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        // SAFETY: the caller's contract is passed through.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: the caller's contract is passed through.
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

fn allocations() -> u64 {
    ALLOCATIONS.load(Ordering::Relaxed)
}

/// The drag's positions: three sweeps (right, left, right) across the full
/// width, 200 inputs each.
const SWEEP_INPUTS: usize = 200;
const SWEEPS: usize = 3;
/// Open-loop bursts: this many moves are sent back to back per burst.
const BURST: usize = 8;
const BURSTS: usize = 100;
/// Paints measured for the paint-only allocation count.
const PAINTS: u64 = 200;
const PAD_DIP: f64 = 12.0;

#[derive(Debug)]
enum Msg {
    Start,
    Change,
    Done,
}

#[derive(Clone, Copy, PartialEq)]
enum Phase {
    ClosedLoop,
    Bursts,
}

struct Bench {
    _slider: Slider<Msg>,
    hwnd: HWND,
    positions: Vec<i32>,
    phase: Phase,
    next: usize,
    sent: Option<(Instant, u64)>,
    latencies_ms: Vec<f64>,
    frame_allocations: Vec<u64>,
    bursts_left: usize,
    burst_changes: usize,
    burst_base: usize,
}

impl Bench {
    fn send(&self, message: u32, wparam: usize, x: i32) {
        // SAFETY: a synchronous message to the slider's own live window.
        unsafe {
            SendMessageW(
                self.hwnd,
                message,
                Some(WPARAM(wparam)),
                Some(LPARAM((10 << 16) | (x as i16 as u16 as isize))),
            );
        }
    }

    fn move_to(&mut self, index: usize) {
        self.sent = Some((Instant::now(), allocations()));
        self.send(WM_MOUSEMOVE, 1, self.positions[index]);
    }

    fn finish_drag(&self) {
        self.send(WM_LBUTTONUP, 0, self.positions[0]);
    }
}

fn percentile(sorted: &[f64], fraction: f64) -> f64 {
    sorted[((sorted.len() - 1) as f64 * fraction).round() as usize]
}

impl App for Bench {
    type Msg = Msg;

    fn update(&mut self, msg: Msg, ui: &mut Ui<Msg>) {
        match msg {
            Msg::Start => {
                self.send(WM_LBUTTONDOWN, 1, self.positions[0]);
                self.next = 1;
                self.move_to(1);
            }
            Msg::Change => match self.phase {
                Phase::ClosedLoop => {
                    let (sent, base) = self.sent.take().expect("a move was sent");
                    self.latencies_ms
                        .push(sent.elapsed().as_secs_f64() * 1000.0);
                    self.frame_allocations.push(allocations() - base);
                    self.next += 1;
                    if self.next < self.positions.len() {
                        self.move_to(self.next);
                    } else {
                        self.phase = Phase::Bursts;
                        self.bursts_left = BURSTS;
                        self.burst_base = 1;
                        self.start_burst();
                    }
                }
                Phase::Bursts => {
                    self.burst_changes += 1;
                    if self.bursts_left > 0 {
                        self.start_burst();
                    } else {
                        self.finish_drag();
                        ui.emit(Msg::Done);
                    }
                }
            },
            Msg::Done => {
                self.report();
                ui.quit();
            }
        }
    }
}

impl Bench {
    fn start_burst(&mut self) {
        self.bursts_left -= 1;
        for offset in 0..BURST {
            let index = (self.burst_base + offset) % self.positions.len();
            self.send(WM_MOUSEMOVE, 1, self.positions[index]);
        }
        self.burst_base += BURST;
    }

    fn report(&self) {
        let mut latencies = self.latencies_ms.clone();
        latencies.sort_by(f64::total_cmp);
        let frames = self.frame_allocations.len() as f64;
        let allocations_per_frame = self.frame_allocations.iter().sum::<u64>() as f64 / frames;

        let before = allocations();
        for _ in 0..PAINTS {
            self.send(WM_PAINT, 0, 0);
        }
        let per_paint = (allocations() - before) as f64 / PAINTS as f64;

        println!(
            "slider bench ({} closed-loop drag inputs; the process made {} allocations in total, so the counter is live)",
            latencies.len(),
            allocations()
        );
        println!(
            "  input-to-presented latency  p50 {:.2} ms   p95 {:.2} ms   max {:.2} ms",
            percentile(&latencies, 0.5),
            percentile(&latencies, 0.95),
            latencies.last().copied().unwrap_or(0.0),
        );
        println!(
            "  allocations per input->frame round trip (incl. app queue): {allocations_per_frame:.1}"
        );
        println!("  allocations per paint alone (WM_PAINT, nothing pending):   {per_paint:.2}");
        println!(
            "  bursts: {} moves sent -> {} on_change ({:.3} paints per input)",
            BURSTS * BURST,
            self.burst_changes,
            self.burst_changes as f64 / (BURSTS * BURST) as f64,
        );
    }
}

#[test]
#[ignore = "a multi-second measurement that prints numbers; run with --ignored --nocapture"]
fn drag_latency_allocations_and_paints_per_input() {
    let spec = WindowSpec::new("win32ui.slider.bench").theme(Theme::light());
    let Some(run) = run_app_spec_with_watchdog_ms(spec, 60_000, |ui| {
        let slider = Slider::new(ui, 0.0..=36_000.0)
            .expect("slider")
            .on_change(|_| Some(Msg::Change));
        ui.set_layout(column![slider.fill(1)]);
        let width = f64::from(slider.bounds().width());
        let dpi = f64::from(ui.dpi());
        let (start, end) = (PAD_DIP * dpi / 96.0, width - PAD_DIP * dpi / 96.0);
        let sweep = |reverse: bool| -> Vec<i32> {
            (0..SWEEP_INPUTS)
                .map(|step| {
                    let fraction = step as f64 / (SWEEP_INPUTS - 1) as f64;
                    let fraction = if reverse { 1.0 - fraction } else { fraction };
                    (start + fraction * (end - start)).round() as i32
                })
                .collect()
        };
        let mut positions: Vec<i32> = (0..SWEEPS).flat_map(|n| sweep(n % 2 == 1)).collect();
        positions.dedup();
        ui.emit(Msg::Start);
        Bench {
            hwnd: HWND(slider.hwnd().raw() as *mut core::ffi::c_void),
            _slider: slider,
            positions,
            phase: Phase::ClosedLoop,
            next: 0,
            sent: None,
            latencies_ms: Vec::new(),
            frame_allocations: Vec::new(),
            bursts_left: 0,
            burst_changes: 0,
            burst_base: 0,
        }
    }) else {
        return;
    };
    assert!(
        !run.timed_out,
        "the bench did not finish before the watchdog"
    );
}
