# Great Sage

A black, gold, and red theme inspired by Sun Wukong, with original character art,
ink landscapes, auspicious clouds, and a gold circlet button trim.

Import this directory in Sailry's **Settings → Appearance** and select
**Great Sage**. Light and dark variants share compressed transparent PNG artwork.
The package uses the existing v1 theme contract and needs no theme-specific
application code. [ARTWORK.md](ARTWORK.md) contains generation prompts and
provenance; the Apache-2.0 license is included in `theme.json`.

Package checks are independent of Sailry's source and build:

```sh
python3 -m pip install -r requirements-dev.txt
python3 -m unittest discover -s tests -v
```

These checks validate PNG resources, resource budgets, wallpaper resolution and
transparency, and translucent card states. Native import and control interaction
behavior belongs to Sailry's generic theme mechanism tests.
