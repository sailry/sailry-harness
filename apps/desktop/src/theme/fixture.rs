//! Original, CC0 raster fixtures shared by package and native-renderer acceptance.
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

pub fn manifest(id: &str) -> Value {
    let variant = |dark: bool| {
        let path = if dark {
            "assets/dark.png"
        } else {
            "assets/light.png"
        };
        let mut assets = serde_json::Map::new();
        for slot in super::package::SLOTS {
            assets.insert((*slot).into(), json!({
                "path": path,
                "fit": if slot.starts_with("icon.") || *slot == "brand" { "contain" } else { "cover" },
                "opacity": if slot.starts_with("background.") { 0.12 } else { 0.8 },
                "alignment": [0.0, 0.5],
                "height": if *slot == "hero.new_session" { 100 } else { 36 }
            }));
        }
        json!({
            "theme": { "name": if dark { "Tide dark" } else { "Tide light" }, "mode": if dark { "dark" } else { "light" },
                "colors": {
                    "background": if dark { "#182b36" } else { "#edf5fb" },
                    "group_box.background": if dark { "#243845" } else { "#e0eef5" },
                    "sidebar.background": if dark { "#12232f" } else { "#eaf1f6" }
                }
            },
            "assets": assets
        })
    };
    json!({
        "version": 1, "id": id, "name": "Tide",
        "description": "A coastal theme with light and dark palettes",
        "package_version": "0.1.0",
        "license": { "author": "Sailry", "name": "CC0-1.0", "text": "Original test artwork, dedicated to the public domain under CC0 1.0" },
        "light": variant(false), "dark": variant(true)
    })
}

pub fn write(root: &Path, id: &str) -> PathBuf {
    let directory = root.join(id);
    std::fs::create_dir_all(directory.join("assets")).unwrap();
    for (name, dark) in [("light", false), ("dark", true)] {
        let pixels = image::RgbaImage::from_fn(240, 100, |x, y| {
            let stripe = (y as f32 - 50. - (x as f32 / 32.).sin() * 15.).abs() < 9.;
            image::Rgba(if stripe {
                if dark {
                    [116, 201, 209, 255]
                } else {
                    [46, 116, 143, 255]
                }
            } else if dark {
                [27, 58, 75, 255]
            } else {
                [166, 207, 221, 255]
            })
        });
        pixels
            .save(directory.join(format!("assets/{name}.png")))
            .unwrap();
    }
    std::fs::write(
        directory.join("theme.json"),
        serde_json::to_vec_pretty(&manifest(id)).unwrap(),
    )
    .unwrap();
    directory
}

/// Exercises optional styles without depending on a distributable theme package.
pub fn styled(root: &Path, id: &str) -> PathBuf {
    let directory = write(root, id);
    let mut value = manifest(id);
    for (mode, dark) in [("light", false), ("dark", true)] {
        let variant = &mut value[mode];
        let foreground = if dark { "#D6F1F4" } else { "#173D48" };
        variant["theme"]["colors"]["foreground"] = json!(foreground);
        variant["theme"]["colors"]["tab.foreground"] =
            json!(if dark { "#AFC5D0" } else { "#4E7180" });
        variant["theme"]["colors"]["tab.active.foreground"] = json!("#F1FBFD");
        variant["theme"]["colors"]["tab.active.background"] = json!("#1D6170");
        variant["theme"]["colors"]["tab_bar.segmented.background"] = json!("#16808E80");
        variant["theme"]["colors"]["button.background"] = json!("#16808E18");
        variant["theme"]["colors"]["button.primary.background"] = json!("#1D6170");
        variant["composer"] = json!({"border_width": 1});
        variant["messages"] = json!({
            "user": {
                "background": if dark { "#243845D9" } else { "#E0EEF5D9" },
                "foreground": foreground, "border": "#16808E70",
                "border_width": 1, "radius": 14
            },
            "assistant": { "foreground": foreground }
        });
        variant["wordmark"] = json!({
            "foreground": if dark { "#B5EEF2" } else { "#1C6F7D" },
            "glow": "#55CED9"
        });
        // One continuous wallpaper also covers the navigation panes.
        let assets = variant["assets"].as_object_mut().unwrap();
        assets.remove("background.content");
        assets.remove("background.sidebar");
    }
    std::fs::write(
        directory.join("theme.json"),
        serde_json::to_vec_pretty(&value).unwrap(),
    )
    .unwrap();
    directory
}
