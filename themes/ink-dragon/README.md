# Gilded Ink Dragon

A modern monochrome ink theme with a dragon coiled around mountain peaks,
silver mist, expressive brush strokes, and restrained gold foil accents.

Import this directory in Sailry's **Settings → Appearance** and select
**Gilded Ink Dragon**. The package contains light and dark variants. It uses the
existing v1 theme contract and needs no theme-specific application code.

The window wallpaper extends behind translucent navigation and content panels.
Its center stays clear for conversations. Surface opacity remains controlled by
Sailry's appearance settings. Dark main and navigation colors carry 82% and 92%
alpha respectively, multiplying the user's panel tint so the silver dragon and
mountain ridges remain visible. The wallpaper opacity stays at 60% to retain its
smoky ink appearance. Cards retain translucent hover and pressed states.
Primary buttons use ink-black fills with silver-white labels in both variants.
Surface borders are muted silver gray; gold remains in small artwork accents.
Dark borders use lower-opacity cool gray to avoid bright outlines. Switch thumbs
use silver white against darker tracks in both variants.
Button, tab, message, and wordmark colors follow the selected variant. The four
original card symbols also appear beside sidebar group headings.

All artwork is compressed transparent PNG. Wallpapers retain their generated
1586 × 992 resolution; symbols are 96 × 96, the repeating foil texture is
256 × 256, and corner ornaments are 192 × 192. The button artwork is resized to
768 pixels wide and cropped by the existing decoration slot's cover fit.
[ARTWORK.md](ARTWORK.md) contains
the generation prompts and provenance. The package's Apache-2.0 license is
included in `theme.json`.

Package checks live here so the theme can move into its own repository:

```sh
python3 -m pip install -r requirements-dev.txt
python3 -m unittest discover -s tests -v
```

These checks validate PNG resources, resource budgets, light/dark text and switch contrast,
and translucent card states. They do not launch Sailry or verify native UI
interactions.
