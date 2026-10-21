//! A short confetti burst for the moment Pro switches on.
//!
//! Pieces fly up and out from one point, fall back with gravity and fade out.
//! Physics runs on real elapsed time, so a slow frame never slows the burst.

use crate::paint::{Gfx, Rgb};
use std::time::Instant;

/// How long the burst lasts.
pub const LENGTH_MS: u128 = 2600;
const FADE_MS: f32 = 700.0;
const PIECES: usize = 120;
/// The app's teal first, a few warm colours for the moment.
const COLORS: [Rgb; 6] = [(13, 148, 136), (45, 212, 191), (13, 148, 136), (245, 158, 11), (244, 63, 94), (99, 102, 241)];

struct Piece {
    x: f32,
    y: f32,
    vx: f32,
    vy: f32,
    angle: f32,
    spin: f32,
    flip: f32,
    flip_speed: f32,
    w: f32,
    h: f32,
    color: Rgb,
}

pub struct Burst {
    start: Instant,
    last: Instant,
    scale: f32,
    pieces: Vec<Piece>,
}

/// xorshift: enough randomness for confetti, no extra crate.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 40) as f32 / (1u64 << 24) as f32
    }
    fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.next()
    }
}

impl Burst {
    /// A burst from (x, y) in pixels; `scale` is pixels per 96-DPI pixel.
    pub fn new(x: f32, y: f32, scale: f32) -> Burst {
        let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0x9E37_79B9, |d| d.as_nanos() as u64);
        let mut rng = Rng(seed | 1);
        let pieces = (0..PIECES)
            .map(|i| {
                let a = rng.range(-170.0, -10.0).to_radians();
                let speed = rng.range(320.0, 900.0) * scale;
                Piece {
                    x,
                    y,
                    vx: a.cos() * speed,
                    vy: a.sin() * speed,
                    angle: rng.range(0.0, std::f32::consts::TAU),
                    spin: rng.range(-9.0, 9.0),
                    flip: rng.range(0.0, std::f32::consts::TAU),
                    flip_speed: rng.range(6.0, 14.0),
                    w: rng.range(6.0, 11.0) * scale,
                    h: rng.range(3.0, 6.0) * scale,
                    color: COLORS[i % COLORS.len()],
                }
            })
            .collect();
        let now = Instant::now();
        Burst { start: now, last: now, scale, pieces }
    }

    pub fn done(&self) -> bool {
        self.start.elapsed().as_millis() >= LENGTH_MS
    }

    /// Milliseconds since the burst started.
    pub fn age_ms(&self) -> f32 {
        self.start.elapsed().as_secs_f32() * 1000.0
    }

    /// Moves every piece on by the time since the last step.
    pub fn step(&mut self) {
        let now = Instant::now();
        let dt = now.duration_since(self.last).as_secs_f32().min(0.05);
        self.last = now;
        let gravity = 950.0 * self.scale;
        let drag = (1.0 - 1.8 * dt).max(0.0);
        for p in &mut self.pieces {
            p.vy += gravity * dt;
            p.vx *= drag;
            p.vy *= drag;
            p.x += p.vx * dt;
            p.y += p.vy * dt;
            p.angle += p.spin * dt;
            p.flip += p.flip_speed * dt;
        }
    }

    pub unsafe fn draw(&self, gfx: &Gfx) {
        let left = LENGTH_MS as f32 - self.age_ms();
        let alpha = (left / FADE_MS).clamp(0.0, 1.0);
        if alpha <= 0.0 {
            return;
        }
        let a = (alpha * 255.0) as u8;
        for p in &self.pieces {
            // Flutter: the piece turns edge-on and back as it falls.
            let hw = (p.w / 2.0) * p.flip.cos().abs().max(0.15);
            let hh = p.h / 2.0;
            let (sin, cos) = p.angle.sin_cos();
            let corner = |dx: f32, dy: f32| (p.x + dx * cos - dy * sin, p.y + dx * sin + dy * cos);
            gfx.quad([corner(-hw, -hh), corner(hw, -hh), corner(hw, hh), corner(-hw, hh)], p.color, a);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pieces_fly_up_then_fall() {
        let mut b = Burst::new(100.0, 100.0, 1.0);
        assert_eq!(b.pieces.len(), PIECES);
        assert!(b.pieces.iter().all(|p| p.vy < 0.0), "every piece starts upwards");
        for _ in 0..60 {
            b.last -= std::time::Duration::from_millis(20);
            b.step();
        }
        assert!(b.pieces.iter().all(|p| p.vy > 0.0), "after a while gravity wins");
        assert!(!b.done());
    }
}
