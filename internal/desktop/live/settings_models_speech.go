package live

import (
	"errors"
	"slices"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/preferences"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
	"github.com/sailry/sailry-harness/internal/speech/capture"
	"github.com/sailry/sailry-harness/internal/speech/models"
)

// Settings › Dictation. Its speech models are the controller's: the
// selected one is the dictation.model preference, and no Node lists,
// registers, installs, cancels, deletes or saves a speech model (a Node
// refuses the speech kind). The controller's manager (internal/speech/
// models) owns the recommended model, the registered ones and their
// downloads under the desktop data directory. The microphone's access and
// the input devices are this machine's too.

// dictationPage is what the page last showed.
type dictationPage struct {
	visible    bool
	permission capture.Permission
	asking     bool // a permission request is with the system
	devices    []capture.Device
	listing    bool
	microphone int // the picker's choice as projected
	checking   bool
	statuses   map[string]models.Status // by row ID, to toast a download's end
}

// speechModels are the page's speech models and the selected one's index,
// -1 when the selected model is not listed. The recommended model has no
// ID and comes first; it is selected while the preference names none.
// Without a manager it is the only row, not installed.
func speechModels(selected *protocol.ModelID, manager *models.Manager) ([]viewmodel.LocalModel, int) {
	rows := []viewmodel.LocalModel{{Status: viewmodel.ModelMissing}}
	if manager != nil {
		if state, known := manager.RecommendedState(); known {
			rows[0].Status, rows[0].Percent = speechStatus(state)
		}
		for _, entry := range manager.Entries() {
			definition := entry.Record.Definition
			// A registered model can be removed; the recommended one cannot.
			row := viewmodel.LocalModel{ID: entry.Record.ID.String(), Name: definition.Name, Repo: definition.Source.Repository,
				Version: definition.Source.Revision, License: definition.License, Size: modelSize(definition.Assets), Removable: true}
			row.Status, row.Percent = speechStatus(entry.State)
			rows = append(rows, row)
		}
	}
	if selected == nil {
		return rows, 0
	}
	id := selected.String()
	return rows, slices.IndexFunc(rows, func(row viewmodel.LocalModel) bool { return row.ID == id })
}

// speechStatus is a row's state: a failed download can be started again.
func speechStatus(state models.State) (viewmodel.ModelStatus, float64) {
	switch state.Status {
	case models.Downloading:
		return viewmodel.ModelDownloading, float64(state.Progress.Percent)
	case models.Installed:
		return viewmodel.ModelInstalled, 0
	}
	return viewmodel.ModelMissing, 0
}

// speechSelection is the dictation.model preference for a row: none for
// the recommended model, and a registered model's ID otherwise.
func speechSelection(id string) (*protocol.ModelID, bool) {
	if id == "" {
		return nil, true
	}
	model, err := protocol.ParseModelID(id)
	if err != nil {
		return nil, false
	}
	return &model, true
}

// dictationPreferences are the stored dictation choices, created on the
// first one that differs from the defaults.
func (r *Runtime) dictationPreferences() *preferences.Dictation {
	d := &r.localRuntime().Preferences.Data
	if d.Dictation == nil {
		d.Dictation = &preferences.Dictation{Language: preferences.DictationLanguages[0]}
	}
	return d.Dictation
}

// selectSpeechModel stores the selected speech model in the desktop
// preferences, which the frame then saves. Only an installed model is
// selected.
func (r *Runtime) selectSpeechModel(id string) {
	root := r.localRuntime()
	selected, valid := speechSelection(id)
	rows, _ := speechModels(nil, root.speechManager())
	index := slices.IndexFunc(rows, func(row viewmodel.LocalModel) bool { return row.ID == id })
	if !valid || index < 0 || rows[index].Status != viewmodel.ModelInstalled {
		root.enqueue(func(c *ui.Context) { kit.Message(c, i18n.T(c, "dictation_model_missing")) })
		return
	}
	if selected == nil && root.Preferences.Data.Dictation == nil {
		return
	}
	root.dictationPreferences().Model = selected
}

// installSpeechModel downloads a speech model on the controller.
func (r *Runtime) installSpeechModel(id string) {
	root := r.localRuntime()
	model, valid := speechSelection(id)
	manager := root.speechManager()
	if !valid || manager == nil {
		root.enqueue(func(c *ui.Context) { root.dictationNotice(c, "dictation_storage") })
		return
	}
	if err := manager.Start(model, nil); err != nil {
		key := dictationKey(err)
		root.enqueue(func(c *ui.Context) { kit.Toast(c, kit.Failed, i18n.T(c, key)) })
	}
}

// cancelSpeechModel stops a speech model's download.
func (r *Runtime) cancelSpeechModel(id string) {
	if model, valid := speechSelection(id); valid {
		if manager := r.localRuntime().speechManager(); manager != nil {
			manager.Cancel(model)
		}
	}
}

// speechHeld is the controller's registered model of a hub result: the
// same hub and repository, and the same revision where the result names a
// commit. Of several, an installed one comes before a downloading one, and
// that before a missing one.
func speechHeld(entries []models.Entry, item protocol.ModelsItem) (libraryHolding, bool) {
	var best libraryHolding
	found := false
	for _, entry := range entries {
		source := entry.Record.Definition.Source
		if source.Hub != item.Source.Hub || source.Repository != item.Source.Repository ||
			commitRevision(item.Source.Revision) && source.Revision != item.Source.Revision {
			continue
		}
		held := libraryHolding{id: entry.Record.ID.String(), definition: entry.Record.Definition}
		held.status, held.percent = speechStatus(entry.State)
		if !found || held.status > best.status {
			best, found = held, true
		}
	}
	return best, found
}

// addSpeechModel registers a definition a Node resolved with the
// controller and downloads it. The same source with other files is the
// model_library_conflict toast.
func (r *Runtime) addSpeechModel(definition protocol.ModelsDefinition) {
	root := r.localRuntime()
	fail := func(key string) {
		root.enqueue(func(c *ui.Context) { kit.Toast(c, kit.Failed, i18n.T(c, key)) })
	}
	manager := root.speechManager()
	if manager == nil {
		fail("dictation_storage")
		return
	}
	id, err := manager.Register(definition)
	if err == nil {
		err = manager.Start(&id, nil)
	}
	switch {
	case errors.Is(err, models.ErrConflict):
		fail("model_library_conflict")
	case err != nil:
		fail(dictationKey(err))
	}
}

// deleteSpeechModel removes a registered speech model and its files, after
// the confirmation. Dictation then uses the recommended model where it
// used this one.
func (r *Runtime) deleteSpeechModel(id string) {
	root := r.localRuntime()
	model, valid := speechSelection(id)
	manager := root.speechManager()
	if !valid || model == nil || manager == nil {
		return
	}
	if err := manager.Remove(*model); err != nil {
		key := dictationKey(err)
		root.enqueue(func(c *ui.Context) { kit.Toast(c, kit.Failed, i18n.T(c, key)) })
		return
	}
	if selected := root.Preferences.Data.DictationModel(); selected != nil && *selected == *model {
		root.dictationPreferences().Model = nil
	}
	root.enqueue(func(c *ui.Context) { kit.Toast(c, kit.Done, i18n.T(c, "knowledge_deleted")) })
}

// updateSpeechModel looks for a newer version of a registered speech model:
// the selected host's Node resolves its source at its head. A version that
// is installed is "Already up to date"; another is registered and
// downloaded.
func (r *Runtime) updateSpeechModel(id string) {
	root := r.localRuntime()
	manager := root.speechManager()
	owner := root.runtimeFor(root.App.Host)
	if manager == nil || owner == nil {
		return
	}
	entries := manager.Entries()
	index := slices.IndexFunc(entries, func(entry models.Entry) bool { return entry.Record.ID.String() == id })
	if index < 0 {
		return
	}
	source := entries[index].Record.Definition.Source
	source.Revision = "HEAD"
	owner.async(func() {
		definition, err := owner.resolveModel(protocol.ModelsKindSpeech, source)
		root.enqueue(func(c *ui.Context) {
			if err != nil {
				owner.reportKeys(c, err, "runtime_failed", "runtime_conflict", "runtime_unknown")
				return
			}
			if slices.ContainsFunc(manager.Entries(), func(entry models.Entry) bool {
				return entry.State.Status == models.Installed && entry.Record.Definition.Source == definition.Source
			}) {
				kit.Message(c, i18n.T(c, "knowledge_up_to_date"))
				return
			}
			root.addSpeechModel(definition)
		})
	})
}

// requestMicrophone asks the system for the microphone. The system shows
// its prompt once; after a refusal only its settings change the answer.
func (r *Runtime) requestMicrophone() {
	root := r.localRuntime()
	page := &root.dictation.page
	if page.asking {
		return
	}
	page.asking = true
	if !root.async(func() {
		granted, err := capture.Request(root.ctx)
		status := capture.Status()
		root.enqueue(func(c *ui.Context) {
			page.asking, page.permission = false, status
			if err != nil || !granted {
				kit.Toast(c, kit.Failed, i18n.T(c, "dictation_permission"))
			}
		})
	}) {
		page.asking = false
	}
}

// dictationFrame keeps Settings › Dictation current while it shows: the
// microphone's access, the input devices with the stored choice, and the
// toasts of the speech downloads that ended. Entering the page reads the
// access again, lists the devices and verifies the selected model's files.
func (r *Runtime) dictationFrame(c *ui.Context) {
	root := r.localRuntime()
	state := &root.dictation
	page := &state.page
	m := root.App.Settings
	root.speechToasts(c)
	if root.App.Page != shell.SettingsPage || m.Section != settings.Dictation || root.Preferences == nil {
		page.visible = false
		return
	}
	if !page.visible {
		page.visible = true
		root.listMicrophones()
		root.checkSpeechModel()
	} else if m.Microphone != page.microphone {
		// The picker moved: its first choice is the system default, and
		// the entry of a stored device that is gone keeps that device.
		switch index := m.Microphone - 1; {
		case index < 0:
			if root.Preferences.Data.Dictation != nil {
				root.dictationPreferences().Microphone = nil
			}
		case index < len(page.devices):
			id := page.devices[index].ID
			root.dictationPreferences().Microphone = &id
		}
	}
	// The access is read again on every frame the page shows, so that a
	// change made in the system's settings shows on return.
	if !page.asking {
		page.permission = capture.Status()
	}
	m.MicrophoneGranted = page.permission.Allowed()
	// The stored device that is not listed stays the choice, named as
	// missing, or as loading until the devices are known.
	label := "dictation_device_missing"
	if page.listing {
		label = "dictation_device_loading"
	}
	m.Microphones, m.Microphone = microphoneChoices(page.devices, root.Preferences.Data.DictationMicrophone(), i18n.T(c, label))
	page.microphone = m.Microphone
}

// microphoneChoices are the picker's entries after the system default and
// the stored device's place among all of them. A stored device that is not
// listed is a last entry of its own, named absent.
func microphoneChoices(devices []capture.Device, stored *string, absent string) ([]string, int) {
	names := make([]string, 0, len(devices)+1)
	for _, device := range devices {
		names = append(names, device.Name)
	}
	if stored == nil {
		return names, 0
	}
	index := slices.IndexFunc(devices, func(device capture.Device) bool { return device.ID == *stored })
	if index < 0 {
		names, index = append(names, absent), len(names)
	}
	return names, index + 1
}

// listMicrophones reads the input devices off the UI thread. Without the
// capture library the picker offers the system default alone.
func (r *Runtime) listMicrophones() {
	page := &r.dictation.page
	if page.listing {
		return
	}
	page.listing = true
	if !r.async(func() {
		var devices []capture.Device
		if speechRuntime() == nil {
			devices, _ = capture.Devices()
		}
		r.enqueue(func(*ui.Context) { page.devices, page.listing = devices, false })
	}) {
		page.listing = false
	}
}

// checkSpeechModel verifies the recommended model's files against their
// pinned hashes, so that the page shows what a dictation would find. A
// registered model's state is the index's.
func (r *Runtime) checkSpeechModel() {
	page := &r.dictation.page
	manager := r.speechManager()
	if manager == nil || page.checking {
		return
	}
	lease, err := manager.Capture(nil)
	if err != nil {
		return
	}
	page.checking = true
	if !r.async(func() {
		ready := lease.Ready()
		lease.Release()
		manager.Observe(nil, ready)
		r.enqueue(func(*ui.Context) { page.checking = false })
	}) {
		lease.Release()
		page.checking = false
	}
}

// speechToasts reports the downloads that ended since the last frame:
// installed, or the failure. A cancelled download says nothing.
func (r *Runtime) speechToasts(c *ui.Context) {
	state := &r.dictation
	if state.manager == nil {
		return
	}
	current := map[string]models.State{}
	if recommended, known := state.manager.RecommendedState(); known {
		current[""] = recommended
	}
	for _, entry := range state.manager.Entries() {
		current[entry.Record.ID.String()] = entry.State
	}
	page := &state.page
	for id, now := range current {
		if page.statuses[id] != models.Downloading || now.Status == models.Downloading {
			continue
		}
		switch now.Status {
		case models.Installed:
			kit.Toast(c, kit.Done, i18n.T(c, "knowledge_installed"))
		case models.Failed:
			kit.Toast(c, kit.Failed, i18n.T(c, dictationKey(now.Err)))
		}
	}
	page.statuses = map[string]models.Status{}
	for id, now := range current {
		page.statuses[id] = now.Status
	}
}
