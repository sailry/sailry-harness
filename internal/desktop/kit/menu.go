// Menu's motion and Select's chevron are adapted from Lobe MyGo's
// overlay.DropdownMenu and field.Select (github.com/lobehub/lobe-mygo
// pkg/overlay/dropdown.go and pkg/field/select.go at 5b3ecde):
//
// MIT License
//
// Copyright (c) 2026 LobeHub
//
// Permission is hereby granted, free of charge, to any person obtaining a
// copy of this software and associated documentation files (the
// "Software"), to deal in the Software without restriction, including
// without limitation the rights to use, copy, modify, merge, publish,
// distribute, sublicense, and/or sell copies of the Software, and to
// permit persons to whom the Software is furnished to do so, subject to
// the following conditions:
//
// The above copyright notice and this permission notice shall be included
// in all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS
// OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
// MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT.
// IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY
// CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT,
// TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE
// SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
//
// Changes: Kit's rows, panel and trigger as below, kept from before the
// port; from the source, the panel's popup motion from its attached
// origin, the rows' color transition and the chevron turning while the
// select is open, with the ease inlined.

package kit

import (
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// Menu is Kit's DropdownMenu, the one left-click menu of the app, built on
// MyGo's headless MenuBase: MyGo's drawn menus (PopoverMenuButton) paint the
// window's Background and highlight with the saturated Accent, and its Theme
// has no menu slot to set them once. The panel is a floating layer as the
// app's popovers (Palette.Floating, MyGo's popover radius, border and
// shadow); rows are Kit's: 26 high, 8 apart from the edges, the highlight in
// accent.background with accent.foreground, a left slot holding the icon or,
// without one, the check, then the label, a muted trailing text or action,
// the check on the right where the menu asks, and the shortcut. Keys, focus
// and roles are MenuBase's. Right-click menus stay the system's
// (ContextMenu).
//
// Build items in the order they show; an item draws once Chosen asks
// whether it was chosen, which is true in the pass after its click.
type Menu struct {
	collect bool
	items   []*Item
	n       *int // the items counted, shared with the submenus
	chosen  int
}

// Item is a row of a Menu, set up by its methods before Chosen.
type Item struct {
	m                         *Menu
	index                     int
	label, trailing, shortcut string
	description               string
	icon                      *ui.SVG
	image                     bool
	checked, danger, disabled bool
	separator                 bool
	action                    string
	act                       func()
	sub                       *Menu
}

// MenuOptions place a Menu and set its look. At and Self attach the menu's
// point Self to the trigger's point At (below, lined up with its start,
// when both are zero), 4 apart; MaxHeight scrolls longer menus; CheckRight
// puts checks after the label (Kit's check_side(Side::Right)); Match
// makes the menu as wide as its trigger, as a select's dropdown.
type MenuOptions struct {
	At, Self   ui.Anchor
	MaxHeight  float32
	CheckRight bool
	Match      bool
}

// menuOpen keys the open state of the menus, by their id, on the root.
type menuOpen struct{}

// menuEntry is a menu's state: whether it shows, and whether it was asked
// for since the last SweepMenus.
type menuEntry struct{ open, used bool }

// MenuButton makes a menu button whose trigger look styles, with a Menu
// that build fills while it shows. id names the menu among the window's,
// keeping whether it shows across frames.
func MenuButton(c *ui.Context, id string, look func(b ui.Element), build func(m *Menu), options ...MenuOptions) ui.Element {
	var o MenuOptions
	if len(options) > 0 {
		o = options[0]
	}
	open := menuState(c, id)
	parts := ui.MenuBase(c, open)
	b := parts.Trigger
	look(b)
	if !*open {
		return b
	}
	m := &Menu{collect: true}
	build(m)
	var ch choice
	parts.Popup(func(panel ui.Element) {
		layer(c, open, func() { m.draw(c, parts, panel, b, o, &ch) })
	})
	if ch.chosen != 0 && ch.act == nil {
		build(&Menu{chosen: ch.chosen})
	}
	if ch.act != nil {
		*open = false
		ch.act()
	}
	return b
}

// OpenMenu opens or closes the MenuButton named id from the app, as a row
// opening its submenu on hover.
func OpenMenu(c *ui.Context, id string, open bool) {
	*menuState(c, id) = open
}

// menuState is whether the menu named id shows.
func menuState(c *ui.Context, id string) *bool {
	states := ui.Local(c.Root(), menuOpen{}, func() map[string]*menuEntry { return map[string]*menuEntry{} })
	entry := (*states)[id]
	if entry == nil {
		entry = &menuEntry{}
		(*states)[id] = entry
	}
	entry.used = true
	return &entry.open
}

// SweepMenus forgets the menus that no frame asked for since the last call:
// a menu whose button is gone neither stays in the window's state nor comes
// back open with its button. The window calls it once a frame, before it
// builds; the menus of the frame before are kept, with the clicks they took.
func SweepMenus(c *ui.Context) {
	states := ui.Local(c.Root(), menuOpen{}, func() map[string]*menuEntry { return map[string]*menuEntry{} })
	for id, entry := range *states {
		if !entry.used {
			delete(*states, id)
		}
		entry.used = false
	}
}

var (
	// menuRow eases a row's highlight in and out (Lobe's menuItemTransition).
	menuRow = ui.ElementTransition{Duration: 150 * time.Millisecond, Ease: EaseOut, Colors: true}
	// chevronTurn turns a select's chevron as it opens and closes.
	chevronTurn = ui.Animation{Duration: 150 * time.Millisecond, Ease: EaseOut}
)

// next counts an item of the menu or of one of its submenus.
func (m *Menu) next() int {
	if m.n == nil {
		m.n = new(int)
	}
	*m.n++
	return *m.n
}

// Item adds an item labeled label.
func (m *Menu) Item(label string) *Item {
	it := &Item{m: m, index: m.next(), label: label}
	if m.collect {
		m.items = append(m.items, it)
	}
	return it
}

// Submenu adds an item labeled label opening a submenu that build fills,
// as Kit's PopupMenu submenus: it opens as the pointer moves onto it or
// clicks it, and for Right; its items are chosen as the menu's.
func (m *Menu) Submenu(label string, build func(m *Menu)) *Item {
	it := m.Item(label)
	sub := &Menu{collect: m.collect, n: m.n, chosen: m.chosen}
	build(sub)
	it.sub = sub
	return it
}

// Separator adds a line between groups of items.
func (m *Menu) Separator() {
	m.next()
	if m.collect {
		m.items = append(m.items, &Item{separator: true})
	}
}

// Icon shows icon in the item's left slot.
func (it *Item) Icon(icon *ui.SVG) *Item { it.icon = icon; return it }

// Image shows mark in the item's left slot in its own colors, as a brand
// mark (icons.BrandColor); a nil mark leaves the slot empty.
func (it *Item) Image(mark *ui.SVG) *Item { it.icon, it.image = mark, mark != nil; return it }

// Checked marks the item with a check: in the left slot where it has no
// icon, or after the label in a menu with CheckRight.
func (it *Item) Checked(on bool) *Item { it.checked = on; return it }

// Description shows a muted line under the label, as Kit's choice rows: the
// row grows to two lines, its icon 16, and a checked row keeps the selected
// fill.
func (it *Item) Description(text string) *Item { it.description = text; return it }

// Trailing shows muted text after the label.
func (it *Item) Trailing(text string) *Item { it.trailing = text; return it }

// Shortcut shows the keys that do the same, muted, at the end.
func (it *Item) Shortcut(text string) *Item { it.shortcut = text; return it }

// Danger colors the item as a destructive action.
func (it *Item) Danger() *Item { it.danger = true; return it }

// Disabled keeps the item from being chosen.
func (it *Item) Disabled(off bool) *Item { it.disabled = off; return it }

// Action adds an extra-small ghost button labeled label after the label,
// as the host Update button; pressing it runs fn and closes the menu
// without choosing the item.
func (it *Item) Action(label string, fn func()) *Item {
	it.action, it.act = label, fn
	return it
}

// Chosen reports whether the item was chosen; it closes the menu.
func (it *Item) Chosen() bool { return !it.m.collect && it.index == it.m.chosen }

// choice is what a pass of a menu chose: the item clicked, by its index,
// or the action pressed. Submenus are built after their menu, as MyGo
// builds them once the menu's items are, so they fill it in place.
type choice struct {
	chosen int
	act    func()
}

// draw fills panel with the items collected, noting in ch what was
// chosen.
func (m *Menu) draw(c *ui.Context, parts ui.MenuParts, panel, trigger ui.Element, o MenuOptions, ch *choice) {
	_, h := c.Size()
	at, self := o.At, o.Self
	if at == 0 && self == 0 {
		at, self = ui.AnchorBottomStart, ui.AnchorTopStart
	}
	panel.AttachTo(trigger, at, self)
	panel.AttachedOrigin().Transition(toward(popup, menuGap(panel, at, self, 4)))
	maxH := h - 16
	if o.MaxHeight > 0 {
		maxH = min(maxH, o.MaxHeight)
	}
	Floating(c, panel.Padding(4).Gap(2).MinWidth(128).MaxWidth(500).MaxHeight(maxH))
	if w := trigger.LayoutBounds().W; o.Match && w > 0 {
		panel.MinWidth(w).MaxWidth(w)
	}
	m.rows(c, parts, o, ch)
}

// rows draws the menu's items in the panel being built, and its
// submenus' in theirs, noting in ch the item clicked or the action
// pressed.
func (m *Menu) rows(c *ui.Context, parts ui.MenuParts, o MenuOptions, ch *choice) {
	p := theme.Of(c)
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
		switch {
		case it.sub != nil:
			row = parts.SubmenuItem()
		case it.checked:
			row = parts.CheckItem(true)
		default:
			row = parts.Item()
		}
		row.Label(it.label).Disabled(it.disabled).PaddingX(8).Gap(4).Radius(min(p.Radius, 8)).
			AlignItems(ui.Center).Shrink(0).Transition(menuRow)
		if it.description != "" {
			row.PaddingY(8).Gap(8).MaxWidth(400)
		} else {
			row.Height(30)
		}
		color, muted := p.PopoverForeground, p.MutedForeground
		switch {
		case it.disabled:
			color = p.MutedForeground
		case row.Highlighted():
			color = p.HoverForeground
			row.Background(p.Hover)
		case it.checked && it.description != "":
			row.Background(p.Active)
		}
		if it.danger && !it.disabled {
			color = p.Danger
		}
		row.TextColor(color)
		row.Children(func() {
			if it.description != "" {
				if it.icon != nil {
					ui.Icon(c, it.icon).FontSize(16).Shrink(0)
				}
				ui.Column(c).Grow(1).Shrink(1).MinWidth(0).Gap(2).Children(func() {
					ui.Text(c, it.label).FontSize(14).LineHeight(1.25)
					ui.Text(c, it.description).FontSize(12).LineHeight(1.25).TextColor(muted)
				})
				if it.checked && o.CheckRight {
					ui.Icon(c, icons.Check).FontSize(12).Shrink(0)
				}
				return
			}
			if slot {
				box := ui.Box(c).Size(12, 12).Shrink(0)
				switch {
				case it.image:
					box.Size(14, 14).Children(func() { ui.Image(c, it.icon).Size(14, 14) })
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
					// Clicked, not OnClick: the menu reads ch in this build.
					if GhostLook(c, ui.ButtonBase(c).Label(it.action), false).Height(20).PaddingX(6).Shrink(0).
						Children(func() { ui.Text(c, it.action).FontSize(12) }).Clicked() {
						ch.act = fn
					}
				}
				if it.checked && o.CheckRight {
					ui.Icon(c, icons.Check).FontSize(12).Shrink(0)
				}
				if it.shortcut != "" {
					ui.Text(c, it.shortcut).FontSize(12).SingleLine().Shrink(0).TextColor(muted)
				}
				if it.sub != nil {
					ui.Icon(c, icons.ChevronRight).FontSize(12).Shrink(0).TextColor(muted)
				}
			})
		})
		if it.sub != nil {
			sub := it.sub
			parts.Submenu(row, func(panel ui.Element) {
				Floating(c, panel.Padding(4).Gap(2).MinWidth(128).MaxWidth(500))
				sub.rows(c, parts, o, ch)
			})
			continue
		}
		if row.Clicked() {
			ch.chosen = it.index
		}
	}
}

// menuGap keeps a menu gap apart from its trigger on the side it shows,
// which it returns.
func menuGap(panel ui.Element, at, self ui.Anchor, gap float32) Placement {
	switch {
	case top(self) && bottom(at):
		panel.Margin(gap, 0, 0, 0)
		return Bottom
	case bottom(self) && top(at):
		panel.Margin(0, 0, gap, 0)
		return Top
	case end(at) && !end(self):
		panel.MarginStart(gap)
		return Right
	default:
		panel.MarginEnd(gap)
		return Left
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

// Select is Kit's Select: a trigger showing *chosen with a chevron that
// turns while it is open, the height and fill of a text input (MyGo's
// TextInput: the input border on the surface, its padding), opening a Menu of options as wide as it, in
// the popover color, with a check on the chosen one. It reports whether
// the user chose another option. id names it as MenuButton's; label
// names the trigger for assistive technology.
func Select(c *ui.Context, id, label string, chosen *string, options []string, disabled bool, values ...[]string) (ui.Element, bool) {
	choices := make([]SelectOption, len(options))
	for index, option := range options {
		choices[index] = SelectOption{Value: option, Label: option}
		if len(values) > 0 {
			choices[index].Value = values[0][index]
		}
	}
	return SelectField(c, id, chosen, SelectProps{Label: label, Options: choices, Disabled: disabled})
}

// SelectOption is one choice of a SelectField: the value chosen and the
// label shown for it.
type SelectOption struct {
	Value, Label string
	// Image, when an option has one, shows before the labels in the menu
	// in its own colors, as a brand's mark.
	Image *ui.SVG
	// Disabled keeps the option from being chosen.
	Disabled bool
}

// SelectProps is what a SelectField shows.
type SelectProps struct {
	// Label names the trigger for assistive technology.
	Label string
	// Placeholder shows, muted, while *chosen is none of the options;
	// without one the trigger shows *chosen itself.
	Placeholder string
	Options     []SelectOption
	Disabled    bool
}

// SelectTrigger gives a button base the Select's trigger: the input's frame
// around the shown choice (muted for a placeholder) and the chevron, turned
// while open. A select with a popup of its own (a searchable one) uses it
// for the same control.
func SelectTrigger(c *ui.Context, b ui.Element, shown string, placeholder, open bool) ui.Element {
	p, t := theme.Of(c), c.Theme()
	b.Padding(t.Space(1.5), t.Space(2.5)).Gap(8).MinWidth(140).Radius(p.Radius).
		Border(1, p.Input).Background(t.Surface).AlignItems(ui.Center).Justify(ui.SpaceBetween).Cursor(ui.CursorPointer)
	if b.IsDisabled() {
		b.Opacity(0.5)
	}
	return b.Children(func() {
		value := ui.Text(c, shown).FontSize(14).SingleLine().Shrink(1).MinWidth(0)
		if placeholder {
			value.TextColor(p.MutedForeground)
		}
		turn := float32(0)
		if open {
			turn = 180
		}
		arrow := ui.Icon(c, icons.ChevronDown).FontSize(14).TextColor(p.MutedForeground).Shrink(0)
		arrow.Rotate(arrow.AnimateTo("turn", turn, chevronTurn))
	})
}

// SelectField is Select over options with values, a placeholder and
// disabled options: the same trigger and dropdown.
func SelectField(c *ui.Context, id string, chosen *string, o SelectProps) (ui.Element, bool) {
	changed := false
	shown, placeholder := *chosen, false
	if o.Placeholder != "" {
		shown, placeholder = o.Placeholder, true
	}
	for _, option := range o.Options {
		if option.Value == *chosen {
			shown, placeholder = option.Label, false
			break
		}
	}
	b := MenuButton(c, id, func(b ui.Element) {
		SelectTrigger(c, b.Label(o.Label).Disabled(o.Disabled), shown, placeholder, *menuState(c, id))
	}, func(m *Menu) {
		for _, option := range o.Options {
			if m.Item(option.Label).Image(option.Image).Checked(option.Value == *chosen).Disabled(option.Disabled).Chosen() && option.Value != *chosen {
				*chosen, changed = option.Value, true
			}
		}
	}, MenuOptions{CheckRight: true, MaxHeight: 320, Match: true})
	return b, changed
}
