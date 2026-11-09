package shell

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// selection is the selected text dialog of the system shortcut: the text
// another app had selected, to read aloud or quote into the conversation it
// was captured over.
type selection struct {
	open bool
	text string
	chat string // the conversation shown as it was captured, "" for none
}

// SystemShortcuts are the system-wide shortcuts on the keys they have now, by
// accelerator: their commands go to Do. cmd/sailry-desktop registers them
// with the system as the app starts.
func (a *App) SystemShortcuts() map[string]string {
	out := map[string]string{}
	for _, s := range settings.Registry {
		if !s.System {
			continue
		}
		if b, ok := a.binding(s); ok && b.Key != 0 {
			out[Accelerator(b.Mods, b.Key)] = s.ID
		}
	}
	return out
}

// captureSelection is the selection shortcut (dispatch "system.selection"):
// the preview reads no other app, so it shows the preview's sample text.
func (a *App) captureSelection() {
	if !a.Preview {
		return
	}
	if a.dialog() {
		return
	}
	chat := ""
	if a.Page == ConversationPage {
		chat = a.Sidebar.Selected
	}
	a.selected = selection{open: true, text: preview.Selection, chat: chat}
}

// ShowSelection presents text captured by the controller's native integration.
func (a *App) ShowSelection(text string) {
	if a.dialog() || text == "" {
		return
	}
	chat := ""
	if a.Page == ConversationPage {
		chat = a.Sidebar.Selected
	}
	a.selected = selection{open: true, text: text, chat: chat}
}

// selectionDialog is the dialog: the title, the text up to 260 high,
// selectable, then Read aloud and Quote, which adds it to the captured
// conversation's draft and closes, disabled without one.
func (a *App) selectionDialog(c *ui.Context) {
	s := &a.selected
	if !s.open {
		return
	}
	p := theme.Of(c)
	w, _ := c.Size()
	title := i18n.T(c, "selection_title")
	kit.Dialog(c.Key("selection"), &s.open, func() {
		ui.Text(c, title).FontSize(16).FontWeight(600).TextColor(p.PopoverForeground)
		ui.Scroll(c.Key("text")).MaxHeight(260).Children(func() {
			ui.Text(c, s.text).FontSize(14).Selectable()
		})
		ui.Row(c.Key("footer")).Gap(8).Justify(ui.End).Children(func() {
			icon, label := icons.Volume, "reader_read"
			if a.Hooks.ReaderActive != nil && a.Hooks.ReaderActive("selection/"+s.chat) {
				icon, label = icons.Square, "reader_stop"
			}
			kit.IconButton(c.Key("read"), icon, i18n.T(c, label), 32, false).OnClick(func() {
				if !a.Preview && a.Hooks.ReadSelection != nil {
					a.Hooks.ReadSelection(s.chat, s.text)
				} else {
					a.notice(c)
				}
			})
			quote := kit.Secondary(c.Key("quote"), i18n.T(c, "message_quote"))
			if s.chat == "" || a.Data.Find(s.chat) == nil {
				quote.Disabled(true).Opacity(0.5)
			}
			quote.OnClick(func() {
				comp := a.Composer(s.chat)
				if comp.Draft != "" {
					comp.Draft += "\n\n"
				}
				comp.Draft += "> " + s.text
				s.open = false
			})
		})
	}).Width(min(480, w-48)).Label(title)
}
