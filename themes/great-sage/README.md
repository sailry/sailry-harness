# Great Sage

A black, gold, and red theme inspired by Sun Wukong, with original character art,
ink landscapes, auspicious clouds, and gold-leaf button edges.

Import this directory in Sailry's **Settings → Appearance** and select
**Great Sage**. Light and dark variants share compressed transparent PNG artwork.
The package uses the existing v1 theme contract and needs no theme-specific
application code. [ARTWORK.md](ARTWORK.md) contains generation prompts and
provenance; the Apache-2.0 license is included in `theme.json`.

Primary buttons share solid warm-black fills and ivory-gold labels,
including hover and pressed states. The new-session action adds a transparent
gold-leaf texture clipped to the button edges through `button.new_session`.
Ordinary buttons use the global Kit color tokens; they do not inherit this
artwork slot. Tabs use translucent copper-gold selected fills and separate
darkened tracks, including segmented controls in dialogs.
Switch thumbs use warm ivory against distinct dark tracks in both variants.
Window artwork uses 30% opacity in light mode and 55% in dark mode. Warm-black
panel tints retain the clouds and red ink without a neutral black void. The
content panel repeats the existing 256-pixel grain texture at 8% opacity in
light mode and 16% in dark mode, so wallpaper gaps still have material detail.

Package checks are independent of Sailry's source and build:

```sh
python3 -m pip install -r requirements-dev.txt
python3 -m unittest discover -s tests -v
```

These checks validate PNG resources, resource budgets, wallpaper resolution and
transparency, button, switch and modal-tab contrast, clear gilding caption space, and
translucent card states. Native import and control interaction
behavior belongs to Sailry's generic theme mechanism tests.
