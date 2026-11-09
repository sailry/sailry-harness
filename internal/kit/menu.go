package kit

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-go/internal/icons"
	"github.com/sailry/sailry-go/internal/theme"
)

// Menu is Kit's DropdownMenu (component/menu/popup_menu.rs), the one
// left-click menu of the app, built on MyGo's headless MenuBase: MyGo's
// drawn menus (PopoverMenuButton) paint the window's Background and
// highlight with the saturated Accent, and its Theme has no menu slot to
// set them once. The panel is a floating layer as the app's popovers
// (Palette.Floating, MyGo's popover radius, border and shadow); rows are
// Kit's: 26 high, 8 apart from the edges, the highlight in accent.background
// with accent.foreground, a left slot holding the icon or, without one,
// the check, then the label, a muted trailing text or action, the check
// on the right where the menu asks, and the shortcut. Keys, focus and
// roles are MenuBase's. Right-click menus stay the system's (ContextMenu).
//
// Build items in the order they show; an item draws once Chosen asks
// whether it was chosen, which is true in the pass after its click.
type Menu struct {
	collect bool
	items   []*Item
	n       int
	chosen  int
}

// Item is a row of a Menu, set up by its methods before Chosen.
type Item struct {
	m                         *Menu
	index                     int
	label, trailing, shortcut string
	icon                      *ui.SVG
	checked, danger, disabled bool
	separator                 bool
	action                    string
	act                       func()
}

// MenuOptions place a Menu and set its look. At and Self attach the menu's
// point Self to the trigger's point At (below, lined up with its start,
// when both are zero), 4 apart; MaxHeight scrolls longer menus; CheckRight
// puts checks after the label (Kit's check_side(Side::Right)).
type MenuOptions struct {
	At, Self   ui.Anchor
	MaxHeight  float32
	CheckRight bool
}

// menuOpen keys the open state of the menus, by their id, on the root.
type menuOpen struct{}

// MenuButton makes a menu button whose trigger look styles, with a Menu
// that build fills while it shows. id names the menu among the window's,
// keeping whether it shows across frames.
func MenuButton(c *ui.Context, id string, look func(b ui.Element), build func(m *Menu), options ...MenuOptions) ui.Element {
	var o MenuOptions
	if len(options) > 0 {
		o = options[0]
	}
	states := ui.Local(c.Root(), menuOpen{}, func() map[string]*bool { return map[string]*bool{} })
	open := (*states)[id]
	if open == nil {
		open = new(bool)
		(*states)[id] = open
	}
	parts := ui.MenuBase(c, open)
	b := parts.Trigger
	look(b)
	if !*open {
		return b
	}
	m := &Menu{collect: true}
	build(m)
	chosen := 0
	var act func()
	parts.Popup(func(panel ui.Element) {
		chosen, act = m.draw(c, parts, panel, b, o)
	})
	if chosen != 0 && act == nil {
		build(&Menu{chosen: chosen})
	}
	if act != nil {
		*open = false
		act()
	}
	return b
}

// Item adds an item labeled label.
func (m *Menu) Item(label string) *Item {
	m.n++
	it := &Item{m: m, index: m.n, label: label}
	if m.collect {
		m.items = append(m.items, it)
	}
	return it
}

// Separator adds a line between groups of items.
func (m *Menu) Separator() {
	m.n++
	if m.collect {
		m.items = append(m.items, &Item{separator: true})
	}
}

// Icon shows icon in the item's left slot.
func (it *Item) Icon(icon *ui.SVG) *Item { it.icon = icon; return it }

// Checked marks the item with a check: in the left slot where it has no
// icon, or after the label in a menu with CheckRight.
func (it *Item) Checked(on bool) *Item { it.checked = on; return it }

// Trailing shows muted text after the label.
func (it *Item) Trailing(text string) *Item { it.trailing = text; return it }

// Shortcut shows the keys that do the same, muted, at the end.
func (it *Item) Shortcut(text string) *Item { it.shortcut = text; return it }

// Danger colors the item as a destructive action.
func (it *Item) Danger() *Item { it.danger = true; return it }

// Disabled keeps the item from being chosen.
func (it *Item) Disabled(off bool) *Item { it.disabled = off; return it }

// Action adds an extra-small ghost button labeled label after the label,
// as Rust's host Update button; pressing it runs fn and closes the menu
// without choosing the item.
func (it *Item) Action(label string, fn func()) *Item {
	it.action, it.act = label, fn
	return it
}

// Chosen reports whether the item was chosen; it closes the menu.
func (it *Item) Chosen() bool { return !it.m.collect && it.index == it.m.chosen }

// draw fills panel with the items collected, returning the index of the
// item clicked and the action pressed, if any.
func (m *Menu) draw(c *ui.Context, parts ui.MenuParts, panel, trigger ui.Element, o MenuOptions) (chosen int, act func()) {
	p := theme.Of(c)
	_, h := c.Size()
	at, self := o.At, o.Self
	if at == 0 && self == 0 {
		at, self = ui.AnchorBottomStart, ui.AnchorTopStart
	}
	panel.AttachTo(trigger, at, self)
	menuGap(panel, at, self, 4)
	maxH := h - 16
	if o.MaxHeight > 0 {
		maxH = min(maxH, o.MaxHeight)
	}
	panel.Padding(4).Gap(2).MinWidth(128).MaxWidth(500).MaxHeight(maxH).Radius(p.RadiusLg).
		Background(p.Floating()).TextColor(p.PopoverForeground).Border(1, p.Border).
		Shadow(0, 6, 20, 0, p.Backdrop().Alpha(0.18))
	slot := false
	for _, it := range m.items {
		slot = slot || it.icon != nil || (it.checked && !o.CheckRight)
	}
	for i, it := range m.items {
		if it.separator {
			if i > 0 && i < len(m.items)-1 {
				ui.Box(c).Height(2).Margin(2, -4).Background(p.Border).Shrink(0)
			}
			continue
		}
		var row ui.Element
		if it.checked {
			row = parts.CheckItem(true)
		} else {
			row = parts.Item()
		}
		row.Label(it.label).Disabled(it.disabled).Height(26).PaddingX(8).Gap(4).Radius(min(p.Radius, 8)).
			AlignItems(ui.Center).Shrink(0)
		color, muted := p.PopoverForeground, p.MutedForeground
		switch {
		case it.disabled:
			color = p.MutedForeground
		case row.Highlighted():
			color = p.HoverForeground
			row.Background(p.Hover)
		}
		if it.danger && !it.disabled {
			color = p.Danger
		}
		row.TextColor(color)
		row.Children(func() {
			if slot {
				box := ui.Box(c).Size(12, 12).Shrink(0)
				switch {
				case it.icon != nil:
					box.Children(func() { ui.Icon(c, it.icon).FontSize(12) })
				case it.checked && !o.CheckRight:
					box.Children(func() { ui.Icon(c, icons.Check).FontSize(12) })
				}
			}
			ui.Row(c).Grow(1).Shrink(1).MinWidth(0).Gap(12).AlignItems(ui.Center).Children(func() {
				ui.Text(c, it.label).FontSize(14).SingleLine().Grow(1).Shrink(1).MinWidth(0)
				if it.trailing != "" {
					ui.Text(c, it.trailing).FontSize(12).SingleLine().Shrink(0).TextColor(muted)
				}
				if it.action != "" {
					fn := it.act
					GhostLook(c, ui.ButtonBase(c).Label(it.action), false).Height(20).PaddingX(6).Shrink(0).
						Children(func() { ui.Text(c, it.action).FontSize(12) }).OnClick(func() { act = fn })
				}
				if it.checked && o.CheckRight {
					ui.Icon(c, icons.Check).FontSize(12).Shrink(0)
				}
				if it.shortcut != "" {
					ui.Text(c, it.shortcut).FontSize(12).SingleLine().Shrink(0).TextColor(muted)
				}
			})
		})
		if row.Clicked() {
			chosen = it.index
		}
	}
	return chosen, act
}

// menuGap keeps a menu gap apart from its trigger on the side it shows.
func menuGap(panel ui.Element, at, self ui.Anchor, gap float32) {
	switch {
	case top(self) && bottom(at):
		panel.Margin(gap, 0, 0, 0)
	case bottom(self) && top(at):
		panel.Margin(0, 0, gap, 0)
	case end(at) && !end(self):
		panel.MarginStart(gap)
	default:
		panel.MarginEnd(gap)
	}
}

func top(a ui.Anchor) bool {
	return a == ui.AnchorTopLeft || a == ui.AnchorTop || a == ui.AnchorTopRight || a == ui.AnchorTopStart || a == ui.AnchorTopEnd
}

func bottom(a ui.Anchor) bool {
	return a == ui.AnchorBottomLeft || a == ui.AnchorBottom || a == ui.AnchorBottomRight || a == ui.AnchorBottomStart || a == ui.AnchorBottomEnd
}

func end(a ui.Anchor) bool {
	return a == ui.AnchorRight || a == ui.AnchorEnd || a == ui.AnchorTopEnd || a == ui.AnchorBottomEnd || a == ui.AnchorTopRight || a == ui.AnchorBottomRight
}

// Select is Kit's Select: an outlined trigger showing *chosen with a
// chevron, opening a Menu of options with a check on the chosen one. It
// reports whether the user chose another option. id names it as
// MenuButton's; label names the trigger for assistive technology.
func Select(c *ui.Context, id, label string, chosen *string, options []string, disabled bool) (ui.Element, bool) {
	p := theme.Of(c)
	changed := false
	b := MenuButton(c, id, func(b ui.Element) {
		b.Label(label).Disabled(disabled).Height(32).PaddingX(12).Gap(8).MinWidth(140).Radius(p.Radius).
			Border(1, p.Input).Background(p.Background).AlignItems(ui.Center).Justify(ui.SpaceBetween).Cursor(ui.CursorPointer)
		if disabled {
			b.Opacity(0.5)
		}
		b.Children(func() {
			ui.Text(c, *chosen).FontSize(14).SingleLine().Shrink(1).MinWidth(0)
			ui.Icon(c, icons.ChevronDown).FontSize(14).TextColor(p.MutedForeground).Shrink(0)
		})
	}, func(m *Menu) {
		for _, o := range options {
			if m.Item(o).Checked(o == *chosen).Chosen() && o != *chosen {
				*chosen, changed = o, true
			}
		}
	}, MenuOptions{CheckRight: true, MaxHeight: 320})
	return b, changed
}
