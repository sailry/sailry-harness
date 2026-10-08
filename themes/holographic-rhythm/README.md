# Holographic Rhythm

A portable Sailry v1 theme with a blue-to-pink virtual singer, a holographic sky
city, pink technology icons and translucent cool-blue surfaces.

Import this directory through Settings → Appearance → Import. Choose either
light or dark mode. Repository theme directories are not installed automatically.
The package uses Sailry's v1 theme hooks, including independent switch tracks.

Both modes include a continuous window wallpaper, a transparent welcome hero,
four card icons, sidebar group icons, circuit textures, composer and message
ornaments, wordmark colors, tabs and global button colors. Send and Stop use
solid contrasting fills with Kit vector glyphs. Standard feature-rail glyphs
retain their shapes and semantic control colors.

The wallpapers use `cover`; the small circuit texture repeats on the cards.
All assets are compressed PNGs. Background opacity is kept at full strength;
the existing window surface preferences control its visibility through panels.
Changing those preferences can change text contrast. The package checks cover
the default main-panel tint, card states and modal control colors.

Cyan and pink stars twinkle over the skyline; the portrait has a soft cyan halo
and pink glints, with slower holographic specks in conversations. Light mode
uses lower opacity. Settings → Appearance → Theme animation controls these layers.

Run the shared checks from the Sailry repository, passing this directory (or an
absolute path to a theme in another repository):

```sh
python3 scripts/check-theme.py themes/holographic-rhythm
./target/debug/sailry-desktop --check-theme themes/holographic-rhythm
```

The first command audits explicit solid color contrast. The second uses an
existing Desktop binary's strict import validator without starting a window,
Node or profile. See [shared checks](../README.md#checking-packages) for PNG
decoding and wallpaper checks. No package-specific test suite is needed.
These checks do not establish native UI interaction acceptance.
Artwork provenance and generation prompts are
in [ARTWORK.md](ARTWORK.md). Theme and original generated artwork use Apache-2.0.
