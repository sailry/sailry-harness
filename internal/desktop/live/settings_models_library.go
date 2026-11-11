package live

import (
	"context"
	"fmt"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// The model library dialog: a hub search for the kind of the page that
// opened it, run by the Node shown, with each result's state from that
// Node's registry.

// modelLibraryState is the local runtime's library dialog: the request
// searched and the Node that searched it, the hub's results, and the
// definitions resolved for the details dialog, by row ID.
type modelLibraryState struct {
	request    settings.LibraryRequest
	owner      *Runtime
	items      []protocol.ModelsItem
	details    map[string]protocol.ModelsDefinition
	resolving  map[string]bool
	generation uint64
	cancel     context.CancelFunc
}

// libraryFrame searches when the dialog opens, when its hub changes and
// when the user searches, and shows the results. Closing the dialog drops
// them and abandons a search under way.
func (r *Runtime) libraryFrame(owner *Runtime) {
	lib := &r.localModels.library
	m := r.App.Settings
	request := m.LibraryRequest()
	if !request.Open || owner == nil {
		if lib.owner != nil {
			if lib.cancel != nil {
				lib.cancel()
			}
			*lib = modelLibraryState{generation: lib.generation + 1}
		}
		return
	}
	if request != lib.request || owner != lib.owner {
		r.searchLibrary(owner, request)
	}
	var tasks []protocol.InstallationTask
	if owner.packages != nil {
		tasks = owner.packages.tasks
	}
	m.Library = libraryRows(lib.items, owner.localModels.models, lib.details, tasks)
}

// searchLibrary reads the hub's first page for the request. An empty page
// that has a successor is followed (followLibrary), so that a page whose
// results the Node filtered out does not read as "no models". A failure is
// the model_library_failed toast; a hub that answered in part is said so.
func (r *Runtime) searchLibrary(owner *Runtime, request settings.LibraryRequest) {
	lib := &r.localModels.library
	if lib.cancel != nil {
		lib.cancel()
	}
	lib.generation++
	generation := lib.generation
	lib.request, lib.owner, lib.items = request, owner, nil
	lib.details, lib.resolving = map[string]protocol.ModelsDefinition{}, map[string]bool{}
	m := r.App.Settings
	m.LibraryLoad.Loading, m.LibraryLoad.Loaded = true, false
	finish := func(page protocol.ModelsPage, err error) {
		r.enqueue(func(c *ui.Context) {
			if lib.generation != generation {
				return
			}
			lib.items = uniqueLibraryItems(page.Items)
			m.LibraryLoad.Loading, m.LibraryLoad.Loaded = false, true
			if err != nil {
				kit.Toast(c, kit.Failed, i18n.T(c, "model_library_failed"))
			} else if page.Partial {
				kit.Message(c, i18n.T(c, "model_library_partial"))
			}
		})
	}
	kind, known := localModelKinds[request.Kind]
	if !known {
		finish(protocol.ModelsPage{}, fmt.Errorf("unknown model kind %q", request.Kind))
		return
	}
	ctx, cancel := context.WithCancel(owner.ctx)
	lib.cancel = cancel
	search := protocol.ModelsSearch{Hub: libraryHub(request.Hub), Kind: kind, Query: request.Query}
	started := owner.async(func() {
		finish(followLibrary(search, func(search protocol.ModelsSearch) (protocol.ModelsPage, error) {
			output, err := owner.controller.Do(ctx, protocol.Command{Kind: "search_models", Data: search})
			page, valid := detail[protocol.ModelsPage](output.Data)
			if err == nil && (output.Kind != "model_search" || !valid) {
				err = fmt.Errorf("unexpected model search response")
			}
			return page, err
		}))
	})
	if !started {
		m.LibraryLoad.Loading, m.LibraryLoad.Loaded = false, true
	}
}

// libraryAnswers bounds the answers one search follows. A Node skips up to
// three hub pages without a compatible model per answer, so a query that
// matches only other kinds of model stops after nine hub pages instead of
// reading the hub's whole listing while the dialog waits.
const libraryAnswers = 3

// followLibrary reads the first answer with results. An answer without any
// that names a successor is followed, libraryAnswers at most; results that
// may lie beyond that are reported as a partial page.
func followLibrary(search protocol.ModelsSearch, read func(protocol.ModelsSearch) (protocol.ModelsPage, error)) (protocol.ModelsPage, error) {
	var page protocol.ModelsPage
	for range libraryAnswers {
		next, err := read(search)
		if err != nil {
			return page, err
		}
		page.Items, page.Partial = append(page.Items, next.Items...), page.Partial || next.Partial
		if len(page.Items) > 0 || next.NextCursor == nil {
			return page, nil
		}
		search.Cursor = next.NextCursor
	}
	page.Partial = true
	return page, nil
}

// libraryItem is the result of a row and the Node that found it.
func (r *Runtime) libraryItem(id string) (*Runtime, protocol.ModelsItem, bool) {
	lib := &r.localModels.library
	for _, item := range lib.items {
		if libraryID(item.Source) == id && lib.owner != nil {
			return lib.owner, item, true
		}
	}
	return nil, protocol.ModelsItem{}, false
}

// installLibraryModel downloads a result: one the Node has registered is
// installed by its ID; another is resolved on its hub, registered and then
// installed. The hub must answer with the model that was asked for.
func (r *Runtime) installLibraryModel(id string) {
	owner, item, found := r.libraryItem(id)
	if !found {
		return
	}
	lib := &r.localModels.library
	kind := modelFormatKind(item.Format)
	if kind == protocol.ModelsKindSpeech {
		// A speech model is registered with the controller, never a Node.
		r.speechUnavailable()
		return
	}
	if model, registered := registeredModel(owner.localModels.models, item); registered {
		owner.installLocalModel(kind, model.ID)
		return
	}
	if lib.resolving[id] {
		return
	}
	lib.resolving[id] = true
	generation := lib.generation
	started := owner.async(func() {
		definition, err := owner.resolveModel(kind, item.Source)
		var registered protocol.OutputLocalModelPayload
		if err == nil {
			var output protocol.Output
			output, err = owner.controller.Do(owner.ctx, protocol.Command{Kind: "register_local_model", Data: protocol.CommandRegisterLocalModelPayload{Definition: definition}})
			var valid bool
			registered, valid = detail[protocol.OutputLocalModelPayload](output.Data)
			if err == nil && (output.Kind != "local_model" || !valid) {
				err = fmt.Errorf("unexpected model registration response")
			}
		}
		r.enqueue(func(c *ui.Context) {
			if lib.generation == generation {
				delete(lib.resolving, id)
			}
			if err != nil {
				owner.reportKeys(c, err, "runtime_failed", "runtime_conflict", "runtime_unknown")
				return
			}
			owner.localModels.stale = true
			owner.installLocalModel(kind, registered.ID)
		})
	})
	if !started {
		delete(lib.resolving, id)
	}
}

// resolveModel asks the Node for a hub model's definition and checks that
// it is the model and the kind asked for.
func (r *Runtime) resolveModel(kind protocol.ModelsKind, source protocol.ModelsSource) (protocol.ModelsDefinition, error) {
	output, err := r.controller.Do(r.ctx, protocol.Command{Kind: "resolve_model", Data: protocol.CommandResolveModelPayload{Kind: kind, Source: source}})
	if err != nil {
		return protocol.ModelsDefinition{}, err
	}
	definition, valid := detail[protocol.ModelsDefinition](output.Data)
	if output.Kind != "model_definition" || !valid || modelFormatKind(definition.Format) != kind ||
		definition.Source.Hub != source.Hub || definition.Source.Repository != source.Repository {
		return protocol.ModelsDefinition{}, fmt.Errorf("unexpected model definition response")
	}
	return definition, nil
}

func (r *Runtime) cancelLibraryModel(id string) {
	if owner, item, found := r.libraryItem(id); found {
		if model, registered := registeredModel(owner.localModels.models, item); registered {
			owner.cancelLocalModel(model.ID)
		}
	}
}

// deleteLibraryModel removes an installed result's files, after the
// dialog's confirmation. Its success is the "Deleted" toast.
func (r *Runtime) deleteLibraryModel(id string) {
	owner, item, found := r.libraryItem(id)
	if !found {
		return
	}
	model, registered := registeredModel(owner.localModels.models, item)
	if !registered {
		return
	}
	owner.executeScoped("local-models", protocol.Command{Kind: "delete_local_model", Data: protocol.CommandDeleteLocalModelPayload{ID: model.ID}}, func(c *ui.Context, _ protocol.Output, err error) {
		owner.localModels.stale = true
		if err != nil {
			owner.reportKeys(c, err, "runtime_failed", "runtime_conflict", "runtime_unknown")
			return
		}
		kit.Toast(c, kit.Done, i18n.T(c, "knowledge_deleted"))
	})
}

// readLibraryModel resolves a result the Node has not registered, for the
// details dialog's size and license: its newest version on the hub.
func (r *Runtime) readLibraryModel(id string) {
	owner, item, found := r.libraryItem(id)
	if !found {
		return
	}
	lib := &r.localModels.library
	if _, registered := registeredModel(owner.localModels.models, item); registered || lib.resolving[id] {
		return
	}
	if _, resolved := lib.details[id]; resolved {
		return
	}
	lib.resolving[id] = true
	generation := lib.generation
	started := owner.async(func() {
		source := item.Source
		output, err := owner.controller.Do(owner.ctx, protocol.Command{Kind: "read_model_versions", Data: protocol.CommandReadModelVersionsPayload{Hub: source.Hub, Repository: source.Repository}})
		versions, valid := detail[[]protocol.ModelsVersion](output.Data)
		if err == nil && (output.Kind != "model_versions" || !valid) {
			err = fmt.Errorf("unexpected model versions response")
		}
		var definition protocol.ModelsDefinition
		if err == nil {
			if len(versions) > 0 {
				source.Revision = versions[0].Revision
			}
			definition, err = owner.resolveModel(modelFormatKind(item.Format), source)
		}
		r.enqueue(func(c *ui.Context) {
			if lib.generation != generation {
				return
			}
			delete(lib.resolving, id)
			if err != nil {
				kit.Toast(c, kit.Failed, i18n.T(c, "model_library_failed"))
				return
			}
			lib.details[id] = definition
		})
	})
	if !started {
		delete(lib.resolving, id)
	}
}

// installLocalModel downloads a registered model. The request is the
// installation queue's to retry, and one whose outcome is unknown is sent
// again by the next install of the model. A download the user canceled
// says nothing more.
func (r *Runtime) installLocalModel(kind protocol.ModelsKind, id string) {
	s := &r.localModels
	if s.installing[id] {
		return
	}
	request, retained := s.requests[id]
	if !retained {
		request = r.controller.Prepare(protocol.Command{Kind: "install_local_model", Data: protocol.CommandInstallLocalModelPayload{ID: id}})
	}
	if r.packages != nil {
		r.packages.requests[request.ID.String()] = request
	}
	if s.installing == nil {
		s.installing, s.requests = map[string]bool{}, map[string]protocol.Request{}
	}
	s.installing[id] = true
	started := r.async(func() {
		_, err := r.controller.Execute(r.ctx, request)
		r.enqueue(func(c *ui.Context) {
			delete(s.installing, id)
			s.stale = true
			switch faultCode(err) {
			case protocol.ErrorCodeOutcomeUnknown:
				s.requests[id] = request
			case protocol.ErrorCodeCancelled:
				delete(s.requests, id)
				return
			default:
				delete(s.requests, id)
			}
			if err != nil {
				r.reportKeys(c, err, "runtime_failed", "runtime_conflict", "runtime_unknown")
				return
			}
			kit.Toast(c, kit.Done, i18n.T(c, "knowledge_saved"))
		})
	})
	if !started {
		delete(s.installing, id)
	}
}

// cancelLocalModel asks the Node to stop a model's download.
func (r *Runtime) cancelLocalModel(id string) {
	r.executeScoped("local-models", protocol.Command{Kind: "cancel_local_model", Data: protocol.CommandCancelLocalModelPayload{ID: id}}, func(c *ui.Context, _ protocol.Output, err error) {
		if err != nil {
			r.reportKeys(c, err, "runtime_failed", "runtime_conflict", "runtime_unknown")
			return
		}
		kit.Message(c, i18n.T(c, "knowledge_cancel_requested"))
	})
}

// libraryRows are the hub's results as the dialog's rows: each with the
// state of the Node's model from the same source, and the size and
// license of that model or of the definition resolved for its details.
func libraryRows(items []protocol.ModelsItem, models []protocol.ModelsLocalModel, details map[string]protocol.ModelsDefinition, tasks []protocol.InstallationTask) []viewmodel.LibraryModel {
	rows := make([]viewmodel.LibraryModel, 0, len(items))
	for _, item := range items {
		id := libraryID(item.Source)
		row := viewmodel.LibraryModel{ID: id, Name: item.Name, Repo: item.Source.Repository, Kind: libraryKindKey(modelFormatKind(item.Format)), Source: libraryHubKey(item.Source.Hub)}
		if row.Name == "" {
			row.Name = row.Repo
		}
		definition, resolved := details[id]
		if model, registered := registeredModel(models, item); registered {
			row.Status, row.Percent = localModelStatus(model, modelFormatKind(item.Format), tasks)
			definition, resolved = model.Definition, true
		}
		if resolved {
			row.Size, row.License = modelSize(definition.Assets), definition.License
		}
		rows = append(rows, row)
	}
	return rows
}

// registeredModel is the Node's model of a hub result: the same hub,
// repository and kind, and the same revision where the result names a
// commit. Of several, an installed one comes before a downloading one, and
// that before a missing one.
func registeredModel(models []protocol.ModelsLocalModel, item protocol.ModelsItem) (protocol.ModelsLocalModel, bool) {
	rank := func(model protocol.ModelsLocalModel) int {
		switch model.State.Kind {
		case "installed":
			return 3
		case "downloading":
			return 2
		}
		return 1
	}
	var best protocol.ModelsLocalModel
	found := 0
	for _, model := range models {
		source := model.Definition.Source
		if source.Hub != item.Source.Hub || source.Repository != item.Source.Repository || modelFormatKind(model.Definition.Format) != modelFormatKind(item.Format) {
			continue
		}
		if commitRevision(item.Source.Revision) && source.Revision != item.Source.Revision {
			continue
		}
		if rank(model) > found {
			best, found = model, rank(model)
		}
	}
	return best, found > 0
}

// commitRevision reports whether a revision names a commit (40 hex
// digits) rather than a branch or a tag.
func commitRevision(revision string) bool {
	if len(revision) != 40 {
		return false
	}
	for _, digit := range revision {
		if (digit < '0' || digit > '9') && (digit < 'a' || digit > 'f') {
			return false
		}
	}
	return true
}

// uniqueLibraryItems drops a source the hub listed twice, since a row is
// keyed by its source.
func uniqueLibraryItems(items []protocol.ModelsItem) []protocol.ModelsItem {
	seen := map[string]bool{}
	var unique []protocol.ModelsItem
	for _, item := range items {
		if id := libraryID(item.Source); !seen[id] {
			seen[id] = true
			unique = append(unique, item)
		}
	}
	return unique
}

// libraryID names a hub result's row.
func libraryID(source protocol.ModelsSource) string {
	return string(source.Hub) + "/" + source.Repository + "@" + source.Revision
}

// libraryHub is the hub of the dialog's picker.
func libraryHub(index int) protocol.ModelsHub {
	if index == 1 {
		return protocol.ModelsHubModelScope
	}
	return protocol.ModelsHubHuggingFace
}

func libraryHubKey(hub protocol.ModelsHub) string {
	if hub == protocol.ModelsHubModelScope {
		return "model_library_modelscope"
	}
	return "model_library_huggingface"
}

// libraryKindKey is the kind's i18n key (settings.Models*).
func libraryKindKey(kind protocol.ModelsKind) string {
	for key, value := range localModelKinds {
		if value == kind {
			return key
		}
	}
	return ""
}

// modelSize is the download size of a model's files, "" when unknown.
func modelSize(assets []protocol.ModelsAsset) string {
	var bytes uint64
	for _, asset := range assets {
		bytes += asset.Bytes
	}
	switch {
	case bytes == 0:
		return ""
	case bytes < 1<<20:
		return fmt.Sprintf("%.1f KiB", float64(bytes)/(1<<10))
	}
	return fmt.Sprintf("%.1f MiB", float64(bytes)/(1<<20))
}
