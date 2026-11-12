package live

import (
	"context"
	"fmt"
	"slices"

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
// searched and the Node that searched it, the hub's results and the cursor
// of its next page, and what the details dialog read, by row ID: the
// model's versions and the definition of the version shown.
type modelLibraryState struct {
	request    settings.LibraryRequest
	owner      *Runtime
	items      []protocol.ModelsItem
	next       *string
	failed     bool
	details    map[string]protocol.ModelsDefinition
	versions   map[string][]protocol.ModelsVersion
	resolving  map[string]bool
	generation uint64
	ctx        context.Context
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
	m.Library = libraryRows(lib.items, lib.details, lib.versions, r.libraryHeld(owner))
	m.LibraryMore, m.LibraryFailed = lib.next != nil, lib.failed
}

// searchLibrary reads the hub's first page for the request. A failure is
// the model_library_failed toast and leaves the dialog a Retry.
func (r *Runtime) searchLibrary(owner *Runtime, request settings.LibraryRequest) {
	lib := &r.localModels.library
	if lib.cancel != nil {
		lib.cancel()
	}
	*lib = modelLibraryState{request: request, owner: owner, generation: lib.generation + 1,
		details: map[string]protocol.ModelsDefinition{}, versions: map[string][]protocol.ModelsVersion{}, resolving: map[string]bool{}}
	lib.ctx, lib.cancel = context.WithCancel(owner.ctx)
	r.App.Settings.LibraryLoad.Loaded = false
	r.readLibraryPage()
}

// moreLibrary reads the hub's next page under the results shown.
func (r *Runtime) moreLibrary() {
	lib := &r.localModels.library
	if lib.owner != nil && lib.next != nil && !r.App.Settings.LibraryLoad.Loading {
		r.readLibraryPage()
	}
}

// readLibraryPage reads the request's page at the cursor and adds its
// results. An empty page that has a successor is followed (followLibrary),
// so that a page whose results the Node filtered out does not read as "no
// models". A failure is the model_library_failed toast and keeps the
// cursor; a hub that answered in part is said so.
func (r *Runtime) readLibraryPage() {
	lib := &r.localModels.library
	owner, generation, ctx := lib.owner, lib.generation, lib.ctx
	m := r.App.Settings
	m.LibraryLoad.Loading = true
	finish := func(page protocol.ModelsPage, err error) {
		r.enqueue(func(c *ui.Context) {
			if lib.generation != generation {
				return
			}
			lib.items = uniqueLibraryItems(append(lib.items, page.Items...))
			m.LibraryLoad.Loading, m.LibraryLoad.Loaded = false, true
			lib.failed = err != nil
			if err != nil {
				kit.Toast(c, kit.Failed, i18n.T(c, "model_library_failed"))
				return
			}
			lib.next = page.NextCursor
			if page.Partial {
				kit.Message(c, i18n.T(c, "model_library_partial"))
			}
		})
	}
	kind, known := localModelKinds[lib.request.Kind]
	if !known {
		finish(protocol.ModelsPage{}, fmt.Errorf("unknown model kind %q", lib.request.Kind))
		return
	}
	search := protocol.ModelsSearch{Hub: libraryHub(lib.request.Hub), Kind: kind, Query: lib.request.Query, Cursor: lib.next}
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
// may lie beyond that are reported as a partial page. The page names the
// last answer's successor, which Load more reads.
func followLibrary(search protocol.ModelsSearch, read func(protocol.ModelsSearch) (protocol.ModelsPage, error)) (protocol.ModelsPage, error) {
	var page protocol.ModelsPage
	for range libraryAnswers {
		next, err := read(search)
		if err != nil {
			return page, err
		}
		page.Items, page.Partial = append(page.Items, next.Items...), page.Partial || next.Partial
		page.NextCursor = next.NextCursor
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

// libraryHolding is the model a host has of a hub result: a Node's
// registered model, or for a speech result the controller's.
type libraryHolding struct {
	id         string
	definition protocol.ModelsDefinition
	status     viewmodel.ModelStatus
	percent    float64
}

// libraryHeld finds what is held of a result: the same hub, repository and
// kind, and the same revision where the result names a commit. A speech
// model is the controller's, never a Node's.
func (r *Runtime) libraryHeld(owner *Runtime) func(protocol.ModelsItem) (libraryHolding, bool) {
	var tasks []protocol.InstallationTask
	if owner.packages != nil {
		tasks = owner.packages.tasks
	}
	manager := r.localRuntime().dictation.manager
	return func(item protocol.ModelsItem) (libraryHolding, bool) {
		kind := modelFormatKind(item.Format)
		if kind == protocol.ModelsKindSpeech {
			if manager == nil {
				return libraryHolding{}, false
			}
			return speechHeld(manager.Entries(), item)
		}
		model, registered := registeredModel(owner.localModels.models, item)
		if !registered {
			return libraryHolding{}, false
		}
		held := libraryHolding{id: model.ID, definition: model.Definition}
		held.status, held.percent = localModelStatus(model, kind, tasks)
		return held, true
	}
}

// installLibraryModel downloads a result. A row's install (no choice)
// takes the model the host has registered, or resolves the result on its
// hub; the details' install takes the version they show, with an embedding
// model's prefixes as typed. The definition is then registered where its
// kind lives and downloaded. The hub must answer with the model that was
// asked for.
func (r *Runtime) installLibraryModel(id string, choice *settings.LibraryChoice) {
	owner, item, found := r.libraryItem(id)
	if !found {
		return
	}
	lib := &r.localModels.library
	kind := modelFormatKind(item.Format)
	held, registered := r.libraryHeld(owner)(item)
	var chosen *protocol.ModelsDefinition
	if choice != nil {
		definition, resolved := lib.details[id]
		if !resolved {
			if !registered {
				return
			}
			definition = held.definition
		}
		if kind == protocol.ModelsKindEmbedding {
			definition.QueryPrefix, definition.DocumentPrefix = modelPrefix(choice.QueryPrefix), modelPrefix(choice.DocumentPrefix)
		}
		chosen = &definition
	}
	if registered && (chosen == nil || sameModelDefinition(*chosen, held.definition)) {
		if kind == protocol.ModelsKindSpeech {
			r.installSpeechModel(held.id)
		} else {
			owner.installLocalModel(kind, held.id)
		}
		return
	}
	if chosen != nil {
		r.acquireModel(owner, kind, *chosen)
		return
	}
	if lib.resolving[id] {
		return
	}
	lib.resolving[id] = true
	generation := lib.generation
	started := owner.async(func() {
		definition, err := owner.resolveModel(kind, item.Source)
		r.enqueue(func(c *ui.Context) {
			if lib.generation == generation {
				delete(lib.resolving, id)
			}
			if err != nil {
				owner.reportKeys(c, err, "runtime_failed", "runtime_conflict", "runtime_unknown")
				return
			}
			r.acquireModel(owner, kind, definition)
		})
	})
	if !started {
		delete(lib.resolving, id)
	}
}

// acquireModel registers a resolved definition where its kind lives and
// downloads it: a speech model with the controller, another with the Node.
func (r *Runtime) acquireModel(owner *Runtime, kind protocol.ModelsKind, definition protocol.ModelsDefinition) {
	if kind == protocol.ModelsKindSpeech {
		r.addSpeechModel(definition)
		return
	}
	owner.async(func() {
		output, err := owner.controller.Do(owner.ctx, protocol.Command{Kind: "register_local_model", Data: protocol.CommandRegisterLocalModelPayload{Definition: definition}})
		registered, valid := detail[protocol.OutputLocalModelPayload](output.Data)
		if err == nil && (output.Kind != "local_model" || !valid) {
			err = fmt.Errorf("unexpected model registration response")
		}
		owner.enqueue(func(c *ui.Context) {
			if err != nil {
				owner.reportKeys(c, err, "runtime_failed", "runtime_conflict", "runtime_unknown")
				return
			}
			owner.localModels.stale = true
			owner.installLocalModel(kind, registered.ID)
		})
	})
}

// modelPrefix is an embedding prefix as typed: none when empty.
func modelPrefix(value string) *string {
	if value == "" {
		return nil
	}
	return &value
}

// sameModelDefinition reports the same source and prefixes.
func sameModelDefinition(a, b protocol.ModelsDefinition) bool {
	text := func(value *string) string {
		if value == nil {
			return ""
		}
		return *value
	}
	return a.Source == b.Source && text(a.QueryPrefix) == text(b.QueryPrefix) && text(a.DocumentPrefix) == text(b.DocumentPrefix)
}

// resolveModel asks the Node for a hub model's definition and checks that
// it is the model and the kind asked for.
func (r *Runtime) resolveModel(kind protocol.ModelsKind, source protocol.ModelsSource) (protocol.ModelsDefinition, error) {
	output, err := r.controller.Do(r.ctx, protocol.Command{Kind: "resolve_model", Data: protocol.CommandResolveModelPayload{Kind: kind, Source: source}})
	return modelDefinition(output, err, kind, source)
}

// modelDefinition is a resolve's answer, checked.
func modelDefinition(output protocol.Output, err error, kind protocol.ModelsKind, source protocol.ModelsSource) (protocol.ModelsDefinition, error) {
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
	owner, item, found := r.libraryItem(id)
	if !found {
		return
	}
	if held, registered := r.libraryHeld(owner)(item); !registered {
		return
	} else if modelFormatKind(item.Format) == protocol.ModelsKindSpeech {
		r.cancelSpeechModel(held.id)
	} else {
		owner.cancelLocalModel(held.id)
	}
}

// deleteLibraryModel removes an installed result's files, after the
// dialog's confirmation. Its success is the "Deleted" toast.
func (r *Runtime) deleteLibraryModel(id string) {
	owner, item, found := r.libraryItem(id)
	if !found {
		return
	}
	if held, registered := r.libraryHeld(owner)(item); !registered {
		return
	} else if modelFormatKind(item.Format) == protocol.ModelsKindSpeech {
		r.deleteSpeechModel(held.id)
	} else {
		owner.deleteLocalModel(held.id)
	}
}

// deleteLocalModel removes a model's files on the Node.
func (r *Runtime) deleteLocalModel(id string) {
	r.executeScoped("local-models", protocol.Command{Kind: "delete_local_model", Data: protocol.CommandDeleteLocalModelPayload{ID: id}}, func(c *ui.Context, _ protocol.Output, err error) {
		r.localModels.stale = true
		if err != nil {
			r.reportKeys(c, err, "runtime_failed", "runtime_conflict", "runtime_unknown")
			return
		}
		kit.Toast(c, kit.Done, i18n.T(c, "knowledge_deleted"))
	})
}

// updateLocalModel looks for a newer version of a Node's model: an OCR
// model's files are read again, another's source is resolved at its head.
// The model keeps its prefixes. A version the Node has installed is
// "Already up to date"; another is registered and downloaded.
func (r *Runtime) updateLocalModel(id string) {
	s := &r.localModels
	index := slices.IndexFunc(s.models, func(model protocol.ModelsLocalModel) bool { return model.ID == id })
	if index < 0 || s.updating[id] {
		return
	}
	previous := s.models[index].Definition
	kind := modelFormatKind(previous.Format)
	source := previous.Source
	source.Revision = "HEAD"
	command := protocol.Command{Kind: "resolve_model", Data: protocol.CommandResolveModelPayload{Kind: kind, Source: source}}
	if kind == protocol.ModelsKindOcr {
		command = protocol.Command{Kind: "refresh_local_model", Data: protocol.CommandRefreshLocalModelPayload{ID: id}}
	}
	if s.updating == nil {
		s.updating = map[string]bool{}
	}
	s.updating[id] = true
	started := r.async(func() {
		output, err := r.controller.Do(r.ctx, command)
		definition, err := modelDefinition(output, err, kind, source)
		r.enqueue(func(c *ui.Context) {
			delete(s.updating, id)
			if err != nil {
				r.reportKeys(c, err, "runtime_failed", "runtime_conflict", "runtime_unknown")
				return
			}
			definition.QueryPrefix, definition.DocumentPrefix = previous.QueryPrefix, previous.DocumentPrefix
			if slices.ContainsFunc(s.models, func(model protocol.ModelsLocalModel) bool {
				return model.State.Kind == "installed" && model.Definition.Source == definition.Source
			}) {
				kit.Message(c, i18n.T(c, "knowledge_up_to_date"))
				return
			}
			r.localRuntime().acquireModel(r, kind, definition)
		})
	})
	if !started {
		delete(s.updating, id)
	}
}

// readLibraryModel reads a result's versions for the details dialog and,
// for one the host does not hold, resolves the newest for its size and
// license.
func (r *Runtime) readLibraryModel(id string) {
	owner, item, found := r.libraryItem(id)
	if !found {
		return
	}
	lib := &r.localModels.library
	if _, read := lib.versions[id]; read || lib.resolving[id] {
		return
	}
	_, held := r.libraryHeld(owner)(item)
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
		if err == nil && !held {
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
			lib.versions[id] = versions
			if !held {
				lib.details[id] = definition
			}
		})
	})
	if !started {
		delete(lib.resolving, id)
	}
}

// showLibraryVersion resolves another of a result's versions for the
// details dialog. The entry past the hub's versions is the version the
// host holds.
func (r *Runtime) showLibraryVersion(id string, version int) {
	owner, item, found := r.libraryItem(id)
	lib := &r.localModels.library
	if !found || version < 0 || lib.resolving[id] {
		return
	}
	versions := lib.versions[id]
	if version >= len(versions) {
		delete(lib.details, id)
		return
	}
	source := item.Source
	source.Revision = versions[version].Revision
	lib.resolving[id] = true
	generation := lib.generation
	started := owner.async(func() {
		definition, err := owner.resolveModel(modelFormatKind(item.Format), source)
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
// state of the model held from the same source, and the version, size and
// license its details show: those of the definition resolved for them, or
// of the model held. A held version the hub does not list is the last of
// the row's versions.
func libraryRows(items []protocol.ModelsItem, details map[string]protocol.ModelsDefinition, versions map[string][]protocol.ModelsVersion, held func(protocol.ModelsItem) (libraryHolding, bool)) []viewmodel.LibraryModel {
	rows := make([]viewmodel.LibraryModel, 0, len(items))
	for _, item := range items {
		id := libraryID(item.Source)
		row := viewmodel.LibraryModel{ID: id, Name: item.Name, Repo: item.Source.Repository, Kind: libraryKindKey(modelFormatKind(item.Format)), Source: libraryHubKey(item.Source.Hub)}
		if row.Name == "" {
			row.Name = row.Repo
		}
		definition, resolved := details[id]
		holding, registered := held(item)
		if registered {
			row.Status, row.Percent = holding.status, holding.percent
			if !resolved {
				definition, resolved = holding.definition, true
			}
			row.Current = definition.Source.Revision == holding.definition.Source.Revision
		}
		if resolved {
			row.Size, row.License, row.Resolved = modelSize(definition.Assets), definition.License, true
			if definition.QueryPrefix != nil {
				row.QueryPrefix = *definition.QueryPrefix
			}
			if definition.DocumentPrefix != nil {
				row.DocumentPrefix = *definition.DocumentPrefix
			}
		}
		for _, version := range versions[id] {
			row.Versions = append(row.Versions, versionName(version))
		}
		if len(row.Versions) > 0 && resolved {
			revision := definition.Source.Revision
			row.Version = slices.IndexFunc(versions[id], func(version protocol.ModelsVersion) bool { return version.Revision == revision })
			if row.Version < 0 {
				row.Versions, row.Version = append(row.Versions, versionName(protocol.ModelsVersion{Revision: revision})), len(row.Versions)
			}
		}
		rows = append(rows, row)
	}
	return rows
}

// versionName is a version's name, or its revision cut as a commit is.
func versionName(version protocol.ModelsVersion) string {
	if version.Name != "" {
		return version.Name
	}
	if commitRevision(version.Revision) {
		return version.Revision[:7]
	}
	return version.Revision
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
