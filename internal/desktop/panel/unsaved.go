package panel

import (
	"slices"

	"github.com/egoist/mygo/ui"
)

// Unsaved lists the open files whose draft differs from the saved text, by
// path. The window's close and quit prompt names them.
func (pr *Project) Unsaved() []string {
	var paths []string
	if pr.Actions == nil {
		return nil
	}
	for path, item := range pr.Files.items {
		pr.syncDocument(path, item)
		if item.dirty {
			paths = append(paths, path)
		}
	}
	slices.Sort(paths)
	return paths
}

// Saving reports whether a save of an open file is in flight.
func (pr *Project) Saving() bool {
	for _, item := range pr.Files.items {
		if item.document != nil && item.document.saving {
			return true
		}
	}
	return false
}

// PendingEdits reports document mutations queued for the source editor's next
// frame. Installation must not lock the editor before those edits are retained.
func (pr *Project) PendingEdits() bool {
	for _, item := range pr.Files.items {
		if state := item.document; state != nil && state.pending != nil && state.pendingEdit {
			return true
		}
	}
	return false
}

// SaveUnsaved starts saving every unsaved draft. It reports false when a
// draft cannot be saved at all (read-only, partially loaded or without a
// save action); that draft stays as it is.
func (pr *Project) SaveUnsaved(c *ui.Context) bool {
	saved := true
	for _, path := range pr.Unsaved() {
		item := pr.Files.items[path]
		state := item.documentState()
		if state.saving {
			continue
		}
		if !pr.fileWritable(item) || !item.loaded || state.partial || pr.Actions.SaveFile == nil {
			saved = false
			continue
		}
		pr.Actions.SaveFile(c, path, item.draft+item.suffix)
	}
	return saved
}
