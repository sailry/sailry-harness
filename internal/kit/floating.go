package kit

import (
	"github.com/egoist/mygo/plugins/glass"
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-go/internal/i18n"
	"github.com/sailry/sailry-go/internal/icons"
	"github.com/sailry/sailry-go/internal/theme"
)

// Floating layers (menus drawn in the window, popovers, dialogs, tooltips,
// hover cards, select popups, the search overlay and toasts) are frosted:
// what is under them shows through blurred, under a tint of the popover
// color. This is a deliberate upgrade over the reference, whose floating
// surfaces are the popover color at 95% (theme/floating.rs): GPUI has no
// backdrop blur. Right-click menus stay the system's.
//
// The material is chosen here alone: glass.Glass{} in place of the blur is
// a one-line change in Frosted.

// floatBlur is the backdrop blur of floating layers, in DIPs.
const floatBlur = 24

// frosted is a backdrop blur under a tint, one material.
type frosted struct {
	blur ui.Material
	tint ui.Color
}

func (f frosted) PaintMaterial(p *ui.Painter, box ui.Rect, radii [4]float32) {
	if f.blur != nil {
		f.blur.PaintMaterial(p, box, radii)
	}
	p.Fill(box, f.tint, radii[0])
}

// Frosted is the material of floating layers: the backdrop blurred under
// the popover color at theme.FloatingAlpha. With Reduce Transparency it is
// the popover color, opaque, and nothing shows through.
func Frosted(c *ui.Context) ui.Material {
	p := theme.Of(c)
	if c.Preferences().ReduceTransparency {
		return frosted{tint: p.Popover.Alpha(1)}
	}
	return frosted{blur: glass.Blur{Radius: floatBlur}, tint: p.Popover.Alpha(theme.FloatingAlpha)}
}

// Floating gives a floating layer's panel its look: the frosted material,
// Kit's large radius, the border, the shadow and the popover's text color.
// Callers set their own padding and size, and may round it otherwise.
func Floating(c *ui.Context, panel ui.Element) ui.Element {
	p := theme.Of(c)
	return panel.Material(Frosted(c)).Radius(p.RadiusLg).Border(1, p.Border).
		Shadow(0, 8, 24, 0, theme.Shade(0.18)).TextColor(p.PopoverForeground)
}

// Tip gives anchor a frosted tooltip of text, above it (Kit's Tooltip:
// text_xs on the floating surface). It returns anchor.
func Tip(c *ui.Context, anchor ui.Element, text string) ui.Element {
	if text == "" {
		return anchor
	}
	ui.TooltipBase(c, anchor, func(tip ui.Element) {
		Floating(c, tip).Padding(4, 8).Radius(theme.Of(c).Radius).Shadow(0, 4, 12, 0, theme.Shade(0.12)).MaxWidth(320)
		tip.Children(func() { ui.Text(c, text).FontSize(12) })
	})
	return anchor
}

// HoverCard shows what build makes in a frosted card beside anchor once
// the pointer rests on it (Kit's HoverCard, 600 ms), after it and lined up
// with its middle, or before it where there is no room.
func HoverCard(c *ui.Context, anchor ui.Element, width float32, build func()) {
	ui.TooltipBase(c, anchor, func(tip ui.Element) {
		tip.AttachTo(anchor, ui.AnchorRight, ui.AnchorLeft).MarginStart(8)
		Floating(c, tip).Width(width).Padding(12).Gap(8).Children(build)
	})
}

// Menu is a menu drawn in the window on the frosted surface, built on
// MyGo's MenuBase, which keeps its keys, focus and placement.
type Menu struct {
	c     *ui.Context
	parts ui.MenuParts
}

// Item is a menu item: Hint is the shortcut shown after the label, Checked
// shows a check before it.
type Item struct {
	Label    string
	Icon     *ui.SVG
	Hint     string
	Checked  bool
	Disabled bool
	Danger   bool
}

// Item adds an item and reports whether it was chosen, which closes the
// menu.
func (m *Menu) Item(it Item) bool {
	row := m.parts.Item()
	m.row(row, it, false)
	return row.Clicked()
}

// Separator adds a line between groups of items.
func (m *Menu) Separator() {
	p := theme.Of(m.c)
	ui.Box(m.c).Height(1).Margin(4, 8).Background(p.Border).Shrink(0)
}

// Heading adds a muted heading over the items after it.
func (m *Menu) Heading(text string) {
	ui.Text(m.c, text).FontSize(12).Padding(6, 8, 2, 8).SingleLine().TextColor(theme.Of(m.c).MutedForeground)
}

// Submenu adds an item opening a submenu that build fills.
func (m *Menu) Submenu(label string, icon *ui.SVG, build func(m *Menu)) {
	row := m.parts.SubmenuItem()
	m.row(row, Item{Label: label, Icon: icon}, true)
	m.parts.Submenu(row, func(panel ui.Element) {
		menuPanel(m.c, panel)
		build(m)
	})
}

// row styles an item as Kit's menu rows: 14 DIP text, a 16 DIP icon, the
// accent fill on the highlight.
func (m *Menu) row(row ui.Element, it Item, sub bool) {
	c := m.c
	p := theme.Of(c)
	row.Label(it.Label).Disabled(it.Disabled).Height(28).PaddingX(8).Gap(8).Radius(p.Radius).Shrink(0)
	text, muted := p.PopoverForeground, p.MutedForeground
	switch {
	case it.Disabled:
		text = p.MutedForeground.Alpha(0.6)
	case row.Highlighted():
		text = p.HoverForeground
		row.Background(p.Hover)
	}
	if it.Danger && !it.Disabled {
		text = p.Danger
	}
	row.TextColor(text).Children(func() {
		if it.Icon != nil {
			ui.Icon(c, it.Icon).FontSize(16).Shrink(0)
		}
		ui.Text(c, it.Label).FontSize(14).SingleLine().Grow(1).Shrink(1).MinWidth(0)
		if it.Hint != "" {
			ui.Text(c, it.Hint).FontSize(12).SingleLine().TextColor(muted).Shrink(0)
		}
		switch {
		case sub:
			ui.Icon(c, icons.ChevronRight).FontSize(14).TextColor(muted)
		case it.Checked:
			ui.Icon(c, icons.Check).FontSize(14)
		}
	})
}

func menuPanel(c *ui.Context, panel ui.Element) {
	Floating(c, panel).MinWidth(180).MaxHeight(480).Padding(4).Gap(1)
}

// MenuButton makes a button opening a menu on the frosted surface below
// it. trigger styles the button and builds its content; place, when not
// nil, places the panel otherwise (AttachTo). The menu's open state lives
// in the holder the button is built in.
func MenuButton(c *ui.Context, key any, trigger func(t ui.Element), place func(panel, t ui.Element), build func(m *Menu)) ui.Element {
	holder := ui.Box(c.Key(key)).Shrink(0)
	open := ui.Local(holder, "open", func() bool { return false })
	var t ui.Element
	holder.Children(func() {
		parts := ui.MenuBase(c, open)
		t = parts.Trigger
		trigger(t)
		parts.Popup(func(panel ui.Element) {
			menuPanel(c, panel)
			if place != nil {
				place(panel, t)
			}
			build(&Menu{c: c, parts: parts})
		})
	})
	return t
}

// GhostLook gives a button without a look Kit's ghost states, as Ghost.
func GhostLook(c *ui.Context, b ui.Element, selected bool) ui.Element {
	p := theme.Of(c)
	b.Radius(p.Radius).Center().TextColor(p.Foreground).Cursor(ui.CursorPointer)
	switch {
	case b.Pressed() || selected:
		b.Background(p.Active)
	case b.Hovered():
		b.Background(p.Hover)
	}
	return b
}

// IconMenu is an IconButton opening a menu.
func IconMenu(c *ui.Context, key any, icon *ui.SVG, label string, size float32, selected bool, place func(panel, t ui.Element), build func(m *Menu)) ui.Element {
	return MenuButton(c, key, func(t ui.Element) {
		GhostLook(c, t, selected).Label(label).Size(size, size)
		Tip(c, t, label)
		glyph := float32(16)
		if size < 24 {
			glyph = 14
		}
		t.Children(func() { ui.Icon(c, icon).FontSize(glyph) })
	}, place, build)
}

// Toasts builds the window's toasts on the frosted surface, at its bottom
// right: an icon of the outcome, the title, the action and a close button.
// Call it once in the view.
func Toasts(c *ui.Context) {
	p := theme.Of(c)
	ui.ToastViewportBase(c, func(viewport ui.Element, toasts []ui.Toast) {
		viewport.Padding(16).AlignItems(ui.End).Justify(ui.End).Gap(8)
		for _, t := range toasts {
			toast := ui.ToastBase(c, t)
			icon, color := icons.Info, p.MutedForeground
			switch t.Type {
			case "success":
				icon, color = icons.CircleCheck, p.Success
			case "error":
				icon, color = icons.CircleX, p.Danger
			case "warning":
				icon, color = icons.TriangleAlert, p.Warning
			}
			Floating(c, toast.Root.Row()).Gap(12).Padding(10, 12).MaxWidth(520).MinWidth(240).Children(func() {
				ui.Icon(c, icon).FontSize(16).TextColor(color).Shrink(0)
				ui.Column(c).Grow(1).Shrink(1).Gap(2).Children(func() {
					ui.Text(c, t.Title).FontSize(14)
					if t.Description != "" {
						ui.Text(c, t.Description).FontSize(12).TextColor(p.MutedForeground)
					}
				})
				if t.Action != "" {
					GhostLook(c, toast.ActionButton(), false).Height(28).PaddingX(8).Label(t.Action).
						Children(func() { ui.Text(c, t.Action).FontSize(14) })
				}
				GhostLook(c, toast.CloseButton(), false).Size(20, 20).Label(i18n.T(c, "close")).TextColor(p.MutedForeground).
					Children(func() { ui.Icon(c, icons.Close).FontSize(14) })
			})
		}
	})
}

// Select is a choice among options, its popup on the frosted surface
// (MyGo's SelectBase keeps the keys and the focus). It returns the
// trigger, whose Changed reports a choice.
func Select(c *ui.Context, chosen *string, options []string) ui.Element {
	p := theme.Of(c)
	sel := ui.SelectBase(c, chosen)
	t := sel.Trigger.Height(32).PaddingX(10).Gap(8).Radius(p.Radius).Border(1, p.Input).Background(p.Background).
		TextColor(p.Foreground).Cursor(ui.CursorPointer)
	if t.Hovered() {
		t.Background(p.Hover)
	}
	t.Children(func() {
		ui.Text(c, *chosen).FontSize(14).SingleLine().Grow(1).Shrink(1)
		ui.Icon(c, icons.ChevronDown).FontSize(14).TextColor(p.MutedForeground)
	})
	sel.Popup(func(panel ui.Element) {
		menuPanel(c, panel)
		panel.Margin(4, 0, 0, 0)
		for _, o := range options {
			item := sel.Item(o).Height(28).PaddingX(8).Gap(8).Radius(p.Radius)
			if item.Highlighted() {
				item.Background(p.Hover).TextColor(p.HoverForeground)
			}
			item.Children(func() {
				ui.Text(c, o).FontSize(14).SingleLine().Grow(1)
				if o == *chosen {
					ui.Icon(c, icons.Check).FontSize(14)
				}
			})
		}
	})
	return t
}
