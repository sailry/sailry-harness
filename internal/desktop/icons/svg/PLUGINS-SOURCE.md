# Plugin icon sources

The plugin icon resolver uses these embedded assets for manifest declarations,
core script icons and native script controls. The assets keep their original
vector paths and follow the current theme through `currentColor`.

## Reicon

Source: https://github.com/dqev/reicon/tree/109d994d810342a998e79ab65572c68176bc2e0a

The `reicon-*.svg` glyphs come from the pinned Outline collection. Source
`data/icon-data.json` SHA-256:
`a50139172599b15f1e198f66efe2716e39924e763fd2282eb5155857efe3f3ce`.
Portable `reicon:category/name` declarations map to
`reicon-category-name.svg`. The loader's 24 × 24 SVG wrapper is retained;
the contained paths are unchanged.

Reicon is licensed under MIT; see `LICENSE-REICON`. Upstream credits Solar
Icons by 480 Design, licensed under CC BY 4.0
(https://creativecommons.org/licenses/by/4.0/), and Zappicon
(https://zappicon.com/). These upstream attributions are retained here.

## Kubai Icons

`kubai:<id>` declarations and package emblems are not embedded here. They are
drawn by `github.com/yldm-tech/mygo-kubai-icons` at the version pinned in
`go.mod`; see `third_party_licenses/kubai-icons.md`. Kubai icons are colored
emblems with no monochrome form, so the Reicon outlines above remain the
glyphs of every text-colored slot.

## Remix Icon

Source: https://github.com/Remix-Design/RemixIcon/tree/8bc436bdd329cf342958d8effdd078bfa74354c6/icons/System

The following unchanged icons are from Remix Icon v4.6.0 by Remix Design:

- `remix-progress-2.svg`: `progress-2-line.svg`
- `remix-progress-4.svg`: `progress-4-line.svg`
- `remix-progress-6.svg`: `progress-6-line.svg`
- `remix-checkbox-blank-circle-line.svg`: `checkbox-blank-circle-line.svg`
- `remix-close-circle-line.svg`: `close-circle-line.svg`

Licensed under Apache License 2.0; see `LICENSE-REMIX`.

## Lucide

`calendar.svg`, `database.svg`, `play.svg` and `sliders-horizontal.svg` preserve
the Lucide assets bundled by the pinned gpui-component revision
`4424427e60fcc9a95cd2a7daeacd77cc51b92e85`.

Source: https://github.com/dux-web/gpui-component/tree/4424427e60fcc9a95cd2a7daeacd77cc51b92e85/crates/assets/assets/icons

The bundled ISC and applicable Feather MIT notices are retained in
`LICENSE-LUCIDE`.
