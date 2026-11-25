package settings

import (
	"github.com/ELY-Open-Source/CrychicUI/typography"
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// keyButtonHeight is a key button's height, Kit's small control size: the
// cap with room for the hover fill around it.
const keyButtonHeight = 28

// button is the handle of a shortcut's key button.
func (k *Keys) button(id string) *ui.Handle {
	if k.buttons == nil {
		k.buttons = map[string]*ui.Handle{}
	}
	h := k.buttons[id]
	if h == nil {
		h = &ui.Handle{}
		k.buttons[id] = h
	}
	return h
}

// shortcuts is the global commands, each a row whose button shows its key and
// records a new one when clicked, then the fixed commands' keys, which record
// nothing. Every key is one CrychicUI key cap (typography.Kbd) written as
// menus write it, so a chord reads as one cap and a command's alternatives as
// caps side by side. While recording, the page takes every key: Escape
// cancels, another key is saved or refused, and the reset and clear buttons
// follow the button; when recording ends here, the focus returns to the
// button. Kit and CrychicUI have no shortcut recorder (CrychicUI's
// ShortcutButton fires a shortcut, it does not record one); this is MyGo's
// buttons over a focusable column taking raw keys (HandleInput).
func shortcuts(c *ui.Context, k *Keys) {
	if k.problem != "" {
		kit.Toast(c, kit.Failed, i18n.T(c, k.problem))
		k.problem = ""
	}
	page := ui.Column(c.Key("shortcuts")).Focusable().FocusRing(false).Bind(&k.focus).HandleInput(func(ev ui.InputEvent) bool {
		if !k.Recording() || ev.Kind != ui.InputKeyDown {
			return false
		}
		id := k.Active
		switch {
		case ev.Key == ui.KeyEscape && ev.Mods == 0:
			k.Cancel()
			k.refocus = id
		case ev.Repeat, ev.Key == ui.KeyUnknown:
		default:
			k.Take(Chord{ev.Mods, ev.Key})
		}
		return true
	})
	// The recorder keeps the keys when a click elsewhere, as on a toast's
	// close button, took the focus.
	if k.Recording() && !k.focus.FocusWithin(c) {
		k.focus.Focus(c)
	}
	page.Children(func() {
		kit.Group(c, "commands", i18n.T(c, "settings_global_commands"), "", nil, func() {
			kit.Card(c, func() {
				for s := range k.commands {
					description := ""
					if k.Active != s.ID && (k.Reserved(s) || panelShortcut(s.ID) && k.Chord(s).Key != 0 && k.Effective(s).Key == 0) {
						description = i18n.T(c, "shortcut_reserved")
					}
					kit.WideRow(c, s.ID, i18n.T(c, s.Label), description, func() { shortcutControl(c, k, s) })
				}
			})
		})
		kit.Group(c, "panes", i18n.T(c, "settings_pane_commands"), "", nil, func() {
			kit.Card(c, func() {
				for _, l := range Listed {
					kit.WideRow(c, l.Label, i18n.T(c, l.Label), "", func() { listedControl(c, l) })
				}
			})
		})
	})
	// The cap of a key just taken pressed once; it rests from the next build.
	k.landed = ""
}

// listedControl is a fixed command's keys: muted caps, no button, wrapping
// right-aligned inside the row's control width.
func listedControl(c *ui.Context, l ListedKeys) {
	ui.Row(c.Key("caps")).Grow(1).MinWidth(0).MinHeight(keyButtonHeight).Wrap().Justify(ui.End).AlignItems(ui.Center).
		Gap(6).TextColor(theme.Of(c).MutedForeground).Children(func() {
		for i, ch := range l.Chords {
			typography.Kbd(c.Key(i), kit.KeyText(ch.Mods, ch.Key), false).Shrink(0)
		}
	})
}

// shortcutControl is a shortcut's key button, then while recording reset and
// clear. The ghost button provides the hit target and keyboard focus. Assistive technology
// reads the command as the button's name and what the cap shows as its
// value, so the buttons of unset shortcuts stay apart.
func shortcutControl(c *ui.Context, k *Keys, s Shortcut) {
	p := theme.Of(c)
	recording := k.Active == s.ID
	configured := k.Chord(s)
	ch := configured
	panel := panelShortcut(s.ID)
	if panel {
		ch = k.Effective(s)
	}
	label := i18n.T(c, "shortcut_disabled")
	switch {
	case recording:
		label = i18n.T(c, "shortcut_record")
	case ch.Key != 0:
		label = kit.KeyText(ch.Mods, ch.Key)
	}
	h := k.button(s.ID)
	b := kit.GhostLook(c, ui.ButtonBase(c.Key("key")).Label(i18n.T(c, s.Label)).Value(label).Bind(h), false).
		MinHeight(keyButtonHeight).MinWidth(keyButtonHeight).PaddingX(2).Shrink(0)
	switch {
	case recording:
		b.TextColor(p.Primary)
	case ch.Key == 0:
		b.TextColor(p.MutedForeground)
	case k.Reserved(s):
		b.TextColor(p.Warning)
	}
	b.Children(func() {
		if panel && ch.Key == 0 && !recording {
			ui.Text(c, label).FontSize(12).Shrink(0)
			return
		}
		typography.Kbd(c, label, recording || k.landed == s.ID).Shrink(0)
	})
	if !recording {
		kit.Tooltip(c, b, i18n.T(c, "shortcut_edit"))
	}
	b.OnClick(func() {
		k.Record(s.ID)
		k.focus.Focus(c)
	})
	if k.refocus == s.ID && !k.Recording() {
		k.refocus = ""
		h.Focus(c)
	}
	if !recording {
		return
	}
	_, custom := k.set[s.ID]
	kit.Action(c.Key("reset"), icons.RotateCw, i18n.T(c, "shortcut_reset"), !custom).OnClick(func() {
		k.Reset()
		k.refocus = s.ID
	})
	kit.Action(c.Key("clear"), icons.Close, i18n.T(c, "shortcut_clear"), configured.Key == 0).OnClick(func() {
		k.Clear()
		k.refocus = s.ID
	})
}
