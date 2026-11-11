package kit

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// Settings building blocks: a page frame, groups with a header over a card of
// rows, and rows of a title, a description and one control.

// PageWidth is a settings page's maximum width.
const PageWidth = 800

// Page is a settings page: a scrolling column at most 800 wide, padded 24,
// its title (18, semibold) and description (14, muted) over its groups 24
// apart. key keeps each page's scroll place.
func Page(c *ui.Context, key any, title, description string, body func()) ui.Element {
	p := theme.Of(c)
	return ui.Scroll(c.Key(key)).Fill().Children(func() {
		ui.Column(c).FillWidth().MaxWidth(PageWidth).Margin(0, ui.Auto).Padding(24).Gap(24).Children(func() {
			ui.Column(c).Gap(8).Children(func() {
				ui.Text(c, title).FontSize(18).FontWeight(600).TextColor(p.Foreground)
				if description != "" {
					ui.Text(c, description).FontSize(14).TextColor(p.MutedForeground)
				}
			})
			body()
		})
	})
}

// Group is a section of a page: a header of a 14 DIP semibold title, an
// optional muted description and trailing actions, over its body 12 below.
// actions may be nil.
func Group(c *ui.Context, key any, title, description string, actions, body func()) {
	p := theme.Of(c)
	ui.Column(c.Key(key)).Gap(12).Children(func() {
		if title != "" || actions != nil {
			ui.Row(c).MinHeight(32).Gap(8).Children(func() {
				ui.Column(c).Grow(1).Shrink(1).MinWidth(0).Gap(TitleGap).Children(func() {
					if title != "" {
						ui.Text(c, title).FontSize(14).FontWeight(600).TextColor(p.SurfaceTitle)
					}
					if description != "" {
						ui.Text(c, description).FontSize(14).TextColor(p.MutedForeground)
					}
				})
				if actions != nil {
					ui.Row(c).Shrink(0).Gap(8).Children(actions)
				}
			})
		}
		body()
	})
}

// Card is a group's box of rows (Kit's GroupBox): the group_box fill, a 1
// DIP border, the large radius, 16 across, rows apart by separators.
func Card(c *ui.Context, rows func()) ui.Element {
	p := theme.Of(c)
	return Frost(c, ui.Column(c).Border(1, p.Border).Radius(p.RadiusLg).PaddingX(16).Dividers(1, p.Border), p.Surface, CardTint).
		TextColor(p.SurfaceForeground).Children(rows)
}

// Row is a setting: a title and a muted description on the left, its
// control on the right, at the row metrics. control may be nil.
func Row(c *ui.Context, key any, title, description string, control func()) ui.Element {
	return LeadRow(c, key, nil, title, description, control)
}

// LeadRow is a Row with a leading icon slot: lead draws into a centered
// RowIcon square, RowIconGap before the text. A nil lead leaves no slot;
// one that draws nothing keeps the text aligned with its neighbours'.
func LeadRow(c *ui.Context, key any, lead func(), title, description string, control func()) ui.Element {
	return ui.Row(c.Key(key)).Padding(RowPad, 0).Gap(16).MinHeight(RowMinHeight).Children(func() {
		ui.Row(c).Grow(1).Shrink(1).MinWidth(0).Gap(RowIconGap).Children(func() {
			if lead != nil {
				ui.Row(c.Key("lead")).Size(RowIcon, RowIcon).Shrink(0).Center().Children(lead)
			}
			RowText(c, title, description).Grow(1).Shrink(1)
		})
		if control != nil {
			ui.Row(c).Shrink(0).Gap(8).Children(control)
		}
	})
}

// RowText is a list row's text at the row metrics: the title in the body
// size and regular weight over its description in the same size, muted,
// TitleGap below. description may be "".
func RowText(c *ui.Context, title, description string) ui.Element {
	p := theme.Of(c)
	return ui.Column(c).MinWidth(0).Gap(TitleGap).Children(func() {
		ui.Text(c, title).FontSize(RowFont).TextColor(p.SurfaceForeground)
		if description != "" {
			ui.Text(c, description).FontSize(RowFont).TextColor(p.Description())
		}
	})
}

// SwitchRow is a setting turned on and off.
func SwitchRow(c *ui.Context, key any, title, description string, on *bool) bool {
	changed := false
	Row(c, key, title, description, func() {
		changed = ui.Switch(c.Key("switch"), on).Label(title).Changed()
	})
	return changed
}

// SelectRow is a choice among options, shown as labels; *chosen is the
// option chosen.
func SelectRow(c *ui.Context, key any, title, description string, chosen *string, options []string) bool {
	changed := false
	Row(c, key, title, description, func() {
		var b ui.Element
		b, changed = Select(c.Key("select"), "setting-"+title, title, chosen, options, false)
		b.MinWidth(160)
	})
	return changed
}

// SegmentedRow is a choice among a few labels, side by side.
func SegmentedRow(c *ui.Context, key any, title, description string, chosen *int, labels ...string) bool {
	changed := false
	Row(c, key, title, description, func() {
		changed = Segmented(c, "segments", chosen, labels, SegmentedProps{}).Label(title).Changed()
	})
	return changed
}

// TextRow is a line of text; placeholder may be "".
func TextRow(c *ui.Context, key any, title, description, placeholder string, value *string) ui.Element {
	var input ui.Element
	Row(c, key, title, description, func() {
		input = ui.TextInput(c.Key("input"), value).Label(title).Placeholder(placeholder).Width(240)
	})
	return input
}

// SecretRow is a line of text hidden as it is typed, as an API key.
func SecretRow(c *ui.Context, key any, title, description, placeholder string, value *string) ui.Element {
	var input ui.Element
	Row(c, key, title, description, func() {
		input = ui.TextInput(c.Key("secret"), value).Password().Label(title).Placeholder(placeholder).Width(240)
	})
	return input
}

// NumberRow is a number between lo and hi.
func NumberRow(c *ui.Context, key any, title, description string, value *float64, lo, hi, step float64) bool {
	changed := false
	Row(c, key, title, description, func() {
		changed = ui.NumberInput(c.Key("number"), value, lo, hi, step).Label(title).Width(120).Changed()
	})
	return changed
}

// SliderRow is a number on a slider, its value shown after it by label.
func SliderRow(c *ui.Context, key any, title, description string, value *float64, lo, hi, step float64, label string) bool {
	p := theme.Of(c)
	changed := false
	Row(c, key, title, description, func() {
		changed = ui.StepSlider(c.Key("slider"), value, lo, hi, step).Label(title).Width(200).Changed()
		ui.Text(c, label).FontSize(14).Width(48).TextAlign(ui.End).TextColor(p.MutedForeground)
	})
	return changed
}

// ButtonRow is a setting acted on with a button: Secondary, or Danger for
// what removes something.
func ButtonRow(c *ui.Context, key any, title, description, action string, danger bool, onClick func()) {
	Row(c, key, title, description, func() {
		b := Secondary(c.Key("action"), action)
		if danger {
			b = Danger(c.Key("action"), action)
		}
		b.OnClick(onClick)
	})
}

// WideRow is a Row whose control takes 256 DIPs and wraps under the title
// when the page is narrow: selects, inputs and sliders.
func WideRow(c *ui.Context, key any, title, description string, control func()) ui.Element {
	return ui.Row(c.Key(key)).Padding(RowPad, 0).Gap(16).MinHeight(RowMinHeight).Wrap().Children(func() {
		RowText(c, title, description).Grow(1).Shrink(1).MinWidth(160)
		ui.Row(c).Width(256).MaxWidthPercent(100).Margin(0, 0, 0, ui.Auto).Gap(8).Justify(ui.End).Children(control)
	})
}

// Entry is a record in a group's card: its content, then an optional control
// and small ghost actions, at the row metrics. control and actions may be
// nil.
func Entry(c *ui.Context, key any, content, control, actions func()) ui.Element {
	return ui.Row(c.Key(key)).Padding(RowPad, 0).Gap(12).MinHeight(RowMinHeight).Children(func() {
		ui.Column(c).Grow(1).Shrink(1).MinWidth(0).Children(content)
		if control != nil {
			ui.Row(c).Shrink(0).Children(control)
		}
		if actions != nil {
			ui.Row(c).Shrink(0).Gap(8).Children(actions)
		}
	})
}

// Record is an entry's content: a medium title over a muted detail,
// TitleGap below, each on one line; detail may be "".
func Record(c *ui.Context, title, detail string) {
	p := theme.Of(c)
	ui.Column(c).MinWidth(0).Gap(TitleGap).Children(func() {
		ui.Text(c, title).FontSize(RowFont).FontWeight(500).SingleLine().TextColor(p.SurfaceForeground)
		if detail != "" {
			ui.Text(c, detail).FontSize(RowFont).SingleLine().TextColor(p.MutedForeground)
		}
	})
}

// Action is an entry's small ghost icon button, named by label for its
// tooltip and assistive technology.
func Action(c *ui.Context, icon *ui.SVG, label string, disabled bool) ui.Element {
	b := IconButton(c, icon, label, 28, false).Disabled(disabled)
	if disabled {
		b.Opacity(0.5)
	}
	return b
}

// TextAction is an entry's small ghost button with a label.
func TextAction(c *ui.Context, label string, disabled bool) ui.Element {
	b := GhostLook(c, ui.ButtonBase(c).Label(label).Disabled(disabled), false).Height(28).PaddingX(8).
		Children(func() { ui.Text(c, label).FontSize(14).SingleLine() })
	if disabled {
		b.Opacity(0.5)
	}
	return b
}

// The row metrics: one vertical rhythm for every list row with a title and
// a description, the application's settings rows and records and a
// package's settings and card rows alike.
const (
	// RowPad is the space above and below a row's content.
	RowPad = 12
	// RowMinHeight is the height of a row with a title alone.
	RowMinHeight = 56
	// RowFont is the size of a row's title and of its description.
	RowFont = 14
	// TitleGap is the space between a row's title and its description
	// (settings rows, records, media rows).
	TitleGap = 5
	// RowIcon is a row's leading icon slot; RowIconGap follows it.
	RowIcon    = 24
	RowIconGap = 12
)
