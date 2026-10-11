# Source and local changes

Source: `longbridge/gpui-kit`, revision
`0c830f4d257e69fdd17200650533ab4ca9a40cc0`, `crates/component` (0.7.0).
The Apache 2.0 license and upstream notices are retained. Workspace manifest
inheritance is resolved in place; product dependencies use the root lockfile.

The optional `switch.checked.background` semantic color separates a checked
Switch track from primary buttons. The pinned control otherwise shares the
primary-button treatment, making tracks indistinct in themes with dark buttons.
Its omitted-color fallback remains the primary button treatment; explicit
`Switch::color` overrides still take precedence. The existing unchecked track,
thumb, geometry, disabled states, accessibility and keyboard handling remain
owned by Kit. The theme schema test covers both independent color and fallback.

The pinned Segmented Tab and TabBar use the page background for the selected
fill, including the animated indicator, without a public color override.
The local patch reads the existing `tab_active` / `tab_active_foreground` theme
pair for selected tabs and their indicators. Geometry, animation, focus,
accessibility, and input behavior remain in Kit. This lets
Sailry configure contrasting selected colors through its single theme rather
than reproducing the tab component or changing the page background.
This is a Sailry presentation choice that changes the default colors of all
segmented tabs using this patched dependency.

Animated tab wrappers also prevented application tabs from sharing their actual
container width. The added `TabBar::equal_width` option distributes space through
those wrappers and the existing scroll row, retaining built-in gaps and insets.
SSH, database, and project forms use it instead of estimating widths from the
window. The SSH interaction test checks symmetric end insets when selecting the
first and last tabs in both narrow and wide windows, on local and remote Nodes.
Equal-width tabs use a flexible wrapper for every variant, including Tab and
Outline without an animated indicator. Desktop interaction tests verify equal
widths, viewport containment and clicks for all five variants at narrow and wide
widths; the existing segmented-tab tests retain their inset and alignment checks.

All segmented bars retain their inset at the outer container instead of
canceling it with nested scroll margins. Indicators use the measured tab offset
directly, without adding the inset again. Desktop component tests cover both
content-sized and equally distributed bars in both theme modes.

Desktop theme and appearance interaction tests cover selected contrast,
theme switching, and package selection. Upstream source tests are retained.

`Button::without_tooltip` clears a tooltip supplied by a composed control; Sailry
uses it for MessageScroller's jump button while retaining its click and focus
behavior. The pinned component had no way to remove that default tooltip.
`NotificationSettings::content_width` makes individual cards use their intrinsic
content width within the existing stack's maximum width. Stack placement,
animations, dismissal, and accessibility remain owned by Kit; the maximum width
is confined to the viewport. Sailry enables this through its common theme.

`DataTable::empty_stripes` reuses the table's existing inert viewport filler rows
when its delegate has no data. The pinned table otherwise replaces those rows
with its empty-state slot. It is opt-in alongside `stripe`, leaves the delegate's
row count unchanged, and retains native headers, resizing and scrolling. Empty
striped tables redraw after viewport measurement because no data change would
otherwise update their filler count.

## List

`List::search_divider` optionally hides the native search-input separator. The
pinned List always draws it and exposes no override. Defaults are unchanged;
Sailry hides it only for an empty conversation query. Search state, input,
focus, keyboard navigation and result rendering remain owned by Kit.

The pinned Select does not expose its native List's search-divider option or a
search-header style slot. Its dropdown composition now uses
`List::search_divider(false)` to omit the separator beneath searchable dropdown
inputs. Other List separators retain their existing defaults. Desktop's shared
select interaction test verifies the header height without the one-pixel border,
popup insets, filtering, keyboard confirmation and dismissal. Retire this patch
when upstream exposes an equivalent Select option.

## Chart

Kit 0.7.0, revision `0c830f4d257e69fdd17200650533ab4ca9a40cc0`, adds
signed domains and tooltip formatters, but `AreaChart::paint` still closes all
fills at the plot's bottom. The opt-in `AreaChart::baseline(Y)` projects a fill
baseline through the same native y scale. An unset baseline retains upstream
behavior, and path caching and hover ownership are unchanged. Sailry uses zero
for incoming/outgoing transfer areas, removing its custom Plot implementation.
The focused native test covers the default, zero and nonzero projected values.
Retire this patch when upstream exposes an equivalent fill-baseline API.

The pinned `BarChart` has value-axis grid lines but no perpendicular grid option,
and `CandlestickChart` lacks value-axis labels and vertical grid builders.
Opt-in `grid_columns` extends both controls through native `Grid` rendering;
the candle's opt-in `y_axis` reuses `PointAxes` label measurement and painting.
Their defaults retain zero perpendicular lines and hidden candle value labels.
The candle grid now uses the existing `chart_grid` semantic color consistently
with line, area and bar charts. Price scales, native hover, and bullish/bearish
colors remain owned by Kit. Sailry's Cartesian chat charts enable the options;
their focused interaction test checks both grid directions, label gutters,
theme changes, resizing and native hover. Retire these builders when upstream
provides equivalent options.

## GroupBox

Kit 0.7.0 adds an auto-height footer-slot wrapper even when there is no footer.
That wrapper prevents a full-height surface from resolving its relative height.
The local fix renders the native surface directly when the footer is absent,
retaining the original footer slot and gap whenever a footer exists. Native
tests cover full-height and intrinsic-height surfaces plus the footer gap;
Desktop tests retain centered and top-aligned full-height Empty card variants.

## Root and approved surfaces

Kit 0.7.0's `DialogClose` and `DialogAction` wrappers impose full width and
height, stretching footer controls despite `DialogFooter`'s native end alignment.
The wrappers now keep their intrinsic content size. Base still owns cancel
activation, and the existing dispatch anchor still owns confirmation; keyboard,
focus, accessibility and footer spacing are unchanged. Desktop's shared dialog
interaction tests cover intrinsic widths, right alignment, cancellation and
confirmation. Retire this patch when upstream removes the full-size wrappers.

Kit 0.7's Base Root registers and renders Component's dialog, sheet and toast
plugins automatically. Sailry no longer adds those layers in application views.
The opt-in `WindowExt::set_notification_insets` retains the workspace toast
region beside an embedded native browser; upstream automatic toast layout had
no inset API. Zero insets preserve the upstream viewport. Kit still owns layer
creation, focus, positioning, dismissal and animation.

The existing floating-surface renderer adapter is rebased onto the new dialog
composition, with the native text-selection scope applied to the complete
dialog layer. It changes background painting, not controls or selection state.

Attachment preview activation uses a native Button rather than the upstream
pointer-only click div, retaining keyboard focus and activation for image cards.
`Attachment::accessibility_label` names image-only preview buttons, and the
existing `highlight(false)` option preserves approved transparent history
images. Composer cards use upstream Medium sizing, built-in remove/retry,
progress, scrims, tooltip and horizontal AttachmentGroup scrolling unchanged.

Failed file cards can render both a media retry button and a description retry
link. The pinned component gave both the same element identity, sharing native
mouse-down and focus state; the first control could clear a click intended for
the second. Slot-specific retry identities keep those states independent without
changing the native controls, layout or event handlers. The focused regression
test covers pointer activation, distinct Tab focus, and Enter/Space activation
for both controls on one failed file card. Retire this patch when upstream gives
the retry slots independent identities.

## Scrollable

The pinned `ScrollableElement::scrollbar` helper forces layout-bounds mode even
when its overlay is a child of the scrolling element. GPUI translates that
child with the content, so the track and its hitboxes leave the viewport as the
list scrolls. The direct helper now uses Base's existing handle-reported
viewport; the `overflow_*_scrollbar` wrapper keeps its stationary sibling
overlay and explicit layout-bounds mode. Native interaction tests cover both
axes, child and sibling placement, and sidebar thumb dragging after wheel
scrolling. Retire this patch when upstream keeps direct helper tracks anchored
to the handle viewport.

## Input

The pinned Radio gives its label the full row width beside a fixed indicator
and gap, letting long options overflow their available space. The label column
now flexes within the remaining width, can shrink to zero, and wraps normally.
The indicator keeps its existing first-line alignment; focus, keyboard,
accessibility, and checked state remain in Kit. Desktop question interaction
tests cover long Unicode options, bounds, alignment, and selection on local and
remote Nodes. Retire this patch when upstream constrains radio label width.

The pinned Input applies fixed single-line frame heights, size-dependent vertical
padding and a 1.25 rem line height together. The remaining interior is shorter
than the line for Medium, Small and XSmall, clipping text and placeholders.
Single-line frames now let Base's existing full-height state center the line
without extra vertical padding, and resolve the line height against the selected
font size so XSmall fits its unchanged frame. Horizontal padding, size APIs,
focus, editing and accessibility remain in Kit. Textarea and Editor retain their
existing line height and internal padding. Desktop Kit interaction tests cover
the standard sizes, rem scaling, placeholders, prefixes and suffixes, pointer
and keyboard editing, and multiline geometry. Retire this patch when upstream
provides fitting single-line geometry for the same sizes.

## GPUI snapshot

The exact GPUI dependency family is updated from 0.3.7 to 0.3.8, retaining
the Kit 0.7.0 source revision and the patches above. This follows the dependency
selection in upstream Kit revision `87d10ae5e1299d1be18670a37570c5402f952e34`
without taking its unrelated component changes. The Component public API
and presentation patches are unchanged.

Native menu submenus accept optional icons through `submenu_with_icon`; the pinned Kit only supported icons on leaf items. This keeps conversation export on the OS-native menu path on macOS and Windows.
