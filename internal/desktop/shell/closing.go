package shell

import (
	"strings"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
)

// CloseDocument identifies the draft that a discard confirmation would lose.
// Each window owns its project editors, even when they show the same file.
type CloseDocument struct {
	Project *panel.Project
	Name    string
	Digest  [32]byte
}

// CloseRequest asks to close a window or quit the application. With unsaved
// documents the prompt offers Save all, Discard and Cancel. Otherwise it
// confirms the quit and warns about any local work that will stop.
type CloseRequest struct {
	// Documents are read again before discarding drafts and after saving.
	Documents      func() []CloseDocument
	StopsLocalWork bool
	// Save starts saving every unsaved document; Saving reports whether a
	// save is still under way. A failed save tells its own toast.
	Save    func()
	Saving  func() bool
	Proceed func()
}

type closing struct {
	CloseRequest
	shown  map[CloseDocument]bool
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
	a.closing = newClosePrompt(r, r.Documents())
}

func newClosePrompt(r CloseRequest, documents []CloseDocument) *closing {
	st := &closing{CloseRequest: r, shown: make(map[CloseDocument]bool, len(documents)), open: true}
	for _, document := range documents {
		st.shown[document] = true
		st.names = append(st.names, document.Name)
	}
	return st
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
		message := ""
		if st.StopsLocalWork {
			message = i18n.T(c, "app_exit_description")
		}
		if kit.Confirm(c.Key("close-guard").Key(st), &st.open, kit.Confirmation{
			Title: i18n.T(c, "app_exit_confirm"), Message: message,
			Buttons: []string{i18n.T(c, "settings_cancel"), i18n.T(c, "app_exit")}, Default: 1, Destructive: []int{1},
		}) == 1 {
			a.finishClose(c, st)
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
			a.finishClose(c, st)
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

func (a *App) finishClose(c *ui.Context, st *closing) {
	if a.closing != st {
		return
	}
	documents := st.Documents()
	for _, document := range documents {
		if !st.shown[document] {
			a.closing = newClosePrompt(st.CloseRequest, documents)
			c.Invalidate()
			return
		}
	}
	st.open = false
	a.closing = nil
	st.Proceed()
}
