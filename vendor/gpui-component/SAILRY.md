# Source and local changes

Source: `longbridge/gpui-kit`, revision
`1ccf011620da24d633f7cd6d3c11ca2e97798dcf`, `crates/component` (0.7.1).
The Apache-2.0 license and upstream notices are retained. Workspace manifest
inheritance is resolved in place; product dependencies use the root lockfile.
Component, Base, Assets and the public Kit use the same selected `gpui-fast`
feature and exact `=0.1.3` backend family. Component macros are pinned to this
same Kit source revision.

## Retained patches

Each prior local source delta was replayed or replaced only after comparison with
the current upstream owner. Geometry, focus, accessibility, input state and
animations remain owned by the original Kit components.

| Change | Disposition and reason |
| --- | --- |
| Single-line Input padding and font-relative line height | Retained; `#3343` preserves the Base root height, but a fixed rem line height still exceeds the smallest control's bordered interior. The existing font-relative line height and centered line retain all semantic control heights; multiline padding stays upstream |
| Optional `switch.checked.background` color with primary fallback and explicit control override | Retained; independent checked-track treatment is absent upstream. Reusable upstream candidate |
| TabBar equal-width sizing | Retained; upstream has no equal-width API. Native wrappers, selected indicator and current clicked-tab reveal remain in use. Reusable upstream candidate |
| `Button::without_tooltip` | Retained; clears composed default tooltip without replacing the Button. Reusable upstream candidate |
| Notification intrinsic content width | Retained; upstream always fills the stack's width. Native card styling, placement, dismissal and animation stay in Kit. Reusable upstream candidate |
| Notification viewport confinement | Retained; maximum card width is bounded by the active viewport |
| `WindowExt::set_notification_insets` and deferred toast priority | Retained; Windows native children still require a region beside a WebView. Zero insets preserve the full viewport; deferred Root painting also participates in Fast composition. Reusable upstream candidate |
| DataTable empty stripes | Retained opt-in inert filler rows and viewport redraw on measurement; row count and selection do not change. Reusable upstream candidate |
| PopupMenu selection skips inert rows | Retained; keyboard traversal and submenu entry never select labels, separators or disabled rows. Reusable upstream candidate |
| Popover and DropdownMenu anchor offsets | Retained pass-through of Base's optional anchor adjustment. Reusable upstream candidate |
| AreaChart optional fill baseline | Retained; defaults preserve upstream fill-to-bottom behavior. Native scale, path cache and hover stay authoritative. Reusable upstream candidate |
| Bar/Candlestick perpendicular grid columns and Candlestick value labels | Retained opt-in builders using native Grid and PointAxes. Current upstream chart entrance and reveal are preserved. Reusable upstream candidate |
| Candlestick label gutter and shared value scale | Retained; value labels, candles and hover use the same native plot bounds and scale |
| GroupBox surface without an empty footer wrapper | Retained; the upstream wrapper still prevents relative full-height surfaces. Existing footer composition is unchanged. Reusable upstream candidate |
| Dialog full-screen content and outer text-selection scope | Retained inside the native dialog lifecycle for immersive approved surfaces. Reusable upstream candidate |
| Attachment native preview Button, accessibility label and transparent highlight option | Retained; preserves keyboard activation and accessible image-card names. Reusable upstream candidate |
| Attachment retry-slot identities | Retained; media and description retries keep independent pointer and focus state. Reusable upstream candidate |
| Direct Scrollable helper viewport anchoring | Retained; child scrollbar overlays use the handle's viewport, while stationary sibling helpers use layout bounds. Reusable upstream candidate |
| Radio label width | Retained; label flexes and shrinks beside the native fixed indicator, preserving first-line alignment. Reusable upstream candidate |
| Native submenu icons | Retained on macOS and Windows; fallback metadata retains the same optional icon. Reusable upstream candidate |
| `ListItem::hover_background` | Retained; only hover fill changes. Selection and event handling remain in Kit. Reusable upstream candidate |
| Tooltip trigger lifetime | Retained keyed owner using Base's request cancellation; current TooltipDefaults controls timing. Reusable upstream candidate |
| MessageScroller list-state access | Retained shared viewport access for native drag selection. Reusable upstream candidate |
| Diff vertical wheel capture and row-height hints | Composes the existing ListState ScrollableMask so an outer capture-phase scroller cannot consume wheel input before Diff's list. Restores hints after List invalidates them on width changes and uses painted, device-snapped heights for unwrapped rows, preserving virtualized scroll bounds, clipping and boundary chaining. Reusable upstream candidate |
| Vendored test fixture paths and license copies | Retained packaging adaptations; tests cannot read omitted external checkout fixtures |

These reusable changes can be prepared as separate upstream contributions.
Kit default colors, radii, spacing and card composition remain upstream. No
upstream publication is part of the vendored refresh.

## Retired patches

| Previous change | Upstream replacement |
| --- | --- |
| Medium input/control text-size overrides | Current `Size::input_text_size` supplies the same small text default through Kit's sizing API. Switch, Checkbox, Radio, OTP and Toggle use that API |
| Old icon-generation build-script explanation | Current Assets supplies the shared icon enum and retains the Cargo icon-directory metadata; the current Component build script is copied unchanged |
| GPUI snapshot-only dependency override | Current upstream already pins the pre family and defines coherent Fast feature propagation |
| Floating Surface renderer extension | Removed by product decision together with the application glass renderer. Native Kit fills, borders, shadows, animation and focus behavior remain authoritative. Interaction and geometry tests observe native surfaces directly |
| Selected Tab color substitutions | Restored the pinned upstream Tab and indicator colors. Existing upstream `tab_active_foreground` uses remain unchanged |
| Checkbox and Radio primary-button color substitutions | Restored upstream `primary` and `primary_foreground`. Button already uses upstream's independent button tokens and was not changed |
| Checked Switch primary-button fallback | Restored upstream `primary` as the fallback. The optional `switch.checked.background` key and explicit control override remain for installed theme compatibility |
| Shared `surface_radius` and opaque popover outlines | Restored upstream surface-specific radii and the `foreground.alpha(0.1)` popover ring |
| Compact notification card composition | Restored upstream card spacing, title weight, absolute icons and hover close affordance. Content width, viewport confinement, insets and deferred priority remain |
| List and Command search-header vertical insets | Restored upstream input height and divider geometry |
| `List::search_divider` and Select search-header separation | Restored upstream search dividers and List padding |
| `PopupMenuLayout` and Select row insets/gaps | Removed application-wide presentation geometry; restored upstream menu rows, item radii and Select spacing. Clickable-row traversal remains |
| Segmented outer-inset override | Restored upstream TabBar padding; opt-in equal-width sizing remains |
| Intrinsic DialogClose and DialogAction wrappers | Restored upstream full-size footer wrappers. Child controls retain their native sizing and activation |
| Candlestick `chart_grid` color substitution | Restored upstream `border` grid color locally. Grid columns, value labels, plot gutter and hover opt-ins remain; shared PointAxes styling is unchanged |

The previous standalone lockfile described the old snapshot. Product builds use
the root Cargo.lock; standalone test resolution must be refreshed for this source.
Existing focused coverage remains beside retained patches and application
interaction tests. Neither source comparison nor test presence claims runtime or
platform acceptance.

## Speech completion and input drain

The selected upstream speech source is from `1ccf011620da24d633f7cd6d3c11ca2e97798dcf`.
Sailry supplies its controller-owned CPAL 0.18.2 input and SenseVoice recognizer;
the upstream `speech` feature and its default CPAL capture remain disabled.

`SpeechState::wait_for_final` opts out of the default transcript timeout.
`drain_input_on_stop` and `AudioSink::finish` provide an opt-in end-of-input
handshake for asynchronous capture: queued PCM drains before recognition flushes,
and duplicate or replaced-session completions are ignored. Other consumers retain
the upstream default timeout and immediate recognition flush. The tail, delayed
final result, source completion and stale completion are covered by focused tests.
