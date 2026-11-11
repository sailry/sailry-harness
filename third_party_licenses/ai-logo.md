# AI Logo brand marks

The Desktop draws the marks of model providers and other brands from
[AI Logo](https://ailogo.yldm.ai/), linked as the Go module
`github.com/yldm-tech/ai-logo/mygo` at `v1.20.0`, the version pinned in
`go.mod`. The module embeds the collection as one archive of SVG files;
`internal/desktop/icons` resolves a mark by its slug and variant, and plugins
name one as `ailogo:<slug>`. No file of the collection is copied into this
repository. The module's optional `avatar` package of raster avatars is not
linked.

AI Logo is MIT licensed, Copyright (c) 2023 LobeHub and Copyright (c) 2026
yldm-tech; it is a fork of [lobe-icons](https://github.com/lobehub/lobe-icons).
Its non-AI service marks are ported from Simple Icons (CC0 1.0), apart from the
marks its notices list with the license their brand owner publishes them
under. The license and those notices are the files `LICENSE` and
`THIRD_PARTY_NOTICES.md` at the root of the module in the Go module cache,
which are authoritative. Release packaging copies both into the package
(`internal/node/host/deployment/licenses.sh` collects `THIRD_PARTY_NOTICES*`
with the license files).

All brand names, logos and trademarks belong to their respective owners. A
mark identifies its owner's product or service and implies no endorsement of
Sailry or of a plugin that shows it; the license above covers the artwork's
files, not the trademarks.
