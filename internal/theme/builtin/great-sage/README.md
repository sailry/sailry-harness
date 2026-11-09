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
Switch thumbs use warm ivory in light mode and deep brown in dark mode. The
checked track uses `primary.background`, independently of primary buttons. The
unchecked dark track is lighter warm bronze for contrast against settings
panels.
Send and Stop use ivory fills and dark Kit vector glyphs in dark mode, reversing
to dark fills and ivory glyphs in light mode. Their colors are independent of
ordinary buttons. Kit supplies hover, pressed and disabled states.
Window artwork uses 30% opacity in light mode and 60% in dark mode. The dark
main tint carries 90% alpha, multiplying the user's panel tint to reveal more
clouds and red ink. The
content panel repeats the existing 256-pixel grain texture at 8% opacity in
light mode and 12% in dark mode, so wallpaper gaps still have material detail.

Decorative motion adds 3–5 pixel gold and vermilion embers above the content and
navigation panel tints, with ambient window sparks, a gold halo behind the
welcome portrait and slower conversation sparks. Light mode uses deeper colors
and lower opacity. Settings → Appearance → Theme animation controls these layers.

Package checks are independent of Sailry's source and build:

```sh
python3 -m pip install -r requirements-dev.txt
python3 -m unittest discover -s tests -v
```

These checks validate PNG resources, resource budgets, wallpaper resolution and
transparency, button, switch and modal-tab contrast, clear gilding caption space, and
translucent card states. Native import and control interaction
behavior belongs to Sailry's generic theme mechanism tests.
