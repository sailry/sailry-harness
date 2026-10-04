// Thinking-orbs geometry originated in gpui-thinking-orbs (MIT).
// Adapted from Bezel 4a7505ab (MIT). See third_party_licenses/bezel.md.
//! Geometry types shared by the animation engine and the Kit widget.

/// The engine's animation states.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum OrbState {
    /// Particles on tilted orbits.
    #[default]
    Working,
    /// A scan meridian sweeps a dotted globe.
    Searching,
    /// Bands scramble in quarter turns, then click back.
    #[cfg(test)]
    Solving,
    /// A waveform rolls through latitude rings.
    #[cfg(test)]
    Listening,
    /// A constellation wires itself, packets running the edges.
    #[cfg(test)]
    Connecting,
    /// Three strands plait around the sphere.
    #[cfg(test)]
    Weaving,
    /// An undulating multi-band sash.
    Composing,
    /// A face-on ring slowly morphing.
    Breathing,
    /// A dotted outline morphs circle → triangle → square.
    #[cfg(test)]
    Shaping,
    /// An iris of particles converges and relaxes around a focal point.
    #[cfg(test)]
    Focusing,
    /// Counter-rotating great circles form a reasoning gyroscope.
    #[cfg(test)]
    Reasoning,
    /// Concentric memory echoes travel out from a steady core.
    #[cfg(test)]
    Recalling,
}

impl OrbState {
    /// All states in playground / gallery order.
    ///
    /// This is a slice so adding future states does not change its public type.
    #[cfg(test)]
    pub const ALL_STATES: &'static [OrbState] = &[
        OrbState::Working,
        OrbState::Searching,
        OrbState::Solving,
        OrbState::Listening,
        OrbState::Connecting,
        OrbState::Weaving,
        OrbState::Composing,
        OrbState::Breathing,
        OrbState::Shaping,
        OrbState::Focusing,
        OrbState::Reasoning,
        OrbState::Recalling,
    ];
}

/// Rendered size in logical pixels. Four tuned presets ship: inline (20),
/// avatar (64), large (96), and hero (128). Larger sizes add detail gradually
/// instead of merely stretching the 64 px artwork.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum OrbSize {
    /// Chat-avatar scale (64 logical px).
    #[default]
    Avatar,
    /// Inline-text scale (20 logical px).
    Inline,
    /// Prominent status / card scale (96 logical px).
    Large,
    /// Hero / empty-state scale (128 logical px).
    Hero,
}

impl OrbSize {
    /// All sizes in compact-to-prominent gallery order.
    #[cfg(test)]
    pub const ALL_SIZES: &'static [OrbSize] = &[
        OrbSize::Inline,
        OrbSize::Avatar,
        OrbSize::Large,
        OrbSize::Hero,
    ];

    /// Logical pixel edge length of the orb.
    #[cfg(test)]
    pub fn pixels(self) -> f32 {
        match self {
            OrbSize::Inline => 20.0,
            OrbSize::Avatar => 64.0,
            OrbSize::Large => 96.0,
            OrbSize::Hero => 128.0,
        }
    }
}

/// Internal mode keys — one painter per state.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ModeKey {
    Orbits,
    Globe,
    #[cfg(test)]
    Rubik,
    #[cfg(test)]
    Wave,
    #[cfg(test)]
    Web,
    #[cfg(test)]
    Braid,
    Ribbon,
    Ring,
    #[cfg(test)]
    Morph,
    #[cfg(test)]
    Focus,
    #[cfg(test)]
    Gyroscope,
    #[cfg(test)]
    Echo,
}

impl ModeKey {
    pub fn from_state(state: OrbState) -> Self {
        match state {
            OrbState::Working => ModeKey::Orbits,
            OrbState::Searching => ModeKey::Globe,
            #[cfg(test)]
            OrbState::Solving => ModeKey::Rubik,
            #[cfg(test)]
            OrbState::Listening => ModeKey::Wave,
            #[cfg(test)]
            OrbState::Connecting => ModeKey::Web,
            #[cfg(test)]
            OrbState::Weaving => ModeKey::Braid,
            OrbState::Composing => ModeKey::Ribbon,
            OrbState::Breathing => ModeKey::Ring,
            #[cfg(test)]
            OrbState::Shaping => ModeKey::Morph,
            #[cfg(test)]
            OrbState::Focusing => ModeKey::Focus,
            #[cfg(test)]
            OrbState::Reasoning => ModeKey::Gyroscope,
            #[cfg(test)]
            OrbState::Recalling => ModeKey::Echo,
        }
    }
}
