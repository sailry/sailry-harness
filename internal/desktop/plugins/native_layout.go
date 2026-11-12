package plugins

import (
	"encoding/json"
	"slices"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

type modalDraft struct {
	open   bool
	source bool
}

func (binding *NativeBinding) renderLayout(view *ui.Context, element NativeElement, children func(), _ func(CallbackRef, json.RawMessage)) (ui.Element, error) {
	identity, props, err := nativeProps(element)
	if err != nil {
		return ui.Element{}, err
	}
	palette := theme.Of(view)
	switch element.Export {
	case "SettingsPage":
		// The application's own settings page: one centered column.
		return kit.Page(view, identity, nativeProp(props, "title", ""), nativeProp(props, "description", ""), children), nil
	case "SettingsGroup":
		var renderErr error
		first := 0
		headerAction := nativeProp(props, "header_action", false) && len(element.Children) > 0
		if headerAction {
			first = 1
		}
		result := ui.Column(view).FillWidth().Gap(12).Children(func() {
			if nativeProp(props, "heading", true) {
				ui.Row(view).FillWidth().MinHeight(28).Gap(12).Children(func() {
					nativeText(view, nativeProp(props, "title", "")).Grow(1).TextColor(palette.Foreground)
					if headerAction {
						ui.Row(view).Shrink(0).Gap(8).Justify(ui.End).Children(func() {
							renderErr = renderNativeChildren(view, element, element.Children[:1])
						})
					}
				})
			}
			rows := func() {
				if renderErr != nil {
					return
				}
				// A group with an icon on one row keeps every row's text
				// aligned behind the slot.
				lead := binding.state.rowLead
				binding.state.rowLead = slices.ContainsFunc(element.Children[first:], rowHasIcon)
				defer func() { binding.state.rowLead = lead }()
				if nativeProp(props, "sortable", false) {
					renderErr = binding.renderSortable(view, element, identity, props, element.Children[first:])
				} else {
					renderErr = renderNativeChildren(view, element, element.Children[first:])
				}
			}
			if nativeProp(props, "variant", "") == "outline" {
				ui.Column(view).Border(1, palette.Border).Radius(palette.RadiusLg).PaddingX(16).Dividers(1, palette.Border).TextColor(palette.SurfaceForeground).Children(rows)
			} else {
				kit.Card(view, rows)
			}
		})
		return result, renderErr
	case "SettingsRow":
		label, description := nativeProp(props, "label", ""), nativeProp(props, "description", "")
		if nativeProp(props, "wide", false) {
			return kit.WideRow(view, identity, label, description, children), nil
		}
		var control func()
		if len(element.Children) > 0 {
			control = children
		}
		// The slot holds the row's icon, or stays empty beside rows that
		// have one; a name no icon set knows draws nothing.
		icon := nativeIcon(nativeProp(props, "icon", ""))
		var lead func()
		if icon != nil || binding.state.rowLead {
			lead = func() {
				if icon != nil {
					ui.Icon(view, icon).FontSize(rowIconSize)
				}
			}
		}
		return kit.LeadRow(view, identity, lead, label, description, control), nil
	case "CardList":
		return ui.Column(view).FillWidth().Gap(12).Children(func() {
			if len(element.Children) > 0 {
				children()
				return
			}
			if nativeProp(props, "loading", false) {
				// The reference's loading card: two skeleton lines.
				kit.Frost(view, ui.Column(view).FillWidth().Padding(24, 16).Gap(12).Radius(palette.RadiusLg).Border(1, palette.Border), palette.Surface, kit.CardTint).Children(func() {
					kit.Skeleton(view, kit.SkeletonProps{Width: 160, Height: 16})
					kit.Skeleton(view, kit.SkeletonProps{Width: 256, Height: 16, Secondary: true})
				})
				return
			}
			if label := nativeProp(props, "empty_label", ""); label != "" && nativeProp(props, "loaded", true) {
				kit.Result(view, kit.ResultProps{Title: label, Icon: nativeIconOr(nativeProp(props, "empty_icon", ""), icons.Inbox), Card: true}).FillWidth()
			}
		}), nil
	case "CardRow":
		// A card row keeps the application's row metrics.
		return kit.Frost(view, ui.Row(view).FillWidth().MinWidth(0).MinHeight(kit.RowMinHeight).Padding(kit.RowPad, 16).Gap(12).Radius(palette.RadiusLg).Border(1, palette.Border), palette.Surface, kit.CardTint).Children(children), nil
	case "CardColumn":
		// The reference's column widths: w_32 and w_48 metadata, a w_12
		// control, a title that takes the rest from min_w_24.
		column := ui.Column(view).MinWidth(0).Gap(4).FontSize(14)
		switch nativeProp(props, "variant", "content") {
		case "leading":
			column.Shrink(0)
		case "title":
			column.Grow(1).Basis(0).MinWidth(96)
		case "metadata":
			column.Width(128).Shrink(0).TextColor(palette.MutedForeground)
		case "wide_metadata":
			column.Width(192).Shrink(0).TextColor(palette.MutedForeground)
		case "control":
			column.Width(48).Shrink(0).Center()
		case "inline":
			column = ui.Row(view).Shrink(0).Gap(8).FontSize(14).TextColor(palette.MutedForeground)
		case "actions":
			gap := float32(8)
			if nativeProp(props, "spacing", "") == "regular" {
				gap = 12
			}
			column = ui.Row(view).Shrink(0).Gap(gap)
		default:
			column.Grow(1).Basis(0)
		}
		return column.Children(children), nil
	case "CardSummary":
		return ui.Row(view).MinWidth(0).Grow(1).Basis(0).Gap(12).Children(func() {
			children()
			ui.Column(view).Grow(1).Basis(0).MinWidth(0).Gap(kit.TitleGap).Children(func() {
				nativeText(view, nativeProp(props, "title", "")).FontSize(kit.RowFont).SingleLine()
				subtitle := nativeProp(props, "subtitle", "")
				icon := nativeIcon(nativeProp(props, "icon", ""))
				if subtitle == "" && icon == nil {
					return
				}
				ui.Row(view).MinWidth(0).Gap(8).TextColor(palette.MutedForeground).Children(func() {
					if icon != nil {
						ui.Icon(view, icon).FontSize(16).Shrink(0)
					}
					nativeText(view, subtitle).FontSize(kit.RowFont).SingleLine().Shrink(1).MinWidth(0)
				})
			})
		}), nil
	case "PanelHeader":
		header := ui.Row(view).FillWidth().MinHeight(36).PaddingX(nativeProp(props, "padding", float32(12))).Gap(nativeProp(props, "gap", float32(8))).Children(children)
		if nativeProp(props, "bordered", false) {
			header.Border(1, palette.Border)
		}
		return header, nil
	case "EmptyState":
		// Kit's empty state, placed as the reference places its variants: a
		// panel centers it in the space it is given, a list keeps its own
		// height, a card frames it.
		variant := nativeProp(props, "variant", "panel")
		result := ui.Column(view).FillWidth().MinWidth(0).AlignItems(ui.Center).Justify(ui.Center)
		switch {
		case variant == "panel":
			// The reference's panel is size_full in whatever holds it.
			result.Fill().Grow(1).MinHeight(0)
		case nativeProp(props, "fill_height", false):
			result.Grow(1).HeightPercent(100).MinHeight(0)
		}
		if nativeProp(props, "vertical_align", "") == "start" {
			result.Justify(ui.Start)
		}
		if variant == "panel" {
			// The reference pads the panel 24 and stacks the package's own
			// children 16 under the state.
			result.Padding(24).Gap(16)
		}
		return result.Children(func() {
			state := kit.Result(view, kit.ResultProps{
				Title: nativeProp(props, "label", ""),
				Icon:  nativeIconOr(nativeProp(props, "icon", ""), icons.Inbox),
				Card:  variant == "card",
			})
			switch variant {
			case "panel":
				state.Padding(0)
			case "card":
				state.FillWidth()
			}
			children()
		}), nil
	case "Loading":
		return ui.Box(view).Fill().Center().Children(func() { kit.Loader(view, 20) }), nil
	case "InputSurface":
		return ui.Column(view).FillWidth().Gap(8).Radius(palette.RadiusLg).Background(palette.Muted).Children(children), nil
	case "StatusList":
		return binding.renderStatusList(view, props, children), nil
	case "ScrollRegion":
		height := nativeProp(props, "max_height", float32(520))
		if !finiteNumber(float64(height)) || height <= 0 {
			height = 520
		}
		return ui.Scroll(view).FillWidth().MaxHeight(height).MinWidth(0).MinHeight(0).Gap(12).Children(children), nil
	case "TextLine":
		return binding.renderTextLine(view, props)
	case "Tooltip":
		// The reference's trigger is a block filling its parent, so what
		// it holds is laid out as if the trigger were not there.
		result := ui.Column(view).Fill().MinWidth(0).MinHeight(0).Children(children)
		kit.Tooltip(view, result, nativeProp(props, "text", ""))
		return result, nil
	case "Modal":
		return binding.renderModal(view, element, identity, props)
	case "Details":
		return ui.Column(view).Gap(12).Children(func() {
			if subtitle := nativeProp(props, "subtitle", ""); subtitle != "" {
				nativeText(view, subtitle).TextColor(palette.MutedForeground)
			}
			kit.Markdown(view, nativeProp(props, "content", ""), kit.MarkdownProps{})
		}), nil
	case "Collapse", "CollapseSlot":
		return binding.renderCollapse(view, element, identity, props, children)
	case "Workspace":
		return ui.Column(view).Fill().MinWidth(0).MinHeight(0).Children(children), nil
	case "CardButton", "ComposedButton":
		return binding.renderActivityButton(view, element, identity, props, children), nil
	case "ClampedText":
		lines := nativeProp(props, "lines", 2)
		if lines < 1 || lines > 8 {
			lines = 2
		}
		color := palette.MutedForeground
		if nativeProp(props, "danger", false) {
			color = palette.Danger
		}
		return nativeText(view, nativeProp(props, "text", "")).MaxLines(lines).MaxHeight(float32(lines) * 20).TextColor(color).Clip(), nil
	case "HoverSwap":
		return binding.renderHoverSwap(view, element, children)
	case "ThinkingIcon":
		return kit.Indicator(view, kit.ThinkingOf(nativeProp(props, "phase", "")), 20), nil
	case "StatusRing":
		return binding.renderStatusRing(view, props), nil
	}
	return ui.Element{}, hostInvalid("unknown native layout")
}

// rowIconSize is a row's leading icon inside Kit's RowIcon slot.
const rowIconSize = 20

// rowHasIcon reports whether a group's child is a settings row whose icon
// name resolves.
func rowHasIcon(child *NativeElement) bool {
	if child.Module != "sailry/ui" || child.Export != "SettingsRow" {
		return false
	}
	_, props, err := nativeProps(*child)
	return err == nil && nativeIcon(nativeProp(props, "icon", "")) != nil
}

// renderSortable draws a group's rows as Kit's sortable list: each behind
// its drag handle, a divider between them. A drop that moved a row is
// reported as a control event of the group, with the rows' identifiers in
// their new order; the package answers with its children in that order.
func (binding *NativeBinding) renderSortable(view *ui.Context, element NativeElement, identity string, props map[string]json.RawMessage, children []*NativeElement) error {
	label := nativeProp(props, "sort_label", "")
	if strings.TrimSpace(label) == "" {
		return hostInvalid("sortable group requires a sort label")
	}
	rows := slices.Clone(children)
	names := make(map[*NativeElement]string, len(rows))
	for _, row := range rows {
		name, _, err := nativeProps(*row)
		if err != nil {
			return err
		}
		names[row] = name
	}
	var renderErr error
	kit.SortableList(view, &rows, func(row *NativeElement) string { return names[row] }, kit.SortableListProps{
		Divided:  true,
		Handle:   label,
		Disabled: nativeProp(props, "sort_disabled", false),
		OnMove: func(from, to int) {
			order := make([]string, len(rows))
			for index, row := range rows {
				order[index] = names[row]
			}
			binding.emit("control", map[string]any{"id": identity, "value": map[string]any{"from": from, "to": to, "order": order}})
		},
	}, func(row *NativeElement, handle func()) {
		handle()
		ui.Column(view).Grow(1).Basis(0).MinWidth(0).MarginStart(8).Children(func() {
			if renderErr == nil {
				renderErr = renderNativeChildren(view, element, []*NativeElement{row})
			}
		})
	})
	return renderErr
}

func (binding *NativeBinding) renderStatusList(view *ui.Context, props map[string]json.RawMessage, children func()) ui.Element {
	palette := theme.Of(view)
	rows := nativeProp(props, "items", []map[string]json.RawMessage{})
	return ui.Column(view).Fill().MinWidth(0).Children(func() {
		ui.Scroll(view).Grow(1).MinHeight(0).Gap(8).Children(func() {
			if body := nativeProp(props, "body", ""); body != "" && len(body) <= 32768 {
				nativeText(view, body)
			}
			if len(rows) == 0 {
				nativeText(view, nativeProp(props, "empty", "")).TextColor(palette.MutedForeground)
			}
			for _, row := range rows {
				text := nativeProp(row, "text", "")
				if len(text) > 32768 {
					continue
				}
				ui.Row(view.Key(nativeProp(row, "id", ""))).Gap(8).Children(func() {
					state := nativeProp(row, "state", "")
					if state == "in_progress" {
						kit.Busy(view)
					} else {
						icon := icons.CircleCheck
						if state == "completed" {
							icon = icons.Check
						}
						ui.Icon(view, icon).Size(14, 14).TextColor(palette.MutedForeground)
					}
					nativeText(view, text)
				})
			}
		})
		ui.Row(view).Justify(ui.End).Gap(8).Children(children)
	})
}

func (binding *NativeBinding) renderTextLine(view *ui.Context, props map[string]json.RawMessage) (ui.Element, error) {
	runs := nativeProp(props, "runs", []map[string]json.RawMessage{})
	lineHeight := nativeProp(props, "line_height", float32(1.2))
	if !finiteNumber(float64(lineHeight)) || lineHeight <= 0 {
		return ui.Element{}, hostInvalid("invalid text line height")
	}
	size := nativeProp(props, "reference_size", float32(0))
	total := 0
	for _, run := range runs {
		text := nativeProp(run, "text", "")
		font := nativeProp(run, "size", float32(14))
		weight := nativeProp(run, "weight", float32(400))
		gap := nativeProp(run, "gap", float32(0))
		total += len(text)
		if !finiteNumber(float64(font)) || font <= 0 || !finiteNumber(float64(weight)) || weight < 100 || weight > 900 || !finiteNumber(float64(gap)) || gap < 0 || total > 32768 {
			return ui.Element{}, hostInvalid("invalid text line run")
		}
		size = max(size, font)
	}
	return ui.Row(view).AlignItems(ui.End).Height(size * lineHeight).Children(func() {
		for _, run := range runs {
			font := nativeProp(run, "size", float32(14))
			text := nativeText(view, nativeProp(run, "text", "")).FontSize(font).FontWeight(int(nativeProp(run, "weight", float32(400)))).MarginStart(nativeProp(run, "gap", float32(0)))
			if color, err := nativeColor(nativeProp(run, "color", "")); err == nil {
				text.TextColor(color)
			}
		}
	}), nil
}

func (binding *NativeBinding) renderModal(view *ui.Context, element NativeElement, identity string, props map[string]json.RawMessage) (ui.Element, error) {
	source := nativeProp(props, "open", false)
	binding.state.mu.Lock()
	state := binding.state.modals[identity]
	if state == nil {
		state = &modalDraft{}
		binding.state.modals[identity] = state
	}
	if state.source != source {
		state.open, state.source = source, source
	}
	binding.state.mu.Unlock()
	if !state.open {
		return ui.Box(view).Size(0, 0), nil
	}
	dismissable := nativeProp(props, "dismissable", true)
	form := nativeProp(props, "form", false)
	close := func() {
		if state.open && dismissable {
			state.open = false
			binding.emit("modal", identity)
		}
	}
	width := nativeProp(props, "width", float32(560))
	if !finiteNumber(float64(width)) || width <= 0 {
		width = 560
	}
	windowWidth, windowHeight := view.Size()
	width = min(width, max(0, windowWidth-48))
	var dialog ui.Element
	var renderErr error
	palette := theme.Of(view)
	// The window holds its native menu's commands behind a modal dialog.
	kit.NoteModal(view)
	ui.Overlay(view, func() {
		backdrop := ui.Box(view).Absolute().Left(0).Top(0).Right(0).Bottom(0).Background(palette.Backdrop()).Modal().AlignItems(ui.Center)
		if form {
			backdrop.Justify(ui.Start).Padding(windowHeight/10, 0, 0, 0)
		} else {
			backdrop.Justify(ui.Center)
		}
		backdrop.OnClick(func() {
			if !form {
				close()
			}
		})
		if backdrop.OverlayShortcut(0, ui.KeyEscape) {
			close()
		}
		backdrop.Children(func() {
			panel := ui.Column(view).MinWidth(0).MinHeight(0)
			if form {
				panel.Width(width).MinHeight(96).MaxHeight(windowHeight*0.8).Gap(24).Padding(24, 0)
			} else {
				panel.Padding(40, 20, 20, 20)
			}
			dialog = kit.Floating(view, panel).OnClick(func() {})
			dialog.Children(func() {
				if form {
					ui.Column(view).Grow(1).MinHeight(0).Gap(8).Children(func() {
						nativeText(view, nativeProp(props, "title", "")).FontSize(16).FontWeight(600).Padding(0, 48, 0, 24).Shrink(0)
						if len(element.Children) > 0 {
							ui.Scroll(view).Grow(1).MinHeight(0).FillWidth().Padding(8, 24, 4, 24).Gap(16).Children(func() {
								renderErr = renderNativeChildren(view, element, element.Children[:1])
							})
						}
					})
					if len(element.Children) > 1 {
						// Kit's dialog footer ends its actions at the right.
						ui.Row(view).FillWidth().Shrink(0).PaddingX(24).Justify(ui.End).Children(func() {
							if renderErr == nil {
								renderErr = renderNativeChildren(view, element, element.Children[1:2])
							}
						})
					}
				} else {
					renderErr = renderNativeChildren(view, element, element.Children)
				}
				kit.IconButton(view, icons.Close, i18n.T(view, "close"), 24, false).Absolute().Top(14).Right(14).Disabled(!dismissable).OnClick(close)
			})
		})
	})
	return dialog, renderErr
}

func (binding *NativeBinding) renderCollapse(view *ui.Context, element NativeElement, identity string, props map[string]json.RawMessage, children func()) (ui.Element, error) {
	if element.Export == "CollapseSlot" {
		slot := ui.Column(view).MinWidth(0)
		if nativeProp(props, "variant", "") != "actions" {
			slot.FillWidth()
		}
		return slot.Children(children), nil
	}
	binding.state.mu.Lock()
	open := binding.state.collapses[identity]
	if open == nil {
		value := nativeProp(props, "default_open", false)
		open = &value
		binding.state.collapses[identity] = open
	}
	binding.state.mu.Unlock()
	headerCount, contentCount, actionsCount := 0, 0, 0
	slots := map[string]*NativeElement{}
	for _, child := range element.Children {
		if child.Module != "sailry/ui" || child.Export != "CollapseSlot" {
			return ui.Element{}, hostInvalid("collapse requires named slots")
		}
		_, slotProps, err := nativeProps(*child)
		if err != nil {
			return ui.Element{}, err
		}
		variant := nativeProp(slotProps, "variant", "")
		slots[variant] = child
		switch variant {
		case "header":
			headerCount++
		case "content":
			contentCount++
		case "actions":
			actionsCount++
		default:
			return ui.Element{}, hostInvalid("invalid collapse slot")
		}
	}
	if headerCount != 1 || contentCount != 1 || actionsCount > 1 {
		return ui.Element{}, hostInvalid("collapse requires one header and content slot")
	}
	var renderErr error
	draw := func(slot string) {
		if child := slots[slot]; child != nil && renderErr == nil {
			renderErr = renderNativeChildren(view, element, []*NativeElement{child})
		}
	}
	plain := nativeProp(props, "variant", "") == "plain"
	result := ui.Column(view).FillWidth().MinWidth(0).Children(func() {
		heading := ui.Row(view).FillWidth().MinWidth(0)
		heading.Children(func() {
			button := kit.Ghost(view, nativeProp(props, "label", ""), false).Grow(1).MinWidth(0).Justify(ui.Start).Height(36).Gap(6)
			if plain {
				button.Height(32).PaddingX(4)
			}
			button.Children(func() {
				icon := icons.ChevronRight
				if *open {
					icon = icons.ChevronDown
				}
				ui.Icon(view, icon).Size(14, 14)
				draw("header")
			}).OnClick(func() {
				*open = !*open
				if binding.Invalidate != nil {
					binding.Invalidate()
				}
			})
			if slots["actions"] != nil {
				actions := ui.Row(view).Shrink(0).Gap(8)
				actions.Children(func() { draw("actions") })
				if !heading.Hovered() && !actions.FocusWithin() {
					actions.Opacity(0)
				}
			}
		})
		if *open {
			draw("content")
		}
	})
	if !plain {
		result.Background(theme.Of(view).Muted).Border(1, theme.Of(view).Border).Radius(theme.Of(view).Radius).Clip()
	}
	return result, renderErr
}

func (binding *NativeBinding) renderHoverSwap(view *ui.Context, element NativeElement, children func()) (ui.Element, error) {
	_, props, err := nativeProps(element)
	if err != nil {
		return ui.Element{}, err
	}
	region := ui.Box(view).Width(112).Height(16).Shrink(0)
	if len(element.Children) == 0 {
		return region, nil
	}
	index := 0
	group := nativeProp(props, "group", "")
	if binding.state.hoverGroups[group] && len(element.Children) > 1 {
		index = 1
	}
	var renderErr error
	region.Children(func() {
		renderErr = renderNativeChildren(view, element, element.Children[index:index+1])
	})
	return region, renderErr
}
