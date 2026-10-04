// Thinking-orbs geometry originated in gpui-thinking-orbs (MIT).
// Adapted from Bezel 4a7505ab (MIT). See third_party_licenses/bezel.md.
//! Backend-agnostic animation engine (ported from thinking-orbs).

#[cfg(test)]
mod braid;
#[cfg(test)]
mod concepts;
mod core;
mod lattice;
#[cfg(test)]
mod morph;
mod orbits;
mod profiles;
mod ribbon;
#[cfg(test)]
mod web;

use core::sort_dots;
pub(super) use core::{Frame, Line};
#[cfg(test)]
pub(super) use profiles::{MAX_ICON_D, MAX_MORPH_DOTS, MAX_NODE_N};
pub(super) use profiles::{ModeOpts, base_profile, scale_counts, scale_radii};
#[cfg(test)]
pub(super) use profiles::{sanitize_mode_opts, sanitize_size};

use crate::ui::thinking::types::ModeKey;
#[cfg(test)]
use braid::draw_braid_into;
#[cfg(test)]
use concepts::{draw_echo_into, draw_focus_into, draw_gyroscope_into};
use lattice::draw_globe_into;
#[cfg(test)]
use lattice::{draw_rubik_into, draw_wave_into};
#[cfg(test)]
use morph::draw_morph_into;
use orbits::draw_orbits_into;
use ribbon::draw_ribbon_into;
#[cfg(test)]
use web::draw_web_into;

/// Paint one frame for the given mode into an existing [`Frame`], reusing its
/// buffers.
///
/// This is the allocation-free entry point for the **geometry** buffers: `out`
/// is cleared but keeps its capacity, so a steady-state animation loop performs
/// no heap growth after the first frame (the Solving mode reuses thread-local
/// scratch for its move tables). Prefer this over [`draw_mode`] in render loops.
///
/// `size` and every count-like field of `opts` are sanitized (finite + hard
/// ceilings) before geometry runs — see [`sanitize_mode_opts`].
#[cfg(test)]
pub fn draw_mode_into(mode: ModeKey, size: f32, t: f32, opts: &ModeOpts, out: &mut Frame) {
    let size = sanitize_size(size);
    let opts = sanitize_mode_opts(opts);
    let t = if t.is_finite() { t } else { 0.0 };
    draw_mode_into_resolved(mode, size, t, &opts, out);
}

/// Fast path for the widget's already-sanitized, size-bounded presets.
///
/// Keeping this crate-private preserves the safety contract of the public
/// power-user entry point while avoiding a 40-field clone/sanitize pass on
/// every widget tick.
pub(crate) fn draw_mode_into_resolved(
    mode: ModeKey,
    size: f32,
    t: f32,
    opts: &ModeOpts,
    out: &mut Frame,
) {
    out.clear();
    match mode {
        ModeKey::Orbits => draw_orbits_into(size, t, opts, out),
        ModeKey::Globe => draw_globe_into(size, t, opts, out),
        #[cfg(test)]
        ModeKey::Rubik => draw_rubik_into(size, t, opts, out),
        #[cfg(test)]
        ModeKey::Wave => draw_wave_into(size, t, opts, out),
        #[cfg(test)]
        ModeKey::Web => draw_web_into(size, t, opts, out),
        #[cfg(test)]
        ModeKey::Braid => draw_braid_into(size, t, opts, out),
        ModeKey::Ribbon | ModeKey::Ring => draw_ribbon_into(size, t, opts, out),
        #[cfg(test)]
        ModeKey::Morph => draw_morph_into(size, t, opts, out),
        #[cfg(test)]
        ModeKey::Focus => draw_focus_into(size, t, opts, out),
        #[cfg(test)]
        ModeKey::Gyroscope => draw_gyroscope_into(size, t, opts, out),
        #[cfg(test)]
        ModeKey::Echo => draw_echo_into(size, t, opts, out),
    }
    // Face-on modes intentionally emit z=0 in painter order, so a comparison
    // sort cannot change their result. Avoid it entirely.
    #[cfg(test)]
    if matches!(mode, ModeKey::Focus | ModeKey::Echo) {
        return;
    }
    sort_dots(&mut out.dots);
}

/// Paint one frame for the given mode into a fresh [`Frame`].
///
/// Convenience wrapper around [`draw_mode_into`] for one-off calls and tests;
/// it allocates. Render loops should use [`draw_mode_into`] instead.
#[cfg(test)]
pub fn draw_mode(mode: ModeKey, size: f32, t: f32, opts: &ModeOpts) -> Frame {
    let mut frame = Frame::default();
    draw_mode_into(mode, size, t, opts, &mut frame);
    frame
}
