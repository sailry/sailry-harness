# Great Sage

A black, gold, and red theme inspired by Sun Wukong, with original character art,
ink landscapes, auspicious clouds, and a gold circlet button frame.

Import this directory in Sailry's **Settings → Appearance** and select
**Great Sage**. Light and dark variants share compressed transparent PNG artwork.
The package uses the existing v1 theme contract and needs no theme-specific
application code. [ARTWORK.md](ARTWORK.md) contains generation prompts and
provenance; the Apache-2.0 license is included in `theme.json`.

Primary buttons share burgundy lacquer gradients and warm ivory-gold labels,
including hover and pressed states. The new-session action adds a transparent
engraved circlet frame with cloud curls through `button.new_session`. Ordinary
buttons use the global Kit color tokens; they do not inherit this artwork slot.
Switch thumbs use warm ivory against distinct dark tracks in both variants.
Window artwork uses 25% opacity in light mode and 35% in dark mode, keeping
clouds and red ink behind the conversation less prominent.

Package checks are independent of Sailry's source and build:

```sh
python3 -m pip install -r requirements-dev.txt
python3 -m unittest discover -s tests -v
```

These checks validate PNG resources, resource budgets, wallpaper resolution and
transparency, button and switch contrast, clear frame caption space, and
translucent card states. Native import and control interaction
behavior belongs to Sailry's generic theme mechanism tests.
