package live

import (
	"os"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
)

// System shortcuts: the registration conflict toast and the selection
// shortcut, which reads the front app's selected text before the window
// comes forward.

// boundedSelection is a captured text, or the message key that refuses it.
func boundedSelection(text string) (string, string) {
	switch {
	case len(text) > 65536 || strings.Contains(text, "\x00"):
		return "", "selection_large"
	case strings.TrimSpace(text) == "":
		return "", "selection_empty"
	}
	return text, ""
}

// ReportShortcutConflict tells that the system refused a system shortcut.
func (r *Runtime) ReportShortcutConflict() {
	r.enqueue(func(c *ui.Context) { kit.Toast(c, kit.Failed, i18n.T(c, "shortcut_conflict")) })
}

// SetLogFolder gives Help › Logs the folder of the process's log. Without
// one the item stays disabled.
func (r *Runtime) SetLogFolder(path string) {
	if path == "" {
		return
	}
	r.App.Hooks.OpenLogs = func() {
		if _, err := os.Stat(path); err != nil || r.attachments.native.OpenLocal == nil {
			r.enqueue(func(c *ui.Context) { kit.Toast(c, kit.Failed, i18n.T(c, "files_open_failed")) })
			return
		}
		r.attachments.native.OpenLocal(path)
	}
}

// selectionReader reads the front app's selected text, or names the message
// that says why there is none.
var selectionReader = readSelection

// CaptureSelection reads the selection of the app in front, then calls
// activate on the window's thread and shows the text or why there is none.
// The caller must not bring the window forward first.
func (r *Runtime) CaptureSelection(activate func()) {
	r.async(func() {
		text, key := selectionReader()
		r.enqueue(func(c *ui.Context) {
			if key != "" {
				kit.Toast(c, kit.Failed, i18n.T(c, key))
				return
			}
			r.App.ShowSelection(text)
		})
		if activate != nil {
			r.mu.Lock()
			update := r.update
			r.mu.Unlock()
			if update != nil {
				update(activate)
			}
		}
	})
}
