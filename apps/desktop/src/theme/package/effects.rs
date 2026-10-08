//! Bounded, declarative decorative motion in the current v1 theme contract.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[cfg(test)]
mod tests;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Kind {
    Twinkle,
    Embers,
    Dust,
    Petals,
    Pulse,
    Scan,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Effect {
    pub kind: Kind,
    pub color: String,
    pub count: u8,
    pub opacity: f32,
    pub size: f32,
    /// One complete loop, in seconds.
    pub duration: f32,
    #[serde(default)]
    pub seed: u32,
}

pub(super) fn validate(
    effects: &BTreeMap<String, Vec<Effect>>,
    assets: &BTreeMap<String, super::Decoration>,
) -> Result<(), String> {
    let mut particles = 0;
    for (slot, layers) in effects {
        let supported = slot == "conversation"
            || slot == "hero.new_session"
            || [
                "background.window",
                "background.sidebar",
                "background.content",
            ]
            .contains(&slot.as_str())
            || (super::SLOTS.contains(&slot.as_str()) && slot.starts_with("banner."));
        if !supported || layers.is_empty() || layers.len() > 4 {
            return Err(format!("invalid theme effect target: {slot}"));
        }
        if (slot.starts_with("banner.") || slot == "hero.new_session") && !assets.contains_key(slot)
        {
            return Err(format!("theme effect target requires artwork: {slot}"));
        }
        for effect in layers {
            if !(1..=64).contains(&effect.count)
                || !effect.opacity.is_finite()
                || !(0. ..=1.).contains(&effect.opacity)
                || !effect.size.is_finite()
                || !(0.5..=12.).contains(&effect.size)
                || !effect.duration.is_finite()
                || !(3. ..=120.).contains(&effect.duration)
                || (matches!(effect.kind, Kind::Pulse | Kind::Scan) && effect.count != 1)
            {
                return Err(format!("invalid theme effect dimensions: {slot}"));
            }
            gpui_kit::component::theme::try_parse_color(&effect.color)
                .map_err(|error| error.to_string())?;
            particles += usize::from(effect.count);
        }
    }
    if particles > 192 {
        return Err("theme effects exceed particle limit".into());
    }
    Ok(())
}
