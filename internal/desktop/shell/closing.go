package shell

import (
	"strings"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
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
	if st.saving {
		return
	}
	if len(st.names) == 0 {
		if kit.Confirm(c.Key("close-guard").Key(st), &st.open, kit.Confirmation{
			Title: i18n.T(c, "app_exit_confirm"), Message: i18n.T(c, "app_exit_description"),
			Buttons: []string{i18n.T(c, "settings_cancel"), i18n.T(c, "app_exit")}, Destructive: []int{1},
		}) == 1 {
			a.finishClose(st)
		}
	} else {
		open := st.open
		switch kit.Confirm(c.Key("close-guard").Key(st), &open, kit.Confirmation{
			Title: i18n.T(c, "files_unsaved"), Message: strings.Join(st.names, "\n") + "\n\n" + i18n.T(c, "files_unsaved_description"),
			Buttons: []string{i18n.T(c, "settings_cancel"), i18n.T(c, "files_unsaved_discard"), i18n.T(c, "files_save_all")},
			Default: 2, Destructive: []int{1},
		}) {
		case 0:
			st.open = false
		case 1:
			a.finishClose(st)
		case 2:
			st.saving = true
			st.Save()
			c.After(closePoll)
		}
	}
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
