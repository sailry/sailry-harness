package live

import (
	"fmt"
	"maps"
	"slices"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// Settings › OCR, Knowledge and Decision: the selected host's local models
// and which of them each kind uses, read when one of the pages shows,
// followed through the Node's events and saved whole at the revision read.
// Dictation's speech models are the controller's, not a Node's
// (settings_models_speech.go).

// localModelKinds are the pages' kinds by the model library's kind keys.
var localModelKinds = map[string]protocol.ModelsKind{
	settings.ModelsEmbedding: protocol.ModelsKindEmbedding,
	settings.ModelsOCR:       protocol.ModelsKindOcr,
	settings.ModelsSpeech:    protocol.ModelsKindSpeech,
	settings.ModelsDecision:  protocol.ModelsKindDecision,
}

// localModelState is a Node's local models as the pages show them: the
// settings saved, the choices a save has not confirmed (drafts), the
// registry's models and what the Node can run. shown, visible and library
// are the local runtime's: the Node the pages hold, whether one of them
// showed last frame, and the model library dialog.
type localModelState struct {
	settings     protocol.ModelsLocalSettings
	drafts       map[protocol.ModelsKind]protocol.ModelsLocalChoice
	models       []protocol.ModelsLocalModel
	availability map[protocol.ModelsKind]protocol.ModelsCatalogAvailability
	// requests are the installs whose outcome is unknown, by model ID, and
	// installing the ones under way.
	requests   map[string]protocol.Request
	installing map[string]bool
	// updating are the models whose newer version is being looked for.
	updating  map[string]bool
	known     bool
	reading   bool
	failed    bool
	stale     bool
	saving    bool
	watching  bool
	unwatched bool // the Node's events could not be followed

	shown   *Runtime
	visible bool
	library modelLibraryState
	// knowledge is whether the Knowledge page showed last frame.
	knowledge bool
}

// localModelsFrame keeps the local-model pages on the selected host's
// Node: it reads the Node's settings and models when they are unknown, a
// save left them in doubt or an event reported a change, follows the
// Node's events from the first visit on, and shows the result. Nothing is
// read while another page shows.
func (r *Runtime) localModelsFrame() {
	root := r.localRuntime()
	state := &root.localModels
	m := root.App.Settings
	settingsPage := root.App.Page == shell.SettingsPage
	if settingsPage && m.Section == settings.Dictation && root.Preferences != nil {
		if m.ModelActions == nil {
			m.ModelActions = root.modelActions()
		}
		m.Speech, m.SpeechModel = speechModels(root.Preferences.Data.DictationModel(), root.speechManager())
		// The library searches and resolves through the selected host's
		// Node; what it installs and removes is the controller's.
		owner := root.runtimeFor(root.App.Host)
		if owner != state.shown {
			m.CloseModelDialogs()
		}
		state.shown = owner
		root.libraryFrame(owner)
	}
	visible := settingsPage && (m.Section == settings.OCR || m.Section == settings.Knowledge || m.Section == settings.Decision)
	if !visible {
		state.visible, state.knowledge = false, false
		return
	}
	owner := root.runtimeFor(root.App.Host)
	entered := !state.visible || owner != state.shown
	if owner != state.shown {
		// The library and the details belong to the Node they were opened on.
		m.CloseModelDialogs()
	}
	state.shown, state.visible = owner, true
	if m.ModelActions == nil {
		m.ModelActions = root.modelActions()
	}
	if owner == nil {
		projectLocalModels(m.Model, &localModelState{}, nil)
		m.Collections = nil
		root.libraryFrame(nil)
		return
	}
	s := &owner.localModels
	if entered {
		s.failed, s.unwatched = false, false
	}
	if !s.watching && !s.unwatched {
		owner.watchLocalModels()
	}
	if !s.reading && !s.saving && (!s.known || s.stale) && !s.failed {
		owner.readLocalModels()
	}
	var tasks []protocol.InstallationTask
	if owner.packages != nil {
		tasks = owner.packages.tasks
	}
	projectLocalModels(m.Model, s, tasks)
	if m.Section == settings.Knowledge {
		owner.knowledgeFrame(m.Model, entered || !state.knowledge)
	}
	state.knowledge = m.Section == settings.Knowledge
	root.libraryFrame(owner)
}

// modelActions are the pages' actions on the Node shown.
func (r *Runtime) modelActions() *settings.ModelActions {
	on := func(act func(owner *Runtime)) {
		if owner := r.localModels.shown; owner != nil {
			act(owner)
		}
	}
	return &settings.ModelActions{
		Save: func(key string) { on(func(owner *Runtime) { owner.saveLocalModelPage(key) }) },
		Select: func(key, id string) {
			if key == settings.ModelsSpeech {
				r.selectSpeechModel(id)
				return
			}
			on(func(owner *Runtime) { owner.selectLocalModel(key, id) })
		},
		Install: func(key, id string) {
			if key == settings.ModelsSpeech {
				r.installSpeechModel(id)
				return
			}
			on(func(owner *Runtime) {
				if kind, known := localModelKinds[key]; known {
					owner.installLocalModel(kind, id)
				}
			})
		},
		Cancel: func(key, id string) {
			if key == settings.ModelsSpeech {
				r.cancelSpeechModel(id)
				return
			}
			on(func(owner *Runtime) { owner.cancelLocalModel(id) })
		},
		Update: func(key, id string) {
			if key == settings.ModelsSpeech {
				r.updateSpeechModel(id)
				return
			}
			on(func(owner *Runtime) { owner.updateLocalModel(id) })
		},
		Delete: func(key, id string) {
			if key == settings.ModelsSpeech {
				r.deleteSpeechModel(id)
				return
			}
			on(func(owner *Runtime) { owner.deleteLocalModel(id) })
		},
		Microphone:     r.requestMicrophone,
		LibraryInstall: r.installLibraryModel,
		LibraryCancel:  r.cancelLibraryModel,
		LibraryDelete:  r.deleteLibraryModel,
		LibraryDetails: r.readLibraryModel,
		LibraryVersion: r.showLibraryVersion,
		LibraryMore:    r.moreLibrary,
		CollectionPause: func(id string, paused bool) {
			on(func(owner *Runtime) { owner.pauseCollection(id, paused) })
		},
		CollectionUpdate: func(id string) { on(func(owner *Runtime) { owner.updateCollection(id) }) },
	}
}

// readLocalModels reads the Node's settings, then its models. A failure is
// a toast and is not tried again until a page is entered anew.
func (r *Runtime) readLocalModels() {
	s := &r.localModels
	if s.reading || r.ctx != nil && r.ctx.Err() != nil {
		return
	}
	s.reading, s.stale = true, false
	s.reading = r.async(func() {
		output, err := r.controller.Do(r.ctx, protocol.Command{Kind: "read_local_model_settings"})
		value, ok := detail[protocol.ModelsLocalSettings](output.Data)
		if err == nil && (output.Kind != "local_model_settings" || !ok) {
			err = fmt.Errorf("unexpected local model settings response")
		}
		var catalog protocol.ModelsCatalog
		if err == nil {
			output, err = r.controller.Do(r.ctx, protocol.Command{Kind: "read_local_models", Data: protocol.CommandReadLocalModelsPayload{}})
			catalog, ok = detail[protocol.ModelsCatalog](output.Data)
			if err == nil && (output.Kind != "local_models" || !ok) {
				err = fmt.Errorf("unexpected local models response")
			}
		}
		r.enqueue(func(c *ui.Context) {
			s.reading = false
			if err != nil {
				s.failed = true
				settings.Report(c, kit.Failed, settings.LocalModelMessages)
				return
			}
			s.models, s.availability = catalog.Models, catalog.Availability
			if s.saving {
				// The save under way answers with newer settings, or
				// leaves these in doubt.
				s.stale = true
				return
			}
			s.acceptRead(value)
		})
	})
}

// acceptRead takes a read's answer, unless it is older than the settings
// held: such a read was answered before a save whose result was accepted
// since, and would take the pages back to what that save replaced.
func (s *localModelState) acceptRead(value protocol.ModelsLocalSettings) {
	if s.known && value.Revision < s.settings.Revision {
		return
	}
	s.acceptSettings(value)
}

// acceptSettings takes the settings the Node holds. A choice that differs
// from them (its save failed, conflicted or is in doubt) stays the page's
// draft; one the Node now holds is no longer a draft.
func (s *localModelState) acceptSettings(value protocol.ModelsLocalSettings) {
	s.settings, s.known, s.failed = value, true, false
	for kind, draft := range s.drafts {
		if sameLocalChoice(draft, value.Choices[kind]) {
			delete(s.drafts, kind)
		}
	}
}

// registryChanged takes the registry an event carries. A read under way
// may have been answered before the change and arrive after it, so what it
// brings is read again.
func (s *localModelState) registryChanged(models []protocol.ModelsLocalModel) {
	s.models = models
	if s.reading {
		s.stale = true
	}
}

// watchLocalModels follows the Node's events: a changed registry replaces
// the models, changed settings, changed knowledge collections and a new
// snapshot mark what was read as stale, read again while a page shows. A stream that ends is not opened
// again until a page is entered anew.
func (r *Runtime) watchLocalModels() {
	s := &r.localModels
	lost := func() {
		r.enqueue(func(*ui.Context) { s.watching, s.unwatched, s.stale, r.knowledge.stale = false, true, true, true })
	}
	s.watching = r.observe(func() {
		stream, err := r.controller.Subscribe(r.ctx, protocol.Topic{Kind: "node"})
		if err != nil {
			lost()
			return
		}
		defer stream.Close()
		for first := true; ; first = false {
			update, err := stream.Next(r.ctx)
			if err != nil {
				lost()
				return
			}
			switch update.Kind {
			case "snapshot":
				// The subscription opens with the snapshot the frame's own
				// read already covers.
				if !first {
					r.enqueue(func(*ui.Context) { s.stale, r.knowledge.stale = true, true })
				}
			case "event":
				envelope, valid := detail[protocol.EventEnvelope](update.Data)
				if !valid || envelope.Node != r.controller.Target() {
					continue
				}
				switch envelope.Event.Kind {
				case "local_models_changed":
					if changed, valid := detail[protocol.EventLocalModelsChangedPayload](envelope.Event.Data); valid {
						r.enqueue(func(*ui.Context) { s.registryChanged(changed.Models) })
					}
				case "knowledge_collections_changed":
					// The event names no collection: the page reads them again.
					r.enqueue(func(*ui.Context) { r.knowledge.stale = true })
				case "local_model_settings_changed":
					if changed, valid := detail[protocol.EventLocalModelSettingsChangedPayload](envelope.Event.Data); valid {
						r.enqueue(func(*ui.Context) {
							if changed.Revision > s.settings.Revision {
								s.stale = true
							}
						})
					}
				}
			}
		}
	})
}

// saveLocalModelPage takes the switch and the picker as a page shows them
// for the kind's choice and saves it.
func (r *Runtime) saveLocalModelPage(key string) {
	kind, page := localModelPage(r.localRuntime().App.Settings.Model, key)
	if page == nil {
		return
	}
	s := &r.localModels
	r.saveLocalChoice(kind, pageLocalChoice(kind, *page, s.settings.Choices[kind]))
}

// selectLocalModel chooses an installed model for its kind. Choosing an
// embedding model turns embeddings on, as the picker's does.
func (r *Runtime) selectLocalModel(key, id string) {
	kind, known := localModelKinds[key]
	if !known || kind == protocol.ModelsKindSpeech {
		return
	}
	s := &r.localModels
	choice := s.choice(kind)
	choice.Model = &id
	if kind == protocol.ModelsKindEmbedding {
		choice.Enabled = true
	}
	r.saveLocalChoice(kind, choice)
}

// saveLocalChoice saves the settings with the kind's choice at the
// revision read. Failure, conflict and an unknown outcome are the
// runtime_* toasts and keep the choice as the page's draft; the settings
// are then read again, so that the next change saves at the Node's
// revision. A request whose outcome is unknown is retained and sent again
// by the same draft.
func (r *Runtime) saveLocalChoice(kind protocol.ModelsKind, choice protocol.ModelsLocalChoice) {
	s := &r.localModels
	if s.saving || !s.known {
		return
	}
	if s.drafts == nil {
		s.drafts = map[protocol.ModelsKind]protocol.ModelsLocalChoice{}
	}
	if sameLocalChoice(choice, s.settings.Choices[kind]) {
		delete(s.drafts, kind)
	} else {
		s.drafts[kind] = choice
	}
	if len(s.drafts) == 0 {
		return
	}
	s.saving = true
	request := localModelRequest(s.settings, s.drafts)
	r.saveSetting(protocol.Command{Kind: "save_local_model_settings", Data: request}, settings.LocalModelMessages, func(output protocol.Output, ok bool) {
		s.saving = false
		value, valid := detail[protocol.ModelsLocalSettings](output.Data)
		if !ok || !valid || output.Kind != "local_model_settings" {
			s.stale = true
			return
		}
		s.drafts = nil
		s.acceptSettings(value)
	})
}

// choice is the kind's choice as the page shows it: the draft, or what the
// Node saved.
func (s *localModelState) choice(kind protocol.ModelsKind) protocol.ModelsLocalChoice {
	if draft, drafted := s.drafts[kind]; drafted {
		return draft
	}
	return s.settings.Choices[kind]
}

// localModelRequest is the settings with the drafts, at the revision read.
func localModelRequest(saved protocol.ModelsLocalSettings, drafts map[protocol.ModelsKind]protocol.ModelsLocalChoice) protocol.ModelsLocalSettings {
	request := saved
	request.Choices = maps.Clone(saved.Choices)
	if request.Choices == nil {
		request.Choices = map[protocol.ModelsKind]protocol.ModelsLocalChoice{}
	}
	maps.Copy(request.Choices, drafts)
	return request
}

func sameLocalChoice(a, b protocol.ModelsLocalChoice) bool {
	return a.Enabled == b.Enabled && (a.Model == nil) == (b.Model == nil) && (a.Model == nil || *a.Model == *b.Model)
}

// localModelPage is the page of a Node kind's key.
func localModelPage(m *settings.Model, key string) (protocol.ModelsKind, *settings.LocalModels) {
	switch key {
	case settings.ModelsOCR:
		return protocol.ModelsKindOcr, &m.OCR
	case settings.ModelsEmbedding:
		return protocol.ModelsKindEmbedding, &m.Knowledge
	case settings.ModelsDecision:
		return protocol.ModelsKindDecision, &m.Decision
	}
	return "", nil
}

// pageLocalChoice is the choice a page's switch and picker make: the
// picker's installed model, none, or the saved model the Node no longer
// has installed (the picker's last choice). Embeddings have no switch:
// they are on when a model is chosen.
func pageLocalChoice(kind protocol.ModelsKind, page settings.LocalModels, saved protocol.ModelsLocalChoice) protocol.ModelsLocalChoice {
	choice := protocol.ModelsLocalChoice{Enabled: page.Enabled.Draft}
	if installed := page.Installed(); page.Model >= 0 && page.Model < len(installed) {
		choice.Model = &installed[page.Model].ID
	} else if page.Model >= len(installed) {
		choice.Model = saved.Model
	}
	if kind == protocol.ModelsKindEmbedding {
		choice.Enabled = choice.Model != nil
	}
	return choice
}

// projectLocalModels shows a Node's models and choices on the three pages,
// keeping the pages' tabs and searches.
func projectLocalModels(m *settings.Model, s *localModelState, tasks []protocol.InstallationTask) {
	for _, key := range []string{settings.ModelsOCR, settings.ModelsEmbedding, settings.ModelsDecision} {
		kind, page := localModelPage(m, key)
		availability, reported := s.availability[kind]
		page.Kind = key
		page.Models = localModelViews(s.models, kind, tasks)
		page.Catalog = slices.DeleteFunc(slices.Clone(page.Models), func(x viewmodel.LocalModel) bool { return x.Status == viewmodel.ModelInstalled })
		page.Unavailable = reported && availability.Kind == "unavailable"
		saved, choice := s.settings.Choices[kind], s.choice(kind)
		page.Enabled = settings.Stored[bool]{Saved: saved.Enabled, Draft: choice.Enabled, Revision: s.settings.Revision,
			Pending: !s.known || s.saving || !reported || availability.Kind != "available"}
		selected := choice.Model
		if kind == protocol.ModelsKindEmbedding {
			// Keyword only is the picker's first choice, not a switch.
			page.Enabled.Saved, page.Enabled.Draft = true, true
			if !choice.Enabled {
				selected = nil
			}
		}
		page.Model, page.Missing = chosenLocalModel(page.Installed(), s.models, selected)
	}
}

// chosenLocalModel is the picker's choice among the installed models: -1
// for none, and one past them with its name for a chosen model the Node no
// longer has installed.
func chosenLocalModel(installed []viewmodel.LocalModel, models []protocol.ModelsLocalModel, selected *string) (int, string) {
	if selected == nil {
		return -1, ""
	}
	if index := slices.IndexFunc(installed, func(x viewmodel.LocalModel) bool { return x.ID == *selected }); index >= 0 {
		return index, ""
	}
	for _, model := range models {
		if model.ID == *selected && model.Definition.Name != "" {
			return len(installed), model.Definition.Name
		}
	}
	return len(installed), *selected
}

// localModelViews are the registry's models of a kind as the pages' rows.
// Models that share a name show their repository too, so that the picker
// tells them apart.
func localModelViews(models []protocol.ModelsLocalModel, kind protocol.ModelsKind, tasks []protocol.InstallationTask) []viewmodel.LocalModel {
	var views []viewmodel.LocalModel
	names := map[string]int{}
	for _, model := range models {
		if modelFormatKind(model.Definition.Format) != kind {
			continue
		}
		view := viewmodel.LocalModel{ID: model.ID, Name: model.Definition.Name, Repo: model.Definition.Source.Repository,
			Version: model.Definition.Source.Revision, License: model.Definition.License, Size: modelSize(model.Definition.Assets)}
		if view.Name == "" {
			view.Name = model.ID
		}
		view.Status, view.Percent = localModelStatus(model, kind, tasks)
		// What is installed can be deleted, and updated from its source.
		view.Removable = view.Status == viewmodel.ModelInstalled
		names[view.Name]++
		views = append(views, view)
	}
	for i := range views {
		if names[views[i].Name] > 1 {
			views[i].Name += " (" + views[i].Repo + " " + views[i].ID + ")"
		}
	}
	return views
}

// localModelStatus is a model's state, with its download's percent from
// the Node's installations.
func localModelStatus(model protocol.ModelsLocalModel, kind protocol.ModelsKind, tasks []protocol.InstallationTask) (viewmodel.ModelStatus, float64) {
	switch model.State.Kind {
	case "installed":
		return viewmodel.ModelInstalled, 0
	case "downloading":
		return viewmodel.ModelDownloading, modelProgress(tasks, kind, model.ID)
	}
	return viewmodel.ModelMissing, 0
}

// modelProgress is the percent of the model's running installation, 0
// while the Node reports none.
func modelProgress(tasks []protocol.InstallationTask, kind protocol.ModelsKind, id string) float64 {
	for _, task := range tasks {
		resource, valid := detail[protocol.InstallationResourceModelPayload](task.Resource.Data)
		if !valid || task.Resource.Kind != "model" || resource.Kind != kind || resource.ID != id {
			continue
		}
		running, valid := detail[protocol.InstallationStateRunningPayload](task.State.Data)
		if !valid || task.State.Kind != "running" || running.Progress == nil {
			continue
		}
		switch running.Progress.Kind {
		case "percent":
			if progress, valid := detail[protocol.InstallationProgressPercentPayload](running.Progress.Data); valid {
				return float64(progress.Percent)
			}
		case "counts":
			if progress, valid := detail[protocol.InstallationProgressCountsPayload](running.Progress.Data); valid && progress.Total > 0 {
				return float64(progress.Completed) / float64(progress.Total) * 100
			}
		}
	}
	return 0
}

// modelFormatKind is the kind of model a format runs as.
func modelFormatKind(format protocol.ModelsFormat) protocol.ModelsKind {
	switch format.Kind {
	case "transformer", "static":
		return protocol.ModelsKindEmbedding
	case "sense_voice":
		return protocol.ModelsKindSpeech
	case "paddle_ocr":
		return protocol.ModelsKindOcr
	case "decision":
		return protocol.ModelsKindDecision
	}
	return ""
}
