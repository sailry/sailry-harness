# ReUI file types

The embedded catalog contains ReUI's Solid file-type artwork for all 99 extensions,
11 file families, the generic file, folder and blank page. Compact (20 px) and
regular (28 px) editions retain the upstream responsive label rules. The Desktop
renders these SVGs as images rather than monochrome icon masks. Image attachments
continue to show their thumbnails.

Source snapshots:

- ReUI 2.8.0, commit `3b67464bcad3c29fbdb12437afb70153384eee19`:
  [file-type.tsx](https://github.com/keenthemes/reui/blob/3b67464bcad3c29fbdb12437afb70153384eee19/registry-reui/bases/base/reui/file-type.tsx)
  and [MIT license](https://github.com/keenthemes/reui/blob/3b67464bcad3c29fbdb12437afb70153384eee19/LICENSE.md)
- Tailwind CSS 4.3.2, the version selected by that ReUI revision's lockfile:
  [theme.css](https://github.com/tailwindlabs/tailwindcss/blob/v4.3.2/packages/tailwindcss/theme.css)
  and [MIT license](https://github.com/tailwindlabs/tailwindcss/blob/v4.3.2/LICENSE)

`LICENSE-REUI` and `LICENSE-TAILWIND` retain the full upstream notices. Geometry
comes from the public FileType component, not a paid icon pack. Its family glyphs
are defined in that component; there are no imported Lucide glyphs. The generic
sans-serif label uses GPUI's existing font resolution; no font is redistributed.

`generate.py` checks the SHA-256 digests of both source snapshots, extracts their
type table, MIME map, Solid geometry and family glyphs, and converts the pinned
OKLCH colors and neutral OKLab mixes to sRGB. Family strokes compensate for the
native renderer's lack of non-scaling SVG strokes. Label positions, responsive
font sizes and weights follow the upstream rules. No runtime network access,
JavaScript runtime, React or Tailwind dependency is required.

Regenerate or verify offline:

```sh
python3 generate.py .
python3 generate.py . --check
python3 -m unittest test_generate.py
```

The catalog stores the SVG strings in one JSON resource, matching the product's
existing embedded-icon catalog pattern. Asset lookups accept only known catalog
entries under `icons/reui-file-type/compact/` or `icons/reui-file-type/regular/`.
