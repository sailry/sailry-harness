package kit

import (
	"fmt"
	"slices"
	"strings"
	"unicode"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// ModelSelect is the one control that chooses a model outside the composer:
// in the settings pages and in the forms and views of packages. It is a
// select's trigger over a popover of its own, since a Menu has neither
// groups with a mark, a query, nor a second column: the models under their
// providers, and beside them the efforts of the model the keys are on, of
// the chosen one until then. The rows are the composer picker's: 32 high,
// the chosen one told by the neutral Palette.Selected alone, a score and
// nothing else at a row's end (AGENTS.md, Removed UI).

// ModelOption is a model a ModelSelect offers.
type ModelOption struct {
	// ID is what the call site stores; Name is what the user reads.
	ID, Name string
	// Provider heads the model's group; models without one come first,
	// under no heading. Mark is the provider's mark (icons.Brand), and
	// without one the provider's initial stands for it.
	Provider string
	Mark     *ui.SVG
	// Efforts are the levels the model takes, none for a model without
	// reasoning; DefaultEffort is the one it starts at.
	Efforts       []string
	DefaultEffort string
	// Score is the model's measured fit, 0 when unmeasured.
	Score float64
}

// ModelSelectProps is what a ModelSelect shows and reports.
type ModelSelectProps struct {
	// Label names the control for assistive technology; Placeholder shows,
	// muted, while no model is chosen and there is no None.
	Label, Placeholder string
	// None labels the choice of no model (Model ""), the list's first row,
	// as a role inheriting the conversation's model; "" offers none.
	None   string
	Models []ModelOption
	// Model and Effort are the chosen ones. AutoEffort labels the effort
	// "" as the first of a model's efforts; "" offers none.
	Model, Effort string
	AutoEffort    string
	// Stored names a chosen model that is not among Models, as one whose
	// provider was removed: the trigger shows it muted, with a warning.
	// Without it the model's ID after its provider's stands for it.
	Stored string
	// Unavailable says why the chosen model cannot run: the trigger then
	// shows it as it shows one that is not among Models, and tells why in
	// its tooltip and its description.
	Unavailable string
	// ReadOnly shows the choice without opening.
	Disabled, ReadOnly bool
	// OnChange reports another model or effort chosen. A model is chosen at
	// the effort chosen now where it has that one, else at Auto where the
	// control offers it, else at the model's default.
	OnChange func(model, effort string)
}

const (
	// modelPanelWidth is the least width of the popover without efforts,
	// its trigger's where that is wider; modelEffortWidth is the efforts'
	// column, a hairline in a gap of modelColumnGap on each side before it.
	modelPanelWidth  = 320
	modelEffortWidth = 132
	modelColumnGap   = 6
	// modelListHeight is ten rows: longer lists scroll.
	modelListHeight = 340
	// modelSearchAt is how many models a list has before it gets a query.
	modelSearchAt  = 8
	modelRowHeight = 32
)

// modelSelectState is what a ModelSelect keeps while it shows.
type modelSelectState struct {
	open  bool
	query string
	// cursor is the model whose efforts show: the one the keys were on
	// last or that was chosen in this popover, "" for the chosen one.
	cursor string
	// column tells that the efforts show, which widens the popover.
	column bool
	// enter gives the chosen row the focus as the popover opens, and
	// efforts gives it to the effort a model was just chosen at.
	enter, efforts bool
}

type modelSelectKey struct{}

// ModelSelect draws the control in c and returns its trigger, for the call
// site to size as it sizes a select.
func ModelSelect(c *ui.Context, o ModelSelectProps) ui.Element {
	p := theme.Of(c)
	b := ui.ButtonBase(c)
	st := ui.Local(b, modelSelectKey{}, func() modelSelectState { return modelSelectState{} })
	chosen, listed := o.find(o.Model)
	name, effort := chosen.Name, ""
	switch {
	case o.Model == "":
		name = o.None
	case !listed:
		if name = o.Stored; name == "" {
			name = o.Model[strings.LastIndex(o.Model, "/")+1:]
		}
	case slices.Contains(chosen.Efforts, o.Effort):
		effort = o.Effort
	}
	value := name
	if effort != "" {
		value += " · " + effort
	}
	warned := o.Model != "" && (!listed || o.Unavailable != "")
	selectFrame(c, b.Label(o.Label).Value(value).Disabled(o.Disabled).HasPopup(ui.PopupListBox).Expanded(st.open))
	if o.ReadOnly {
		b.Cursor(ui.CursorDefault)
	}
	if warned {
		Tooltip(c, b, o.Unavailable)
	}
	b.Children(func() {
		ui.Row(c).Grow(1).Shrink(1).MinWidth(0).Gap(8).AlignItems(ui.Center).Children(func() {
			text := func(s string) ui.Element {
				return ui.Text(c, s).FontSize(14).SingleLine().Shrink(1).MinWidth(0)
			}
			switch {
			case name == "":
				text(o.Placeholder).TextColor(p.MutedForeground)
			case o.Model == "":
				text(name)
			case warned:
				ui.Icon(c, icons.TriangleAlert).FontSize(14).Shrink(0).TextColor(p.Warning)
				text(name).TextColor(p.MutedForeground)
			default:
				modelMark(c, chosen, 16, p.Foreground)
				text(name)
				if effort != "" {
					ui.Text(c, effort).FontSize(14).SingleLine().Shrink(0).TextColor(p.MutedForeground)
				}
			}
		})
		if !o.ReadOnly {
			selectChevron(c, st.open)
		}
	}).OnClick(func() {
		if o.ReadOnly {
			return
		}
		*st = modelSelectState{open: !st.open, enter: true}
	})
	panel := Popover(c, b, &st.open, func() { o.panel(c, st, chosen, listed) })
	if panel != (ui.Element{}) {
		// The models keep their width as the efforts come and go beside
		// them: the popover grows by the column, unless the trigger is wide
		// enough to hold both, where the popover stays as wide as it.
		width := max(modelPanelWidth, b.Bounds().W)
		if column := float32(modelEffortWidth + 2*modelColumnGap + 1); st.column && width < modelPanelWidth+column {
			width += column
		}
		window, _ := c.Size()
		Snapped(panel.Padding(6).Width(min(width, window-16)))
	}
	return b
}

// ModelsOffered are the models named ids, in their order, as a field that
// stores a model alone offers them: each as known has it without its
// efforts, and one known has not under the model part of its id.
func ModelsOffered(known []ModelOption, ids []string) []ModelOption {
	offered := make([]ModelOption, 0, len(ids))
	for _, id := range ids {
		model := ModelOption{ID: id, Name: id[strings.LastIndex(id, "/")+1:]}
		if at := slices.IndexFunc(known, func(m ModelOption) bool { return m.ID == id }); at >= 0 {
			model = known[at]
			model.Efforts, model.DefaultEffort = nil, ""
		}
		offered = append(offered, model)
	}
	return offered
}

// find returns the offered model named id.
func (o ModelSelectProps) find(id string) (ModelOption, bool) {
	for _, m := range o.Models {
		if m.ID == id && id != "" {
			return m, true
		}
	}
	return ModelOption{}, false
}

// carried is the effort a model is chosen at from its row, the one its
// column fills: the chosen model's own, else the effort chosen now where
// the model has it, else Auto where the control offers it, else the
// model's default, or its middle one.
func (o ModelSelectProps) carried(m ModelOption) string {
	switch {
	case len(m.Efforts) == 0:
		return ""
	case m.ID == o.Model && (slices.Contains(m.Efforts, o.Effort) || o.Effort == "" && o.AutoEffort != ""):
		return o.Effort
	case m.ID != o.Model && slices.Contains(m.Efforts, o.Effort):
		return o.Effort
	case o.AutoEffort != "":
		return ""
	case slices.Contains(m.Efforts, m.DefaultEffort):
		return m.DefaultEffort
	}
	return m.Efforts[len(m.Efforts)/2]
}

// modelMark is a provider's mark size across in color: its brand mark, or
// its name's initial in a tile where it has none. A model without a
// provider has no mark.
func modelMark(c *ui.Context, m ModelOption, size float32, color ui.Color) {
	if m.Mark != nil {
		ui.Icon(c, m.Mark).FontSize(size).Shrink(0).TextColor(color)
		return
	}
	initial := []rune(strings.TrimSpace(m.Provider))
	if len(initial) == 0 {
		return
	}
	ui.Row(c).Size(size, size).Shrink(0).Radius(4).Center().Background(theme.Of(c).Secondary).Children(func() {
		ui.Text(c, string(unicode.ToUpper(initial[0]))).FontSize(size * 0.62).FontWeight(600).SingleLine().TextColor(color)
	})
}

// modelRow is a row of the popover, a model's or an effort's, as the
// composer picker's: 32 high, the chosen one filled with the neutral
// Palette.Selected and selected for assistive technology, with no check.
func modelRow(c *ui.Context, key any, label string, selected bool, content func()) ui.Element {
	p := theme.Of(c)
	b := ui.ButtonBase(c.Key(key)).Label(label).Role(ui.RoleOption).AccessSelected(selected).FillWidth().MinWidth(0).
		Height(modelRowHeight).Shrink(0).PaddingX(8).Gap(8).Radius(p.RadiusLg).Justify(ui.Start).AlignItems(ui.Center).
		Cursor(ui.CursorPointer).TextColor(p.PopoverForeground)
	switch {
	case selected || b.Pressed():
		b.Background(p.Selected())
	case b.Hovered():
		b.Background(p.Hover)
	}
	return b.Children(func() {
		ui.Text(c, label).FontSize(14).LineHeight(20.0 / 14).SingleLine().Grow(1).Shrink(1).MinWidth(0)
		if content != nil {
			content()
		}
	})
}

// panel is the popover's content: the query over a list of many models,
// then the models and, a hairline apart, the efforts of the one the keys
// are on or that is chosen, where it has any. Up and Down move in a
// column, Right or Tab from the models to the efforts, Left back; Enter
// chooses, in the query the first match.
func (o ModelSelectProps) panel(c *ui.Context, st *modelSelectState, chosen ModelOption, listed bool) {
	p := theme.Of(c)
	change := func(model, effort string) {
		if o.OnChange != nil && (model != o.Model || effort != o.Effort) {
			o.OnChange(model, effort)
		}
	}
	// A model's row chooses it and closes, or leaves its efforts to choose.
	choose := func(m ModelOption) {
		if m.ID != o.Model {
			change(m.ID, o.carried(m))
		}
		st.cursor = m.ID
		if st.efforts = len(m.Efforts) > 0; !st.efforts {
			st.open = false
		}
	}
	query := strings.ToLower(strings.TrimSpace(st.query))
	var shown []ModelOption
	var providers []string
	for _, m := range o.Models {
		if query != "" && !strings.Contains(strings.ToLower(m.Name+" "+m.Provider), query) {
			continue
		}
		shown = append(shown, m)
		if !slices.Contains(providers, m.Provider) {
			providers = append(providers, m.Provider)
		}
	}
	// The models without a provider come first, under no heading.
	slices.SortStableFunc(providers, func(a, b string) int {
		if (a == "") != (b == "") && a == "" {
			return -1
		} else if (a == "") != (b == "") {
			return 1
		}
		return 0
	})
	ui.Column(c.Key("model-select")).FillWidth().Gap(6).Children(func() {
		var field ui.Element
		searched := len(o.Models) > modelSearchAt
		if searched {
			label := i18n.T(c, "search")
			field = ui.SearchField(c.Key("query"), &st.query).Label(label).Placeholder(label).FillWidth().AutoFocus()
			if field.Submitted() && len(shown) > 0 {
				choose(shown[0])
			}
		}
		ui.Row(c.Key("columns")).FillWidth().Gap(modelColumnGap).AlignItems(ui.Stretch).Children(func() {
			rows := map[string]ui.Element{}
			var first ui.Element
			list := ui.Scroll(c.Key("models")).Grow(1).Basis(0).MinWidth(0).MaxHeight(modelListHeight).Gap(2).
				Role(ui.RoleListBox).Label(o.Label).FocusGroup(ui.Vertical)
			list.Children(func() {
				if o.None != "" && query == "" {
					first = modelRow(c, "none", o.None, o.Model == "", nil).OnClick(func() {
						change("", "")
						st.open = false
					})
					rows[""] = first
				}
				if len(shown) == 0 && first == (ui.Element{}) {
					ui.Text(c, i18n.T(c, "composer_model_empty")).FontSize(14).LineHeight(20.0/14).Padding(6, 8).TextColor(p.MutedForeground)
				}
				for i, provider := range providers {
					if provider != "" {
						top := float32(8)
						if i == 0 && first == (ui.Element{}) {
							top = 4
						}
						ui.Row(c.Key("provider-"+provider)).FillWidth().Shrink(0).Padding(top, 8, 4, 8).Gap(6).AlignItems(ui.Center).
							TextColor(p.MutedForeground).Children(func() {
							for _, m := range shown {
								if m.Provider == provider {
									modelMark(c, m, 14, p.MutedForeground)
									break
								}
							}
							ui.Text(c, provider).FontSize(12).FontWeight(500).SingleLine().Shrink(1).MinWidth(0)
						})
					}
					for _, m := range shown {
						if m.Provider != provider {
							continue
						}
						row := modelRow(c, "model-"+m.ID, m.Name, m.ID == o.Model, func() {
							if m.Score > 0 {
								ui.Text(c, fmt.Sprintf("%.1f", m.Score)).Font("monospace").FontSize(14).SingleLine().Shrink(0).
									TextColor(p.MutedForeground)
							}
						}).OnClick(func() { choose(m) })
						rows[m.ID] = row
						if first == (ui.Element{}) {
							first = row
						}
					}
				}
			})
			// The popover opens with the focus in the query, or on the
			// chosen row, which a long list brings into view.
			if row, ok := rows[o.Model]; st.enter && ok {
				row.ScrollIntoView()
				if !searched {
					row.Focus()
				}
			} else if st.enter && !searched && first != (ui.Element{}) {
				first.Focus()
			}
			st.enter = false
			if searched && field.Shortcut(0, ui.KeyDown) && first != (ui.Element{}) {
				first.Focus()
			}
			// The efforts are of the model the keys are on, else of the one
			// chosen in this popover, else of the chosen one.
			for id, row := range rows {
				if row.Focused() {
					st.cursor = id
				}
			}
			// A model the query leaves out shows none.
			_, visible := rows[chosen.ID]
			column, has := chosen, listed && visible
			for _, m := range shown {
				if m.ID == st.cursor {
					column, has = m, true
				}
			}
			if st.cursor == "" && rows[""].Focused() {
				has = false
			}
			st.column = has && len(column.Efforts) > 0
			if !st.column {
				st.efforts = false
				return
			}
			row := rows[column.ID]
			if column.ID != o.Model && !row.Pressed() {
				// The column's model stays marked while the pointer or the
				// focus is in the column.
				row.Background(p.Hover)
			}
			right := list.Shortcut(0, ui.KeyRight) || list.Shortcut(0, ui.KeyTab)
			ui.Box(c.Key("divider")).Width(1).Shrink(0).Background(p.Border)
			current := o.carried(column)
			levels := column.Efforts
			if o.AutoEffort != "" {
				levels = append([]string{""}, levels...)
			}
			efforts := ui.Scroll(c.Key("efforts")).Width(modelEffortWidth).Shrink(0).MaxHeight(modelListHeight).Gap(2).
				Role(ui.RoleListBox).Label(i18n.T(c, "composer_effort")).FocusGroup(ui.Vertical)
			efforts.Children(func() {
				for i, level := range levels {
					label := level
					if level == "" {
						label = o.AutoEffort
					}
					selected := level == current
					option := modelRow(c, "effort-"+level, label, selected, nil).OnClick(func() {
						change(column.ID, level)
						st.open = false
					})
					if (right || st.efforts) && (selected || i == 0 && !slices.Contains(levels, current)) {
						option.Focus()
					}
				}
			})
			st.efforts = false
			if efforts.Shortcut(0, ui.KeyLeft) {
				row.Focus()
			}
		})
	})
}
