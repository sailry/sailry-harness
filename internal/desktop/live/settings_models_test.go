package live

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"net/http"
	"os"
	"path/filepath"
	"slices"
	"testing"

	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/speech/models"

	"github.com/sailry/sailry-harness/internal/desktop/preferences"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// checkLocalModelHelpers covers the pure halves of the local-model pages:
// the registry's projection onto the pages, the choice a page saves, the
// request it sends, download progress and the library's rows.
func checkLocalModelHelpers(t *testing.T) {
	t.Run("library search follows empty answers a bounded number of times", checkLibraryFollow)
	// A registry event during a read marks the read's answer stale.
	watched := localModelState{reading: true}
	watched.registryChanged([]protocol.ModelsLocalModel{{ID: "during"}})
	if !watched.stale || len(watched.models) != 1 {
		t.Fatalf("an event during a read: %+v", watched)
	}
	watched = localModelState{}
	watched.registryChanged([]protocol.ModelsLocalModel{{ID: "idle"}})
	if watched.stale || watched.models[0].ID != "idle" {
		t.Fatalf("an event while idle: %+v", watched)
	}
	t.Helper()
	model := func(id, name, format, state string, source protocol.ModelsSource) protocol.ModelsLocalModel {
		return protocol.ModelsLocalModel{ID: id, Definition: protocol.ModelsDefinition{Source: source, Name: name, License: "mit",
			Format: protocol.ModelsFormat{Kind: format}, Assets: []protocol.ModelsAsset{{Bytes: 3 << 19}}}, State: protocol.ModelsLocalState{Kind: state}}
	}
	commit := "0123456789abcdef0123456789abcdef01234567"
	hub := func(repository, revision string) protocol.ModelsSource {
		return protocol.ModelsSource{Hub: protocol.ModelsHubHuggingFace, Repository: repository, Revision: revision}
	}
	models := []protocol.ModelsLocalModel{
		model("e1", "Embed", "transformer", "installed", hub("org/embed", commit)),
		model("e2", "Embed", "static", "downloading", hub("org/static", "main")),
		model("e3", "Other", "transformer", "missing", hub("org/other", "main")),
		model("o1", "OCR", "paddle_ocr", "installed", hub("org/ocr", "main")),
		model("s1", "Speech", "sense_voice", "installed", hub("org/speech", "main")),
	}
	tasks := []protocol.InstallationTask{
		{Resource: protocol.InstallationResource{Kind: "model", Data: protocol.InstallationResourceModelPayload{Kind: protocol.ModelsKindEmbedding, ID: "e2"}},
			State: protocol.InstallationState{Kind: "running", Data: protocol.InstallationStateRunningPayload{
				Progress: &protocol.InstallationProgress{Kind: "percent", Data: protocol.InstallationProgressPercentPayload{Percent: 42}}}}},
		{Resource: protocol.InstallationResource{Kind: "model", Data: protocol.InstallationResourceModelPayload{Kind: protocol.ModelsKindOcr, ID: "e3"}},
			State: protocol.InstallationState{Kind: "running", Data: protocol.InstallationStateRunningPayload{
				Progress: &protocol.InstallationProgress{Kind: "counts", Data: protocol.InstallationProgressCountsPayload{Completed: 1, Total: 4}}}}},
	}

	views := localModelViews(models, protocol.ModelsKindEmbedding, tasks)
	if len(views) != 3 || views[0].Status != viewmodel.ModelInstalled || views[1].Status != viewmodel.ModelDownloading || views[1].Percent != 42 ||
		views[2].Status != viewmodel.ModelMissing || views[0].Name == views[1].Name || views[2].Name != "Other" {
		t.Fatalf("embedding views = %+v", views)
	}
	if got := modelProgress(tasks, protocol.ModelsKindOcr, "e3"); got != 25 {
		t.Fatalf("counted progress = %v", got)
	}
	if got := modelProgress(tasks, protocol.ModelsKindEmbedding, "e3"); got != 0 {
		t.Fatalf("another kind's progress = %v", got)
	}

	selected, gone := "e1", "e3"
	state := &localModelState{known: true, models: models,
		settings: protocol.ModelsLocalSettings{Revision: 4, Choices: map[protocol.ModelsKind]protocol.ModelsLocalChoice{
			protocol.ModelsKindEmbedding: {Enabled: true, Model: &selected},
			protocol.ModelsKindOcr:       {Enabled: true, Model: &gone},
			protocol.ModelsKindDecision:  {},
		}},
		availability: map[protocol.ModelsKind]protocol.ModelsCatalogAvailability{
			protocol.ModelsKindEmbedding: {Kind: "available"},
			protocol.ModelsKindOcr:       {Kind: "available"},
			protocol.ModelsKindDecision:  {Kind: "unavailable"},
		}}
	m := &settings.Model{Live: true}
	projectLocalModels(m, state, tasks)
	if m.Knowledge.Kind != settings.ModelsEmbedding || m.Knowledge.Model != 0 || m.Knowledge.Missing != "" || m.Knowledge.Enabled.Pending ||
		!m.Knowledge.Enabled.Draft || len(m.Knowledge.Models) != 3 || len(m.Knowledge.Catalog) != 2 {
		t.Fatalf("knowledge page = %+v", m.Knowledge)
	}
	if m.OCR.Model != 1 || m.OCR.Missing != "Other" || !m.OCR.Enabled.Draft || m.OCR.Unavailable {
		t.Fatalf("OCR page = %+v", m.OCR)
	}
	if !m.Decision.Unavailable || !m.Decision.Enabled.Pending || m.Decision.Model != -1 {
		t.Fatalf("decision page = %+v", m.Decision)
	}
	// Speech models are the controller's: a Node's are not shown, and the
	// selection is the dictation.model preference.
	if len(m.Speech) != 0 {
		t.Fatalf("a Node's speech models are shown: %+v", m.Speech)
	}
	stored := preferences.Default()
	if stored.DictationModel() != nil || stored.DictationLanguage() != "auto" || stored.DictationMicrophone() != nil {
		t.Fatalf("default dictation preferences = %+v", stored.Dictation)
	}
	speech, chosen := speechModels(stored.DictationModel(), nil)
	if len(speech) != 1 || speech[0].ID != "" || chosen != 0 {
		t.Fatalf("speech models = %+v, selected %d", speech, chosen)
	}
	id, microphone := protocol.NewModelID(), "usb"
	picked, valid := speechSelection(id.String())
	if !valid || picked == nil || *picked != id {
		t.Fatalf("speech selection = %v, %v", picked, valid)
	}
	if none, valid := speechSelection(""); !valid || none != nil {
		t.Fatalf("recommended speech selection = %v, %v", none, valid)
	}
	if _, valid := speechSelection("s1"); valid {
		t.Fatal("a Node model ID is taken for a speech model")
	}
	stored.Dictation = &preferences.Dictation{Microphone: &microphone, Language: "chinese", Model: picked}
	if _, chosen := speechModels(stored.DictationModel(), nil); chosen != -1 {
		t.Fatalf("an unlisted speech model is selected at %d", chosen)
	}
	if stored.DictationLanguage() != "chinese" || *stored.DictationMicrophone() != "usb" || *stored.DictationModel() != id {
		t.Fatalf("dictation preferences = %+v", stored.Dictation)
	}
	encoded, err := json.Marshal(stored.Dictation)
	if err != nil || string(encoded) != `{"microphone":"usb","language":"chinese","model":"`+id.String()+`"}` {
		t.Fatalf("dictation preferences encode as %s, %v", encoded, err)
	}

	// The picker's choices: none turns embeddings to keywords only, and
	// the saved model that is no longer installed stays the choice.
	page := m.Knowledge
	page.Model = -1
	if choice := pageLocalChoice(protocol.ModelsKindEmbedding, page, state.settings.Choices[protocol.ModelsKindEmbedding]); choice.Enabled || choice.Model != nil {
		t.Fatalf("keyword-only choice = %+v", choice)
	}
	page = m.OCR
	page.Enabled.Draft = false
	choice := pageLocalChoice(protocol.ModelsKindOcr, page, state.settings.Choices[protocol.ModelsKindOcr])
	if choice.Enabled || choice.Model == nil || *choice.Model != "e3" {
		t.Fatalf("OCR choice = %+v", choice)
	}
	page.Model = 0
	if choice := pageLocalChoice(protocol.ModelsKindOcr, page, state.settings.Choices[protocol.ModelsKindOcr]); choice.Model == nil || *choice.Model != "o1" {
		t.Fatalf("installed OCR choice = %+v", choice)
	}

	// A save sends every choice at the revision read and leaves what was
	// read untouched; a draft the Node then holds is no longer one.
	state.drafts = map[protocol.ModelsKind]protocol.ModelsLocalChoice{protocol.ModelsKindOcr: choice}
	request := localModelRequest(state.settings, state.drafts)
	if request.Revision != 4 || len(request.Choices) != 3 || request.Choices[protocol.ModelsKindOcr].Enabled ||
		!state.settings.Choices[protocol.ModelsKindOcr].Enabled || !sameLocalChoice(request.Choices[protocol.ModelsKindEmbedding], state.settings.Choices[protocol.ModelsKindEmbedding]) {
		t.Fatalf("save request = %+v", request)
	}
	if state.choice(protocol.ModelsKindOcr).Enabled {
		t.Fatal("the page does not show its draft")
	}
	request.Revision = 5
	state.acceptSettings(request)
	if len(state.drafts) != 0 || state.settings.Revision != 5 {
		t.Fatalf("accepted settings = %+v, drafts %+v", state.settings, state.drafts)
	}
	// A read answered before that save does not take the pages back; a
	// newer one is taken.
	state.acceptRead(protocol.ModelsLocalSettings{Revision: 4})
	if state.settings.Revision != 5 || len(state.settings.Choices) != len(request.Choices) {
		t.Fatalf("an older read left %+v", state.settings)
	}
	newer := request
	newer.Revision = 6
	if state.acceptRead(newer); state.settings.Revision != 6 {
		t.Fatalf("a newer read left %+v", state.settings)
	}

	// The library: a result takes the state of the Node's model from the
	// same source; a commit must match, a branch matches any revision.
	item := func(repository, revision, format string) protocol.ModelsItem {
		return protocol.ModelsItem{Source: hub(repository, revision), Name: repository, Format: protocol.ModelsFormat{Kind: format}}
	}
	items := uniqueLibraryItems([]protocol.ModelsItem{
		item("org/embed", "main", "transformer"), item("org/embed", "main", "transformer"),
		item("org/embed", "89abcdef0123456789abcdef0123456789abcdef", "transformer"),
		item("org/static", "main", "static"), item("org/new", "main", "transformer"),
	})
	if len(items) != 4 {
		t.Fatalf("unique items = %+v", items)
	}
	resolved := map[string]protocol.ModelsDefinition{libraryID(items[3].Source): {License: "apache-2.0", Assets: []protocol.ModelsAsset{{Bytes: 512}}}}
	held := func(item protocol.ModelsItem) (libraryHolding, bool) {
		model, registered := registeredModel(models, item)
		holding := libraryHolding{id: model.ID, definition: model.Definition}
		holding.status, holding.percent = localModelStatus(model, modelFormatKind(item.Format), tasks)
		return holding, registered
	}
	rows := libraryRows(items, resolved, nil, held)
	if rows[0].Status != viewmodel.ModelInstalled || rows[0].Size != "1.5 MiB" || rows[0].License != "mit" || rows[0].Kind != settings.ModelsEmbedding ||
		rows[0].Source != "model_library_huggingface" || rows[1].Status != viewmodel.ModelMissing || rows[1].Size != "" ||
		rows[2].Status != viewmodel.ModelDownloading || rows[2].Percent != 42 || rows[3].Size != "0.5 KiB" || rows[3].License != "apache-2.0" {
		t.Fatalf("library rows = %+v", rows)
	}
	// A held model's own definition is what its details show until another
	// version is resolved for them; a held version the hub does not list
	// is the last of the row's versions.
	listed := map[string][]protocol.ModelsVersion{libraryID(items[0].Source): {{Name: "main", Revision: "fedcba9876543210fedcba9876543210fedcba98"}, {Revision: "v1"}}}
	rows = libraryRows(items, resolved, listed, held)
	if want := []string{"main", "v1", commit[:7]}; !slices.Equal(rows[0].Versions, want) || rows[0].Version != 2 || !rows[0].Current || !rows[0].Resolved || rows[1].Resolved || rows[3].Current {
		t.Fatalf("held versions = %+v", rows)
	}
	prefix := "query: "
	resolved[libraryID(items[0].Source)] = protocol.ModelsDefinition{Source: hub("org/embed", "fedcba9876543210fedcba9876543210fedcba98"), License: "bsd", QueryPrefix: &prefix}
	rows = libraryRows(items, resolved, listed, held)
	if rows[0].Version != 0 || rows[0].Current || rows[0].License != "bsd" || rows[0].QueryPrefix != prefix || rows[0].Status != viewmodel.ModelInstalled || len(rows[0].Versions) != 2 {
		t.Fatalf("another version's details = %+v", rows[0])
	}
	if modelPrefix("") != nil || *modelPrefix(prefix) != prefix || !sameModelDefinition(protocol.ModelsDefinition{}, protocol.ModelsDefinition{QueryPrefix: modelPrefix("")}) ||
		sameModelDefinition(protocol.ModelsDefinition{QueryPrefix: &prefix}, protocol.ModelsDefinition{}) {
		t.Fatal("prefixes are misread")
	}
	// A managed row carries its version, license and size; what is
	// installed can be removed.
	if views[0].Version != commit || views[0].License != "mit" || views[0].Size != "1.5 MiB" || !views[0].Removable || views[1].Removable || views[2].Removable {
		t.Fatalf("managed details = %+v", views)
	}
	if !commitRevision(commit) || commitRevision("main") || commitRevision(commit[:39]+"G") {
		t.Fatal("commit revisions are misread")
	}
	if libraryHub(1) != protocol.ModelsHubModelScope || libraryHub(0) != protocol.ModelsHubHuggingFace || libraryHubKey(protocol.ModelsHubModelScope) != "model_library_modelscope" {
		t.Fatal("hubs are misread")
	}
}

func checkLibraryFollow(t *testing.T) {
	cursor := func(value string) *string { return &value }
	// answers are the hub's pages in order; the last one repeats.
	follow := func(answers ...protocol.ModelsPage) (protocol.ModelsPage, []string) {
		var asked []string
		page, err := followLibrary(protocol.ModelsSearch{Query: "julia"}, func(search protocol.ModelsSearch) (protocol.ModelsPage, error) {
			at := ""
			if search.Cursor != nil {
				at = *search.Cursor
			}
			asked = append(asked, at)
			return answers[min(len(asked), len(answers))-1], nil
		})
		if err != nil {
			t.Fatal(err)
		}
		return page, asked
	}
	found := protocol.ModelsItem{Name: "Julia"}

	// The first answer with results ends the search.
	page, asked := follow(protocol.ModelsPage{NextCursor: cursor("two")}, protocol.ModelsPage{Items: []protocol.ModelsItem{found}, NextCursor: cursor("three")})
	if len(page.Items) != 1 || page.Partial || len(asked) != 2 || asked[0] != "" || asked[1] != "two" || page.NextCursor == nil || *page.NextCursor != "three" {
		t.Fatalf("followed answer: %+v after %q", page, asked)
	}
	// A hub without more pages has no results.
	if page, asked = follow(protocol.ModelsPage{}); len(page.Items) != 0 || page.Partial || len(asked) != 1 || page.NextCursor != nil {
		t.Fatalf("last answer: %+v after %q", page, asked)
	}
	// A listing of other kinds of model is not read to its end.
	if page, asked = follow(protocol.ModelsPage{NextCursor: cursor("more")}); len(page.Items) != 0 || !page.Partial || len(asked) != libraryAnswers {
		t.Fatalf("endless listing: %+v after %q", page, asked)
	}
	// A failure keeps what was read.
	failed := errors.New("hub failed")
	if _, err := followLibrary(protocol.ModelsSearch{}, func(protocol.ModelsSearch) (protocol.ModelsPage, error) {
		return protocol.ModelsPage{}, failed
	}); !errors.Is(err, failed) {
		t.Fatalf("failure: %v", err)
	}
}

// checkSpeechLibrary covers the library's speech models, which the
// controller holds: a hub result takes the state of the registered model
// from its source, a definition with other files for a registered source is
// refused, and a removal deletes the files and gives dictation back the
// recommended model.
func checkSpeechLibrary(t *testing.T) {
	directory := filepath.Join(t.TempDir(), "desktop")
	id := protocol.NewModelID()
	source := protocol.ModelsSource{Hub: protocol.ModelsHubHuggingFace, Repository: "org/voice", Revision: "0123456789abcdef0123456789abcdef01234567"}
	definition := protocol.ModelsDefinition{Source: source, Name: "Voice", License: "mit", Format: protocol.ModelsFormat{Kind: "sense_voice", Data: &protocol.ModelsFormatSenseVoicePayload{}}}
	files := filepath.Join(directory, "models", "speech", id.String())
	if err := os.MkdirAll(files, 0o700); err != nil {
		t.Fatal(err)
	}
	for _, name := range []string{"model.onnx", "tokens.txt"} {
		content := []byte("fixture " + name)
		sum := sha256.Sum256(content)
		definition.Assets = append(definition.Assets, protocol.ModelsAsset{Source: source, Path: name, Name: name, Bytes: uint64(len(content)), Sha256: hex.EncodeToString(sum[:])})
		if err := os.WriteFile(filepath.Join(files, name), content, 0o600); err != nil {
			t.Fatal(err)
		}
	}
	index, err := json.Marshal([]models.Record{{ID: id, Installed: true, Definition: definition}})
	if err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(directory, "models", "speech.json"), index, 0o600); err != nil {
		t.Fatal(err)
	}
	ctx, cancel := context.WithCancel(t.Context())
	manager, err := models.Open(ctx, models.Options{Directory: directory, Transport: roundTrip(func(*http.Request) (*http.Response, error) {
		return nil, errors.New("no network in tests")
	})})
	if err != nil {
		t.Fatal(err)
	}
	r := &Runtime{ctx: ctx, cancel: cancel, App: shell.NewLive(&viewmodel.Data{Workspace: &viewmodel.Workspace{}}),
		Preferences: preferences.Open(filepath.Join(directory, "preferences.json"))}
	r.dictation.manager = manager
	defer func() {
		if err := r.Close(); err != nil {
			t.Error(err)
		}
	}()
	r.Preferences.Data.Dictation = &preferences.Dictation{Language: "english", Model: &id}

	item := protocol.ModelsItem{Source: protocol.ModelsSource{Hub: source.Hub, Repository: source.Repository, Revision: "main"}, Name: "Voice", Format: definition.Format}
	held := r.libraryHeld(r)
	holding, registered := held(item)
	if !registered || holding.id != id.String() || holding.status != viewmodel.ModelInstalled {
		t.Fatalf("held speech model = %+v, %v", holding, registered)
	}
	other := item
	other.Source.Revision = "fedcba9876543210fedcba9876543210fedcba98"
	if _, registered := held(other); registered {
		t.Fatal("another commit counts as the registered model")
	}
	rows := libraryRows([]protocol.ModelsItem{item}, nil, nil, held)
	if rows[0].Status != viewmodel.ModelInstalled || rows[0].License != "mit" || !rows[0].Current || rows[0].Kind != settings.ModelsSpeech {
		t.Fatalf("speech library row = %+v", rows[0])
	}
	managed, selected := speechModels(&id, manager)
	if len(managed) != 2 || selected != 1 || managed[0].Removable || !managed[1].Removable || managed[1].Version != source.Revision || managed[1].License != "mit" {
		t.Fatalf("speech rows = %+v, selected %d", managed, selected)
	}

	// The same source with other files is refused and registers nothing.
	conflict := definition
	conflict.Assets = slices.Clone(definition.Assets)
	conflict.Assets[0].Bytes++
	r.addSpeechModel(conflict)
	if entries := manager.Entries(); len(entries) != 1 {
		t.Fatalf("a conflicting definition left %d models", len(entries))
	}

	r.deleteSpeechModel(id.String())
	if entries := manager.Entries(); len(entries) != 0 {
		t.Fatalf("the removal left %+v", entries)
	}
	if _, err := os.Stat(files); !os.IsNotExist(err) {
		t.Fatalf("the removal left the files: %v", err)
	}
	if r.Preferences.Data.DictationModel() != nil {
		t.Fatal("dictation still names the removed model")
	}
	if _, registered := r.libraryHeld(r)(item); registered {
		t.Fatal("the removed model is still held")
	}
}
