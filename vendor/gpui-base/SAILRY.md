# Source and local changes

Source: `longbridge/gpui-kit`, revision
`1ccf011620da24d633f7cd6d3c11ca2e97798dcf`, `crates/base` (0.7.1).
The Apache-2.0 license and upstream notices are retained. Development examples
excluded by the upstream package are omitted. Workspace manifest inheritance is
resolved in place; the application uses the root Cargo.lock.

The selected application backend is Kit's `gpui-fast` feature, pinned to
`=0.1.6` for the entire Fast family. Upstream's `gpui-pre =0.3.8` dependencies
remain as required by its additive features. `gpui_macros` remains the Kit-selected
`gpui-pre-macros =0.3.8`; it is not an independent GPUI engine selection.

## Retained patches

Every local source delta from the previous `0c830f4d` snapshot was compared with
this upstream revision. The following changes still have no equivalent upstream
API or preserve an approved Sailry presentation choice.

| Change | Disposition and reason |
| --- | --- |
| Indexed shaped-glyph positions and wrap boundaries | Retained; preserves first-match lookup for repeated and non-monotonic glyph indices while avoiding paragraph rescans. Reusable upstream candidate |
| InlineFlow text alignment and shared source/link geometry | Retained; the pinned upstream ignores text alignment. Source and hover-link handling preserve original text across visual fragments for selection, copying and pointer behavior. Reusable upstream candidates |
| TextViewStyle hover-underline setting | Retained; optional public refinement preserves theme ownership. Reusable upstream candidate |
| Table nowrap header floors and ordinary body wrapping | Retained and rebased into upstream's measured column layout and cache. Native code padding, header typography and custom-cell measurement remain authoritative. Reusable upstream candidate |
| Document root identity | Retained; scopes the existing selectable text geometry and survives current upstream flow-position calculation |
| Editor gutter-background override; hidden read-only caret and active row | Retained inside the native editor. Reusable upstream candidate |
| Public clipboard and undo transaction operations | Retained for linked editing surfaces that share the existing input engine and history; no second history owner. Reusable upstream candidate |
| Textarea presentation decorations and clickable link spans | Retained; Composer needs links and decorations while preserving Textarea auto-growth and Enter policy. Editing, selection and history remain in the shared input engine. Reusable upstream candidate |
| LinkSpan hit regions | Retained as a shared shaped-text geometry adapter for selectable text and Textarea link spans. Reusable upstream candidate |
| Popover Enter/Space dispatch | Retained; content controls keep keyboard activation instead of toggling the enclosing popover. Reusable upstream candidate |
| Popup and Popover anchor offsets | Retained; the application can translate a native anchor while retaining upstream placement and viewport clamping. Reusable upstream candidate |
| Window-owned automatic text selection order | Corrects Fast partial repaint: retained automatic participants keep committed slots; a complete painted frame updates real document order. Incomplete fresh passes request one full frame instead of guessing retained sibling positions. The existing selection frame callback also runs when its lifecycle layer is retained. Reusable upstream candidate |
| Nested selection drag scrolling | Retained; synthetic wheel events update the selection and stop at its viewport boundary, while ordinary wheel chaining is unchanged. Reusable upstream candidate |
| Editor completion and code-action menu key consumption | Retained; a handled overlay key cannot later insert text, indent or submit. Reusable upstream candidate |
| Tooltip request owner and trigger-release cancellation | Retained alongside upstream TooltipDefaults and per-request delays; an unmounted trigger cancels only its own pending or visible tooltip. Reusable upstream candidate |
| Tooltip hitbox occlusion | Retained; a visible managed tooltip participates in ordinary Kit pointer handling |
| Public DropTarget constructor | Retained visibility-only change for native Dock interaction fixtures. Reusable upstream candidate |

InlineFlow keeps upstream's new shaped-width correction for proportional fonts.
Native code padding participates in wrapping and painted width measurement.
Upstream table header typography and custom-cell measurement remain authoritative.

## Retired patches

| Previous change | Upstream replacement |
| --- | --- |
| Local horizontal input/caret clamping helper and paint offsets | `#3358` centralizes horizontal clamping and applies the final offset consistently to all carets. Deferred targets call that shared helper |
| Explicit old line-wrapper signature adaptation | Current upstream callers already use the selected wrapping APIs, including SameIndent and measured editor wrapping |
| Header-weight-only table measurement override | Current TableTextStyles resolves the complete header/body refinements, and its cache retains those resolved inputs. Only the unsupported decorations and nowrap floors above remain local |
| GPUI 0.3.7-to-0.3.8 dependency update | Current upstream already pins that pre family and supplies the selected Fast feature |
| TextViewStyle break-all and link-icon options, measured link decorations and wrapping helpers | No product callers remain; wrapping follows the pinned upstream policy, and conversation link icons belong to the content crate |
| TextViewDefaults style getter | No product caller uses it; the existing style setter and semantic theme configuration remain authoritative |
| Configurable source-edge inline-code insets and radii | Product accepts upstream presentation: every native code fragment uses the upstream four-pixel padding and semantic small radius. Decoration-only wrapping helpers and cache fields were removed |
| List, continuation-paragraph and adjacent bullet/task group spacing | Product accepts upstream spacing and the native tight/loose list distinction through `spread` |

The old standalone lockfile described the previous source snapshot and is not
carried into the refreshed package. Product dependency resolution is owned by the
root Cargo.lock. Standalone vendored test runs must use the selected development
volume for their target and temporary files.

The retained focused tests cover lookup parity, Unicode wrapping,
selection and copied source, native code padding, text alignment, Popover keyboard input,
nested drag scrolling, link annotations and handled editor overlay keys. Keeping
tests beside these patches does not establish application or platform acceptance.
