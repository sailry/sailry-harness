# Source and local changes

Source: `longbridge/gpui-kit`, revision
`0c830f4d257e69fdd17200650533ab4ca9a40cc0`, `crates/base` (0.7.0).
The Apache 2.0 license and upstream source notices are retained. Development
examples excluded by the upstream package are omitted. The manifest resolves
upstream workspace settings in place; the root Cargo.lock still selects GPUI.

Local changes:

- Index shaped glyphs and wrap boundaries for long-text hit regions and
  selection. Preserve the original first-matching lookup, including repeated or
  non-monotonic glyph indices, while avoiding repeated full-paragraph scans.
- Extend InlineFlow with grapheme-boundary wrapping, link-icon measurements,
  inline-code insets and alignment. Visual fragments share their original text
  so multi-click selection, dragging and copying retain contiguous source text.
- Add optional TextViewStyle settings for break-all wrapping, link icons,
  hover-only link underlines, and inline-code padding and radii.
  TextViewDefaults exposes its configured style
  so the application can refine it through the existing theme.
- Measure Markdown tables using header weight, link icons and non-wrapping
  header widths. Table cells keep ordinary word wrapping.
- Adjust list and nested-paragraph spacing and separate adjacent bullet/task
  groups. These are Sailry presentation defaults; the patched layout does not
  distinguish tight and loose lists through the upstream `spread` flag.
- Expose an Editor gutter-background override for custom surfaces. Read-only
  editors hide their caret and current row highlight. Both changes apply within
  the existing editor implementation.
- Let controls inside an open Popover handle Enter/Space without accidentally
  toggling the popover closed.
- Update selection during synthetic drag scrolling and stop that scrolling at
  the nested viewport boundary. Ordinary mouse-wheel chaining is preserved.
- Consume completion and code-action menu keys in the Editor before later
  bindings can insert text, indent or submit while an overlay handles the key.

The text and editor APIs are reusable extensions for Kit's TextView and Editor.
The list-spacing defaults are a product presentation change. These patches keep
shaping and selection ownership in Kit. Sailry's conversation Markdown and Git
diffs use separate application content components styled from the same Kit theme.

Regression coverage includes lookup parity, Unicode/grapheme wrapping,
multi-click and drag selection, exact copied text, code insets, text alignment,
Popover keyboard input, nested selection scrolling, and handled editor overlay keys.

The standalone lockfile pins upstream unit-test dependencies. Product builds use
the root workspace lockfile. Build output and test temporary directories must
use the selected development volume rather than the system temporary directory.

Kit 0.7's native inline tokens, multi-line Textarea, root plugins, plotting and
input history remain upstream-owned. The existing `DropTarget::new` constructor
is public so application Dock interaction tests can construct a native target;
the upstream constructor was crate-private. This visibility-only change does
not alter drag handling or introduce another target representation.

## GPUI snapshot

The exact GPUI dependency family is updated from 0.3.7 to 0.3.8, retaining
the Kit 0.7.0 source revision and the patches above. This follows the dependency
selection in upstream Kit revision `87d10ae5e1299d1be18670a37570c5402f952e34`
without taking its unrelated component changes. Line-wrapper calls explicitly
retain `IndentAdjustment::SameIndent`, including Sailry's extra wrap paths.
