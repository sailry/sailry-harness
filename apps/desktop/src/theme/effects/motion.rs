//! Deterministic normalized motion, independent of frame rate and viewport size.
use std::f32::consts::{PI, TAU};

#[derive(Clone, Copy, Debug)]
pub(super) struct Sample {
    pub x: f32,
    pub y: f32,
    pub time: f32,
    pub fade: f32,
    pub pulse: f32,
    pub scale: f32,
    pub sway: f32,
}

fn noise(index: u32, seed: u32, channel: u32) -> f32 {
    let mut value = index.wrapping_mul(0x9e3779b9) ^ seed ^ channel.wrapping_mul(0x85ebca6b);
    value ^= value >> 16;
    value = value.wrapping_mul(0x7feb352d);
    value ^= value >> 15;
    value = value.wrapping_mul(0x846ca68b);
    value ^= value >> 16;
    (value & 0xffffff) as f32 / 0x1000000 as f32
}

pub(super) fn sample(index: u32, seed: u32, phase: f32) -> Sample {
    let time = (phase + noise(index, seed, 1)).rem_euclid(1.);
    Sample {
        x: 0.025 + noise(index, seed, 2) * 0.95,
        y: 0.025 + noise(index, seed, 3) * 0.95,
        time,
        fade: (time * PI).sin().powi(2),
        pulse: 0.12 + 0.88 * (time * TAU).sin().powi(2),
        scale: 0.65 + noise(index, seed, 4) * 0.7,
        sway: (time * TAU).sin() * 0.025,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remains_bounded() {
        for seed in [0, 12, u32::MAX] {
            for index in 0..64 {
                for step in 0..100 {
                    let point = sample(index, seed, step as f32 / 100.);
                    for value in [point.x, point.y, point.time, point.fade, point.pulse] {
                        assert!((0. ..=1.).contains(&value));
                    }
                    assert!((0.65..=1.35).contains(&point.scale));
                }
            }
        }
    }

    #[test]
    fn loops_without_position_jump() {
        for index in 0..64 {
            let first = sample(index, 27, 0.);
            let last = sample(index, 27, 1.);
            for (start, end) in [
                (first.x, last.x),
                (first.y, last.y),
                (first.time, last.time),
                (first.fade, last.fade),
                (first.pulse, last.pulse),
                (first.sway, last.sway),
            ] {
                assert!((start - end).abs() < 0.00001);
            }
        }
    }

    #[test]
    fn hides_travel_endpoints() {
        for index in 0..64 {
            let offset = sample(index, 27, 0.).time;
            assert_eq!(sample(index, 27, -offset).fade, 0.);
            assert!(sample(index, 27, 1. - offset - 0.00001).fade < 0.00001);
        }
    }

    #[test]
    fn varies_with_seed() {
        let first = sample(1, 1, 0.2);
        let other = sample(1, 2, 0.2);
        assert_ne!((first.x, first.y), (other.x, other.y));
    }
}
