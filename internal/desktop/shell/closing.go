package shell

import (
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// CloseRequest is a window close or an application quit that would lose
// something. With unsaved Documents the prompt names them and offers Save
// all, Discard changes and Cancel; without any it confirms the quit that
// stops local work. Proceed closes the window or quits; the owner
// (cmd/sailry-desktop) lets that close pass.
type CloseRequest struct {
	// Documents are the unsaved documents now, read again after a save.
	Documents func() []string
	// Save starts saving every unsaved document; Saving reports whether a
	// save is still under way. A failed save tells its own toast.
	Save    func()
	Saving  func() bool
	Proceed func()
}

type closing struct {
	CloseRequest
	names  []string
	open   bool
	saving bool
}

// closePoll is how often the prompt looks at saves under way, which another
// window's frames may finish.
const closePoll = 100 * time.Millisecond

// ConfirmClose opens the close prompt. A prompt already showing stays as it
// is, so repeated close requests ask once.
func (a *App) ConfirmClose(r CloseRequest) {
	if a.closing != nil && a.closing.open {
		return
	}
	a.closing = &closing{CloseRequest: r, names: r.Documents(), open: true}
}

// Closing reports whether the close prompt shows.
func (a *App) Closing() bool { return a.closing != nil && a.closing.open }

func (a *App) closeDialog(c *ui.Context) {
	st := a.closing
	if st == nil {
		return
	}
	if st.saving {
		switch {
		case !st.open:
			// Cancelled while saving: the saves finish, the window stays.
			st.saving = false
		case st.Saving():
			c.After(closePoll)
		case len(st.Documents()) == 0:
			st.saving, st.open = false, false
			a.closing = nil
			st.Proceed()
			return
		default:
			// A save failed and said so in its toast: the drafts stay.
			st.saving, st.open = false, false
		}
	}
	p := theme.Of(c)
	_, h := c.Size()
	kit.Dialog(c.Key("close-guard"), &st.open, func() {
		if len(st.names) == 0 {
			ui.Text(c, i18n.T(c, "app_exit_confirm")).FontSize(16).FontWeight(600).PaddingEnd(24)
			ui.Text(c, i18n.T(c, "app_exit_description")).FontSize(14)
			ui.Row(c).Gap(8).Justify(ui.End).Children(func() {
				kit.Secondary(c.Key("cancel"), i18n.T(c, "settings_cancel")).OnClick(func() { st.open = false })
				kit.Primary(c.Key("quit"), i18n.T(c, "app_exit"), false).Children(func() { ui.Text(c, i18n.T(c, "app_exit")).FontSize(14) }).OnClick(func() { a.finishClose(st) })
			})
			return
		}
		ui.Text(c, i18n.T(c, "files_unsaved")).FontSize(16).FontWeight(600).PaddingEnd(24)
		ui.Scroll(c.Key("documents")).MaxHeight(min(160, h*0.4)).Shrink(1).Gap(4).Children(func() {
			for _, name := range st.names {
				ui.Text(c.Key(name), name).FontSize(14).TextColor(p.MutedForeground)
			}
		})
		ui.Text(c, i18n.T(c, "files_unsaved_description")).FontSize(14)
		ui.Row(c).Gap(8).Justify(ui.End).Children(func() {
			kit.Secondary(c.Key("cancel"), i18n.T(c, "settings_cancel")).Disabled(st.saving).OnClick(func() { st.open = false })
			kit.Secondary(c.Key("discard"), i18n.T(c, "files_unsaved_discard")).Disabled(st.saving).OnClick(func() { a.finishClose(st) })
			kit.Primary(c.Key("save"), i18n.T(c, "files_save_all"), st.saving).Children(func() { ui.Text(c, i18n.T(c, "files_save_all")).FontSize(14) }).OnClick(func() {
				st.saving = true
				st.Save()
			})
		})
	}).Width(440)
	if !st.open && !st.saving && a.closing == st {
		a.closing = nil
	}
}

func (a *App) finishClose(st *closing) {
	if a.closing != st {
		return
	}
	st.open = false
	a.closing = nil
	st.Proceed()
}
