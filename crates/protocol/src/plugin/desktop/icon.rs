//! Portable navigation glyphs: an embedded catalog name or self-contained SVG.
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

pub const NAMES: &str = include_str!("icon_names.txt");
pub const MAX_SVG_BYTES: usize = 32 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(untagged, deny_unknown_fields)]
pub enum Icon {
    Name(String),
    Svg { svg: String },
}

impl Icon {
    pub fn valid(&self) -> bool {
        match self {
            Self::Name(name) => {
                static NAMES_SORTED: OnceLock<Vec<&str>> = OnceLock::new();
                name.strip_prefix("reicon:").is_some_and(|name| {
                    NAMES_SORTED
                        .get_or_init(|| NAMES.lines().collect())
                        .binary_search(&name)
                        .is_ok()
                })
            }
            Self::Svg { svg } => valid_svg(svg),
        }
    }
}

fn valid_svg(svg: &str) -> bool {
    if svg.len() > MAX_SVG_BYTES {
        return false;
    }
    let Ok(document) = roxmltree::Document::parse_with_options(
        svg,
        roxmltree::ParsingOptions {
            nodes_limit: 512,
            ..Default::default()
        },
    ) else {
        return false;
    };
    let root = document.root_element();
    if root.tag_name().name() != "svg" {
        return false;
    }
    let viewbox = root
        .attribute("viewBox")
        .unwrap_or("")
        .split(|c: char| c.is_ascii_whitespace() || c == ',')
        .filter(|v| !v.is_empty())
        .map(str::parse::<f32>)
        .collect::<Result<Vec<_>, _>>();
    let Ok(viewbox) = viewbox else {
        return false;
    };
    if viewbox.len() != 4
        || viewbox.iter().any(|n| !n.is_finite())
        || viewbox[2] <= 0.
        || viewbox[3] <= 0.
    {
        return false;
    }
    // Navigation glyphs are vectors, not documents. No scripts, fonts, images,
    // filesystem/remote references, CSS, animation, or entity expansion.
    let mut shapes = 0;
    for node in root.descendants().filter(|node| node.is_element()) {
        if node
            .tag_name()
            .namespace()
            .is_some_and(|ns| ns != "http://www.w3.org/2000/svg")
        {
            return false;
        }
        match node.tag_name().name() {
            "path" | "rect" | "circle" | "ellipse" | "line" | "polyline" | "polygon" => shapes += 1,
            "svg" | "g" | "defs" | "clipPath" | "mask" | "linearGradient" | "radialGradient"
            | "stop" | "title" | "desc" => (),
            _ => return false,
        }
        for attr in node.attributes() {
            if attr.namespace().is_some()
                || !matches!(
                    attr.name(),
                    "viewBox"
                        | "width"
                        | "height"
                        | "id"
                        | "d"
                        | "x"
                        | "y"
                        | "x1"
                        | "y1"
                        | "x2"
                        | "y2"
                        | "cx"
                        | "cy"
                        | "r"
                        | "rx"
                        | "ry"
                        | "fx"
                        | "fy"
                        | "points"
                        | "transform"
                        | "fill"
                        | "fill-rule"
                        | "fill-opacity"
                        | "stroke"
                        | "stroke-width"
                        | "stroke-linecap"
                        | "stroke-linejoin"
                        | "stroke-miterlimit"
                        | "stroke-dasharray"
                        | "stroke-dashoffset"
                        | "stroke-opacity"
                        | "opacity"
                        | "clip-path"
                        | "clip-rule"
                        | "clipPathUnits"
                        | "mask"
                        | "maskUnits"
                        | "maskContentUnits"
                        | "gradientUnits"
                        | "gradientTransform"
                        | "spreadMethod"
                        | "offset"
                        | "stop-color"
                        | "stop-opacity"
                        | "preserveAspectRatio"
                        | "vector-effect"
                        | "color"
                        | "version"
                        | "role"
                        | "aria-hidden"
                        | "focusable"
                )
            {
                return false;
            }
            let value = attr.value().trim();
            if value.contains("url(")
                && !(value.starts_with("url(#")
                    && value.ends_with(')')
                    && value.matches("url(").count() == 1)
            {
                return false;
            }
        }
    }
    shapes > 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_names() {
        for name in NAMES.lines() {
            assert!(Icon::Name(format!("reicon:{name}")).valid());
        }
        for name in ["", "folder", "reicon:unknown", "reicon:../folder"] {
            assert!(!Icon::Name(name.into()).valid());
        }
    }

    #[test]
    fn self_contained_vectors() {
        let shape = r#"<path d="M4 12h16" stroke="currentColor"/>"#;
        let svg = |content: &str| {
            format!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24">{content}</svg>"#
            )
        };
        assert!(valid_svg(&svg(shape)));
        for content in [
            "",
            "<script/>",
            "<image href='file:///tmp/x'/>",
            "<use href='https://example.invalid/icon.svg'/>",
            "<style/>",
            "<foreignObject/>",
            "<path onload='run()'/>",
            "<path fill='url(https://example.invalid)'/>",
        ] {
            assert!(!valid_svg(&svg(content)), "accepted {content}");
        }
        assert!(!valid_svg(&svg(shape).replace("0 0 24 24", "0 0 0 24")));
        assert!(!valid_svg(&svg(shape).replace("0 0 24 24", "0 0 NaN 24")));
        assert!(!valid_svg(&svg(&shape.repeat(1024))));
        assert!(!valid_svg(
            "<!DOCTYPE svg [<!ENTITY icon 'x'>]><svg viewBox='0 0 24 24'>&icon;</svg>"
        ));
        let value = Icon::Svg { svg: svg(shape) };
        assert_eq!(
            serde_json::from_value::<Icon>(serde_json::to_value(&value).unwrap()).unwrap(),
            value
        );
        assert!(serde_json::from_str::<Icon>(r#"{"svg":"x","url":"x"}"#).is_err());
    }
}
