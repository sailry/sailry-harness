package plugins

import (
	"encoding/json"
	"reflect"
	"slices"
	"strconv"
	"strings"
	"unicode/utf8"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

type tableDraft struct {
	list      ui.ListState
	selected  int
	selection ui.Selection[int]
	revision  string
	page      int
	rows      [][]string
	rowIDs    []string
}

type nativeTableLine struct {
	Text      string `json:"text"`
	Icon      string `json:"icon"`
	Tone      string `json:"tone"`
	TextTone  string `json:"text_tone"`
	Badge     string `json:"badge"`
	BadgeIcon string `json:"badge_icon"`
	Note      string `json:"note"`
	Chip      string `json:"chip"`
	Meter     *struct {
		Value float64  `json:"value"`
		Label string   `json:"label"`
		Tone  string   `json:"tone"`
		Icon  string   `json:"icon"`
		Width *float32 `json:"width"`
		Fill  bool     `json:"fill"`
	} `json:"meter"`
}

type nativeTableCell struct {
	Primary   nativeTableLine  `json:"primary"`
	Secondary *nativeTableLine `json:"secondary"`
}

func (binding *NativeBinding) renderTable(view *ui.Context, element NativeElement, _ func(), _ func(CallbackRef, json.RawMessage)) (ui.Element, error) {
	identity, props, err := nativeProps(element)
	if err != nil {
		return ui.Element{}, err
	}
	columns := nativeProp(props, "columns", []string{})
	rows := nativeProp(props, "rows", [][]string{})
	cells := nativeProp(props, "cells", [][]nativeTableCell{})
	revision := nativeProp(props, "revision", "")
	page := nativeProp(props, "page", 0)
	if page < 0 {
		return ui.Element{}, hostInvalid("invalid plugin table bounds")
	}
	for _, row := range rows {
		if len(row) != len(columns) {
			return ui.Element{}, hostInvalid("plugin table row width differs from its columns")
		}
	}
	widths := nativeProp(props, "widths", []float32{})
	alignments := nativeProp(props, "alignments", []string{})
	details := nativeProp(props, "details", [][]string{})
	rowIDs := nativeProp(props, "row_ids", []string{})
	fill := nativeProp[*int](props, "fill", nil)
	if len(cells) != 0 && len(cells) != len(rows) || len(details) != 0 && len(details) != len(rows) {
		return ui.Element{}, hostInvalid("plugin table cells differ from its rows")
	}
	for _, row := range cells {
		if len(row) != len(columns) {
			return ui.Element{}, hostInvalid("plugin table cell width differs from its columns")
		}
		for _, cell := range row {
			if !validTableLine(cell.Primary) || cell.Secondary != nil && !validTableLine(*cell.Secondary) {
				return ui.Element{}, hostInvalid("invalid plugin table cell")
			}
		}
	}
	for _, row := range details {
		if len(row) != len(columns) {
			return ui.Element{}, hostInvalid("plugin table details differ from its columns")
		}
	}
	for _, width := range widths {
		if !finiteNumber(float64(width)) || width < 16 || width > 4096 {
			return ui.Element{}, hostInvalid("invalid plugin table column width")
		}
	}
	for _, alignment := range alignments {
		if alignment != "start" && alignment != "end" {
			return ui.Element{}, hostInvalid("invalid plugin table alignment")
		}
	}
	for _, bound := range []struct {
		name    string
		minimum float32
	}{{"row_height", 32}, {"header_height", 24}} {
		if height := nativeProp[*float32](props, bound.name, nil); height != nil && (!finiteNumber(float64(*height)) || *height < bound.minimum || *height > 96) {
			return ui.Element{}, hostInvalid("invalid plugin table height")
		}
	}
	if fill != nil && (*fill < 0 || *fill >= len(columns)) {
		return ui.Element{}, hostInvalid("invalid plugin table fill column")
	}
	if len(rowIDs) != 0 {
		if len(rowIDs) != len(rows) {
			return ui.Element{}, hostInvalid("plugin table identifiers differ from its rows")
		}
		unique := map[string]struct{}{}
		for _, identity := range rowIDs {
			if _, exists := unique[identity]; exists {
				return ui.Element{}, hostInvalid("plugin table identifiers must be unique")
			}
			unique[identity] = struct{}{}
		}
	}
	first := len(rows)
	if page <= len(rows)/50 {
		first = page * 50
	}
	count := min(50, len(rows)-first)
	binding.state.mu.Lock()
	state := binding.state.tables[identity]
	if state == nil {
		state = &tableDraft{selected: -1}
		binding.state.tables[identity] = state
	}
	if state.revision != revision || state.page != page || !reflect.DeepEqual(state.rows, rows) {
		retained := map[string]struct{}{}
		for index := range state.selection.All() {
			absolute := index + state.page*50
			if absolute >= 0 && absolute < len(state.rowIDs) {
				retained[state.rowIDs[absolute]] = struct{}{}
			}
		}
		state.selected = -1
		state.selection.Clear()
		for index := 0; index < count; index++ {
			if first+index >= len(rowIDs) {
				break
			}
			if _, exists := retained[rowIDs[first+index]]; exists {
				state.selection.Add(index)
				if state.selected < 0 {
					state.selected = index
				}
			}
		}
		state.list = ui.ListState{Columns: state.list.Columns}
		state.revision, state.page = revision, page
	}
	state.rows = rows
	state.rowIDs = rowIDs
	binding.state.mu.Unlock()
	state.list.Selected = &state.selected
	state.list.Selection = &state.selection
	definitions := make([]ui.TableColumn, len(columns))
	for index, label := range columns {
		column := ui.TableColumn{Title: label, ID: strconv.Itoa(index), Width: 180, MinWidth: 16, MaxWidth: 4096, Fixed: !nativeProp(props, "resizable", true)}
		if index < len(widths) && finiteNumber(float64(widths[index])) && widths[index] > 0 {
			column.Width = widths[index]
		}
		if index < len(alignments) && alignments[index] == "end" {
			column.Align = ui.End
		}
		if fill != nil && *fill == index {
			column.MinWidth = column.Width
			column.Width = 0
		}
		definitions[index] = column
	}
	emit := func(kind string, row, column *int, action string) {
		selected := []int{}
		for index := range state.selection.All() {
			selected = append(selected, index+first)
		}
		slices.Sort(selected)
		event := map[string]any{"table": identity, "revision": revision, "kind": kind, "row": row, "column": column, "rows": selected}
		if action != "" {
			event["action"] = action
		}
		binding.emit("table", event)
	}
	state.list.CellMenu = func(menu *ui.Menu, row, column int) {
		for _, item := range nativeProp(props, "menu", []nativeMenuItem{}) {
			if item.Separator {
				menu.Separator()
				continue
			}
			if menu.Item(item.Label).Disabled(item.Disabled || item.Enabled != nil && !*item.Enabled).Chosen() {
				absolute := row + first
				emit("action", &absolute, &column, item.ID)
			}
		}
	}
	// The frame, the header's fill and the rows' separators are Kit's table
	// look (kit.TableFrame, kit.TableCell), shared with the host's processes.
	rowHeight := nativeProp(props, "row_height", float32(32))
	header := nativeProp(props, "header", true)
	stripe := nativeProp(props, "stripe", false)
	headHeight := theme.Neutral(view).Space(8)
	cellPadding := theme.Neutral(view).Space(1.5)
	stripeColor := theme.Of(view).Muted
	state.list.HeaderCell = func(cell ui.Element, _ int) { cell.Hidden(!header) }
	result := ui.Column(view).Fill().MinWidth(0)
	if len(columns) > 0 {
		kit.TableFrame(view, result)
		if !header {
			result.Draw(nil)
		}
	}
	_, pointerY, over := result.PointerPosition()
	pointerY += result.Bounds().Y
	result.Children(func() {
		// MyGo fills a chosen row with its theme's accent, the app's
		// emphasis color: the table is built under the neutral theme, so a
		// chosen row takes the neutral selected fill, as the file picker's
		// and the host's processes do.
		var table ui.Element
		if len(columns) > 0 && (count > 0 || header) {
			area := ui.Themed(view.Key("neutral"), theme.Neutral(view)).Background(ui.Transparent).FillWidth().MinHeight(0)
			if count > 0 {
				area.Grow(1)
			} else {
				area.Height(headHeight + 1).Shrink(0)
			}
			if !header {
				// The pinned MyGo table always builds a header and divider.
				// Hide its header cells from display and accessibility, and clip
				// that fixed-height strip above the frame; row interaction and
				// column layout still belong to ui.Table.
				area.Margin(-headHeight-1, 0, 0, 0)
			}
			area.Children(func() {
				table = ui.Table(view, &state.list, definitions, count, func(row, column int) {
					absolute := first + row
					// A row is row_height high with the cell's padding, and
					// its content is centered in it.
					cell := kit.TableCell(view, rowHeight, row+1 == count, func(bounds ui.Rect) ui.Color {
						hovered := over && pointerY >= bounds.Y-cellPadding && pointerY < bounds.Y+bounds.H+cellPadding
						if stripe && absolute%2 == 1 && !state.selection.Has(row) && !hovered {
							return stripeColor
						}
						return ui.Transparent
					}).Gap(3)
					if index := column; index < len(alignments) && alignments[index] == "end" {
						cell.AlignItems(ui.End)
					}
					if absolute < len(details) {
						cell.Tooltip(details[absolute][column])
					} else {
						cell.Tooltip(rows[absolute][column])
					}
					cell.Children(func() {
						if absolute < len(cells) && column < len(cells[absolute]) {
							binding.renderTableLine(view, cells[absolute][column].Primary, false)
							if secondary := cells[absolute][column].Secondary; secondary != nil {
								binding.renderTableLine(view, *secondary, true)
							}
						} else {
							nativeText(view, rows[absolute][column]).SingleLine()
						}
					})
				}).Grow(1).FillWidth()
			})
		}
		if count == 0 {
			ui.Column(view.Key("empty")).Grow(1).FillWidth().MinHeight(0).AlignItems(ui.Center).Justify(ui.Center).Children(func() {
				kit.Result(view, kit.ResultProps{Title: nativeProp(props, "empty", ""), Icon: nativeIcon(nativeProp(props, "empty_icon", ""))})
			})
			return
		}
		table.OnChange(func() {
			var row *int
			if state.selected >= 0 {
				absolute := state.selected + first
				row = &absolute
			}
			emit("selection", row, nil, "")
		})
		if table.Shortcut(ui.Cmd, ui.KeyC) {
			selected := []int{}
			for index := range state.selection.All() {
				selected = append(selected, index+first)
			}
			slices.Sort(selected)
			lines := make([]string, 0, len(selected))
			for _, row := range selected {
				if row < len(rows) {
					lines = append(lines, strings.Join(rows[row], "\t"))
				}
			}
			view.Services().WriteClipboard(strings.Join(lines, "\n"))
			var firstSelected *int
			if len(selected) > 0 {
				firstSelected = &selected[0]
			}
			emit("copy", firstSelected, nil, "")
		}
	})
	return result, nil
}

func validTableLine(line nativeTableLine) bool {
	if utf8.RuneCountInString(line.Badge) > 2 {
		return false
	}
	if meter := line.Meter; meter != nil && (!finiteNumber(meter.Value) || meter.Value < 0 || meter.Value > 100 || meter.Width != nil && (!finiteNumber(float64(*meter.Width)) || *meter.Width < 16 || *meter.Width > 240)) {
		return false
	}
	return true
}

func (binding *NativeBinding) renderTableLine(view *ui.Context, line nativeTableLine, secondary bool) {
	palette := theme.Of(view)
	ui.Row(view).Gap(6).MinWidth(0).AlignItems(ui.Center).Children(func() {
		if line.Badge != "" || line.BadgeIcon != "" {
			ui.Box(view).Size(18, 18).Radius(5).Background(palette.Muted).AlignItems(ui.Center).Justify(ui.Center).Children(func() {
				if icon := nativeIcon(line.BadgeIcon); icon != nil {
					ui.Icon(view, icon).Size(12, 12)
				} else {
					nativeText(view, line.Badge).FontSize(12).TextColor(palette.MutedForeground)
				}
			})
		}
		color := palette.Foreground
		if secondary {
			color = palette.MutedForeground
		}
		if icon := nativeIcon(line.Icon); icon != nil {
			tint := color
			if line.Tone != "" {
				tint = nativeTone(view, line.Tone)
			}
			ui.Icon(view, icon).Size(12, 12).TextColor(tint)
		}
		if line.TextTone != "" {
			color = nativeTone(view, line.TextTone)
		}
		nativeText(view, line.Text).SingleLine().TextColor(color)
		if line.Note != "" {
			nativeText(view, line.Note).TextColor(palette.MutedForeground).SingleLine()
		}
		if line.Chip != "" {
			kit.Badge(view, line.Chip, kit.Neutral)
		}
		if meter := line.Meter; meter != nil {
			width := float32(36)
			if meter.Width != nil {
				width = *meter.Width
			}
			width = min(max(width, 16), 240)
			progress := ui.Progress(view, min(max(meter.Value, 0), 100)/100).Label(meter.Label).Width(width).TextColor(nativeTone(view, meter.Tone))
			if meter.Fill {
				progress.Grow(1)
			}
			if icon := nativeIcon(meter.Icon); icon != nil {
				ui.Icon(view, icon).Size(12, 12).TextColor(nativeTone(view, meter.Tone))
			}
		}
	})
}

func nativeTone(view *ui.Context, name string) ui.Color {
	palette := theme.Of(view)
	switch name {
	case "primary":
		return palette.Primary
	case "muted":
		return palette.MutedForeground
	case "warning":
		return palette.Warning
	case "success":
		return palette.Success
	case "danger":
		return palette.Danger
	case "info":
		return palette.Info
	case "chart_1", "chart_2", "chart_3", "chart_4", "chart_5", "chart_6", "chart_7", "chart_8":
		return nativeChartColor(view, name)
	}
	return palette.Foreground
}

type pickerDraft struct {
	open     bool
	source   bool
	query    string
	selected int
}

func (binding *NativeBinding) renderPicker(view *ui.Context, element NativeElement, _ func(), _ func(CallbackRef, json.RawMessage)) (ui.Element, error) {
	identity, props, err := nativeProps(element)
	if err != nil {
		return ui.Element{}, err
	}
	items := nativeProp(props, "items", []map[string]json.RawMessage{})
	if len(items) > 4096 {
		return ui.Element{}, hostInvalid("picker item limit exceeded")
	}
	binding.state.mu.Lock()
	state := binding.state.pickers[identity]
	if state == nil {
		state = &pickerDraft{}
		binding.state.pickers[identity] = state
	}
	source := nativeProp(props, "open", false)
	if source != state.source {
		state.open, state.source = source, source
	}
	binding.state.mu.Unlock()
	commands := []kit.CommandItem{}
	values := []map[string]json.RawMessage{}
	for _, item := range items {
		label := nativeProp(item, "label", "")
		if !strings.Contains(strings.ToLower(label+" "+nativeProp(item, "detail", "")), strings.ToLower(state.query)) {
			continue
		}
		commands = append(commands, kit.CommandItem{ID: nativeProp(item, "id", ""), Label: label, Detail: nativeProp(item, "detail", ""), Icon: nativeIcon(nativeProp(item, "icon", "")), Group: nativeProp(item, "group", ""), Folder: nativeProp(item, "submenu", false), Checked: nativeProp(item, "checked", false), Disabled: nativeProp(item, "disabled", false)})
		values = append(values, item)
	}
	was := state.open
	kit.Command(view, identity, &state.open, &state.query, &state.selected, commands, kit.CommandProps{Placeholder: nativeProp(props, "title", ""), Empty: nativeProp(props, "empty", "")}, func(index int) {
		if index < 0 || index >= len(values) {
			return
		}
		item := values[index]
		binding.emit("picker", map[string]any{"picker": identity, "kind": "select", "id": nativeProp(item, "id", ""), "value": optionalRaw(item, "value")})
	})
	if was && !state.open {
		binding.emit("picker", map[string]any{"picker": identity, "kind": "close"})
	}
	return ui.Box(view).Size(0, 0), nil
}
