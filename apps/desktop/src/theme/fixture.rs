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
        "license": { "author": "Sailry", "text": "Original test artwork, dedicated to the public domain under CC0 1.0" },
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
