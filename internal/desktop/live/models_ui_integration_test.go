//go:build integration

package live

import (
	"context"
	"crypto/sha256"
	"encoding/binary"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"io"
	"io/fs"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"slices"
	"strconv"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/preferences"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/localmodels"
	"github.com/sailry/sailry-harness/internal/node"
	"github.com/sailry/sailry-harness/internal/protocol"
	"github.com/sailry/sailry-harness/internal/speech"
	"github.com/sailry/sailry-harness/internal/speech/capture"
	"github.com/sailry/sailry-harness/internal/speech/models"
)

// The B11 desktop acceptance drives the headless App over a real Node with
// the pinned native runtimes and the real model files. A loopback TLS server
// stands in for Hugging Face and ModelScope and serves the real files.
//
//	SAILRY_ONNXRUNTIME_DIR      pinned ONNX Runtime and sherpa-onnx libraries
//	SAILRY_CAPTURE_DIR          pinned capture library
//	SAILRY_DECISION_RUNTIME     packaged llama.cpp runtime with llama-runtime.json
//	SAILRY_EMBEDDING_MODEL_DIR  granite-multi: config.json, tokenizer.json, model.onnx
//	SAILRY_OCR_MODEL_DIR        PP-OCRv6 tiny files under their asset names
//	SAILRY_DECISION_JULIA       pinned Julia decision model
//	SAILRY_SPEECH_MODEL_DIR     model.int8.onnx, tokens.txt and test_wavs/en.wav
//
// The runtimes are copied beside the test binary, the only place the
// production code loads them from, so the Node runs the capabilities of the
// build unchanged.

const (
	// modelsUIWait bounds every wait that does not move or load a real model.
	modelsUIWait = 15 * time.Second
	// modelsUIInstallWait bounds an installation: it transfers, hashes and
	// loads up to 415 MB of real model files into a native runtime.
	modelsUIInstallWait = 180 * time.Second

	modelsUIJuliaRepository = "ggml-org/Julia-1-GGUF"
	modelsUIJuliaRevision   = "1e6b48a6983d7467f8eedc2cb0d21267b5fc052d"

	modelsUISpeechRepository = "csukuangfj/sherpa-onnx-sense-voice-zh-en-ja-ko-yue-2024-07-17"
	modelsUISpeechRevision   = "2365baeacb507f821a0c8120fcee3d484dba7a07"
	modelsUITranscript       = "The tribal chieftain called for the boy and presented him with 50 pieces of code."
)

func modelsUIEnvironment(t *testing.T, name string) string {
	t.Helper()
	value := os.Getenv(name)
	if value == "" {
		t.Fatalf("%s must be set", name)
	}
	return value
}

type modelsUIHubFile struct {
	local string
	size  int64
}

type modelsUIHubRepository struct {
	commit, license, path, local, sha256 string
}

type modelsUIHubRequest struct {
	host, key, ranged string
}

// modelsUIHub is a loopback stand-in for both model hubs.
type modelsUIHub struct {
	t      *testing.T
	server *httptest.Server
	done   chan struct{}
	// stalled receives the key of a response that stopped part way.
	stalled chan string

	mu           sync.Mutex
	files        map[string]modelsUIHubFile
	repositories map[string]modelsUIHubRepository
	stalls       map[string]int64
	requests     []modelsUIHubRequest
}

func modelsUIHubKey(source protocol.ModelsSource, path string) string {
	return fmt.Sprintf("%s/%s/%s/%s", source.Hub, source.Repository, source.Revision, path)
}

func newModelsUIHub(t *testing.T) *modelsUIHub {
	t.Helper()
	hub := &modelsUIHub{
		t: t, done: make(chan struct{}), stalled: make(chan string, 16),
		files: map[string]modelsUIHubFile{}, repositories: map[string]modelsUIHubRepository{}, stalls: map[string]int64{},
	}
	hub.server = httptest.NewTLSServer(http.HandlerFunc(hub.serve))
	t.Cleanup(func() {
		close(hub.done)
		hub.server.Close()
	})
	return hub
}

// RoundTrip sends a request for any host to the loopback server.
func (hub *modelsUIHub) RoundTrip(request *http.Request) (*http.Response, error) {
	redirected := request.Clone(request.Context())
	redirected.Header.Set("X-Hub-Host", request.URL.Host)
	redirected.URL.Host = hub.server.Listener.Addr().String()
	redirected.Host = ""
	return hub.server.Client().Transport.RoundTrip(redirected)
}

func (hub *modelsUIHub) add(key, local string) {
	hub.t.Helper()
	info, err := os.Stat(local)
	if err != nil {
		hub.t.Fatal(err)
	}
	hub.mu.Lock()
	hub.files[key] = modelsUIHubFile{local: local, size: info.Size()}
	hub.mu.Unlock()
}

// definition serves the assets of a pinned definition from a directory that
// holds them under their asset names.
func (hub *modelsUIHub) definition(definition protocol.ModelsDefinition, directory string) {
	hub.t.Helper()
	for _, asset := range definition.Assets {
		hub.add(modelsUIHubKey(asset.Source, asset.Path), filepath.Join(directory, asset.Name))
	}
}

// stall stops the first unranged response for key after bytes; zero lets
// the file through again.
func (hub *modelsUIHub) stall(key string, bytes int64) {
	hub.mu.Lock()
	if bytes > 0 {
		hub.stalls[key] = bytes
	} else {
		delete(hub.stalls, key)
	}
	hub.mu.Unlock()
}

// seen returns the recorded requests for one key.
func (hub *modelsUIHub) seen(key string) []modelsUIHubRequest {
	hub.mu.Lock()
	defer hub.mu.Unlock()
	var found []modelsUIHubRequest
	for _, request := range hub.requests {
		if request.key == key {
			found = append(found, request)
		}
	}
	return found
}

func (hub *modelsUIHub) record(request *http.Request, key string) {
	hub.requests = append(hub.requests, modelsUIHubRequest{host: request.Header.Get("X-Hub-Host"), key: key, ranged: request.Header.Get("Range")})
}

func (hub *modelsUIHub) serve(writer http.ResponseWriter, request *http.Request) {
	path := request.URL.Path
	switch {
	case path == "/api/models":
		query := strings.ToLower(request.URL.Query().Get("search"))
		rows := []map[string]string{}
		hub.mu.Lock()
		for name := range hub.repositories {
			if strings.Contains(strings.ToLower(name), query) {
				rows = append(rows, map[string]string{"id": name})
			}
		}
		hub.record(request, "search:"+query)
		hub.mu.Unlock()
		json.NewEncoder(writer).Encode(rows)
	case strings.HasPrefix(path, "/api/v1/models/") && strings.HasSuffix(path, "/repo"):
		name := strings.TrimSuffix(strings.TrimPrefix(path, "/api/v1/models/"), "/repo")
		query := request.URL.Query()
		source := protocol.ModelsSource{Hub: protocol.ModelsHubModelScope, Repository: name, Revision: query.Get("Revision")}
		hub.file(writer, request, modelsUIHubKey(source, query.Get("FilePath")))
	case strings.HasPrefix(path, "/api/models/"):
		rest := strings.TrimPrefix(path, "/api/models/")
		if name, found := strings.CutSuffix(rest, "/refs"); found {
			hub.mu.Lock()
			repository, known := hub.repositories[name]
			hub.mu.Unlock()
			if !known {
				http.NotFound(writer, request)
				return
			}
			json.NewEncoder(writer).Encode(map[string]any{"branches": []map[string]string{{"name": "main", "targetCommit": repository.commit}}, "tags": []any{}})
			return
		}
		name, revision, found := strings.Cut(rest, "/revision/")
		hub.mu.Lock()
		repository, known := hub.repositories[name]
		hub.record(request, "metadata:"+name+"@"+revision)
		hub.mu.Unlock()
		if !found || !known || revision != "main" && revision != repository.commit {
			http.NotFound(writer, request)
			return
		}
		info, err := os.Stat(repository.local)
		if err != nil {
			http.Error(writer, err.Error(), http.StatusInternalServerError)
			return
		}
		json.NewEncoder(writer).Encode(map[string]any{"sha": repository.commit, "private": false, "gated": false,
			"siblings": []map[string]any{{"rfilename": repository.path, "size": info.Size(),
				"lfs": map[string]any{"sha256": repository.sha256, "size": info.Size()}}},
			"cardData": map[string]string{"license": repository.license}})
	default:
		parts := strings.SplitN(strings.TrimPrefix(path, "/"), "/", 5)
		if len(parts) != 5 || parts[2] != "resolve" {
			http.NotFound(writer, request)
			return
		}
		source := protocol.ModelsSource{Hub: protocol.ModelsHubHuggingFace, Repository: parts[0] + "/" + parts[1], Revision: parts[3]}
		hub.file(writer, request, modelsUIHubKey(source, parts[4]))
	}
}

func (hub *modelsUIHub) file(writer http.ResponseWriter, request *http.Request, key string) {
	hub.mu.Lock()
	file, known := hub.files[key]
	stall := hub.stalls[key]
	hub.record(request, key)
	hub.mu.Unlock()
	if !known {
		http.NotFound(writer, request)
		return
	}
	source, err := os.Open(file.local)
	if err != nil {
		http.Error(writer, err.Error(), http.StatusInternalServerError)
		return
	}
	defer source.Close()
	writer.Header().Set("ETag", fmt.Sprintf(`"%s-%d"`, filepath.Base(file.local), file.size))
	writer.Header().Set("Content-Type", "application/octet-stream")
	if stall > 0 && request.Header.Get("Range") == "" {
		writer.Header().Set("Content-Length", strconv.FormatInt(file.size, 10))
		if _, err := io.CopyN(writer, source, stall); err != nil {
			return
		}
		writer.(http.Flusher).Flush()
		select {
		case hub.stalled <- key:
		default:
		}
		select {
		case <-request.Context().Done():
		case <-hub.done:
		case <-time.After(modelsUIInstallWait):
		}
		return
	}
	http.ServeContent(writer, request, "", time.Time{}, source)
}

// awaitStall waits for a response of key to stop part way.
func (hub *modelsUIHub) awaitStall(key string) {
	hub.t.Helper()
	deadline := time.After(modelsUIInstallWait)
	for {
		select {
		case stalled := <-hub.stalled:
			if stalled == key {
				return
			}
		case <-deadline:
			hub.t.Fatalf("the download of %s did not reach its stall", key)
		}
	}
}

// stageModelRuntimes copies the pinned native runtimes beside the test
// binary. The sources are only read.
func stageModelRuntimes(t *testing.T) {
	t.Helper()
	executable, err := os.Executable()
	if err == nil {
		executable, err = filepath.EvalSymlinks(executable)
	}
	if err != nil {
		t.Fatal(err)
	}
	directory := filepath.Dir(executable)
	stage := func(source string) {
		t.Helper()
		info, err := os.Stat(source)
		if err != nil {
			t.Fatal(err)
		}
		target := filepath.Join(directory, filepath.Base(source))
		if staged, err := os.Stat(target); err == nil && staged.Size() == info.Size() {
			return
		}
		data, err := os.ReadFile(source)
		if err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(target, data, 0o755); err != nil {
			t.Fatal(err)
		}
	}
	for _, name := range []string{"SAILRY_ONNXRUNTIME_DIR", "SAILRY_DECISION_RUNTIME"} {
		source := modelsUIEnvironment(t, name)
		entries, err := os.ReadDir(source)
		if err != nil {
			t.Fatal(err)
		}
		for _, entry := range entries {
			if entry.Type().IsRegular() {
				stage(filepath.Join(source, entry.Name()))
			}
		}
	}
	stage(filepath.Join(modelsUIEnvironment(t, "SAILRY_CAPTURE_DIR"), capture.LibraryName()))
}

// modelsUIDesk is a real Node whose Hub requests reach the model hub, a
// Client for the test's own reads and a session to open a window on.
type modelsUIDesk struct {
	t       *testing.T
	ctx     context.Context
	hub     *modelsUIHub
	owner   *node.Node
	profile string
	setup   *client.Client
	session protocol.Session
}

func newModelsUIDesk(t *testing.T, ctx context.Context) *modelsUIDesk {
	t.Helper()
	stageModelRuntimes(t)
	desk := &modelsUIDesk{t: t, ctx: ctx, hub: newModelsUIHub(t), profile: filepath.Join(t.TempDir(), "profile")}
	owner, err := node.StartWithOptions(ctx, desk.profile, node.Options{
		LocalModels: localmodels.Options{Capabilities: node.LocalModelCapabilities(), Transport: desk.hub},
	})
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { owner.Stop(context.Background()) })
	desk.owner = owner
	desk.setup = client.New(link.NewLocal(owner.ID(), owner.ID(), owner))
	if err := desk.setup.Start(ctx); err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { desk.setup.Close() })
	folder, err := filepath.EvalSymlinks(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	project, valid := detail[protocol.Project](desk.do(protocol.Command{Kind: "create_project", Data: protocol.ProjectsDraft{
		Name: "Models workspace", Path: folder, Appearance: protocol.DefaultProjectsAppearance(), Source: protocol.ProjectsSource{Kind: "local"},
	}}).Data)
	if !valid {
		t.Fatal("registered project was not returned")
	}
	// A session needs a model; no request is ever sent to this provider.
	provider := protocol.DefaultConversationProvider()
	provider.ID = protocol.NewProviderID()
	provider.Name, provider.Api = "Models fixture", protocol.ConversationModelApiChatCompletions
	provider.Authentication, provider.Endpoint = protocol.AuthenticationApiKey, "http://127.0.0.1:9/v1"
	provider.Enabled, provider.DefaultModel = true, "fixture"
	model := protocol.DefaultConversationModel()
	model.ID, model.Context, model.Output = "fixture", 200000, 8192
	model.Reasoning, model.DefaultEffort = true, protocol.Effort{Kind: "none"}
	model.Efforts = []protocol.Effort{model.DefaultEffort}
	provider.Models = []protocol.ConversationModel{model}
	secret := protocol.NewSecret("fixture-key")
	desk.do(protocol.Command{Kind: "save_provider", Data: protocol.CommandSaveProviderPayload{Provider: provider, Secret: &secret}})
	config := protocol.DefaultSessionConfig()
	config.Provider, config.Model, config.Effort = provider.ID, model.ID, model.DefaultEffort
	config.Mode, config.Permission = protocol.WorkModeCode, protocol.PermissionFull
	desk.session, valid = detail[protocol.Session](desk.do(protocol.Command{Kind: "create_session", Data: protocol.CommandCreateSessionPayload{Project: &project.ID, Config: &config}}).Data)
	if !valid {
		t.Fatal("created session was not returned")
	}
	return desk
}

func (desk *modelsUIDesk) do(command protocol.Command) protocol.Output {
	desk.t.Helper()
	output, err := desk.setup.Do(desk.ctx, command)
	if err != nil {
		desk.t.Fatalf("%s: %v", command.Kind, err)
	}
	return output
}

// open starts a new Client and Runtime on the Node, as a launch does.
func (desk *modelsUIDesk) open() *terminalUI {
	desk.t.Helper()
	return openTerminalWindow(desk.t, desk.ctx, desk.owner, desk.profile, desk.session, nil)
}

// models are the Node's local models and what it can run.
func (desk *modelsUIDesk) models() protocol.ModelsCatalog {
	desk.t.Helper()
	catalog, valid := detail[protocol.ModelsCatalog](desk.do(protocol.Command{Kind: "read_local_models", Data: protocol.CommandReadLocalModelsPayload{}}).Data)
	if !valid {
		desk.t.Fatal("the Node returned no local models")
	}
	return catalog
}

// model is the Node's model of a source repository and kind.
func (desk *modelsUIDesk) model(kind protocol.ModelsKind, repository string) (protocol.ModelsLocalModel, bool) {
	desk.t.Helper()
	for _, model := range desk.models().Models {
		if modelFormatKind(model.Definition.Format) == kind && model.Definition.Source.Repository == repository {
			return model, true
		}
	}
	return protocol.ModelsLocalModel{}, false
}

func (desk *modelsUIDesk) state(kind protocol.ModelsKind, repository string) string {
	desk.t.Helper()
	model, _ := desk.model(kind, repository)
	return model.State.Kind
}

// installed reports whether the Node holds the model of an ID installed.
func (desk *modelsUIDesk) installed(id string) bool {
	desk.t.Helper()
	return slices.ContainsFunc(desk.models().Models, func(model protocol.ModelsLocalModel) bool {
		return model.ID == id && model.State.Kind == "installed"
	})
}

// choice is the kind's choice as the Node holds it.
func (desk *modelsUIDesk) choice(kind protocol.ModelsKind) protocol.ModelsLocalChoice {
	desk.t.Helper()
	value, valid := detail[protocol.ModelsLocalSettings](desk.do(protocol.Command{Kind: "read_local_model_settings"}).Data)
	if !valid {
		desk.t.Fatal("the Node returned no local model settings")
	}
	return value.Choices[kind]
}

func chose(choice protocol.ModelsLocalChoice, id string) bool {
	return choice.Model != nil && *choice.Model == id
}

// settle builds frames until ready, for at most limit.
func settle(w *terminalUI, label string, limit time.Duration, ready func() bool) {
	w.t.Helper()
	deadline := time.Now().Add(limit)
	for {
		w.frame()
		if ready() {
			return
		}
		if time.Now().After(deadline) || w.ctx.Err() != nil {
			w.t.Fatalf("%s did not settle within %s; visible text: %q; elements: %s", label, limit, w.tester.Texts(), w.elements())
		}
		time.Sleep(10 * time.Millisecond)
	}
}

// shown counts the visible texts that are exactly text.
func shown(w *terminalUI, text string) int {
	count := 0
	for _, visible := range w.tester.Texts() {
		if visible == text {
			count++
		}
	}
	return count
}

// showSection opens a settings page.
func showSection(w *terminalUI, section settings.Section) {
	w.runtime.App.Settings.Open(section)
	w.runtime.App.Navigate(shell.SettingsPage)
	w.frame()
}

// rowAction is the button named name on the row that shows anchor. The
// anchor may also show elsewhere, as a model's name does in its select.
func rowAction(w *terminalUI, anchor, name string) (ui.AccessInfo, bool) {
	buttons := w.named(ui.RoleButton, name)
	var hits []ui.AccessInfo
	for _, row := range w.named(ui.RoleText, anchor) {
		_, y := center(row.Bounds)
		for _, candidate := range buttons {
			same := func(hit ui.AccessInfo) bool { return hit.Bounds == candidate.Bounds }
			if _, cy := center(candidate.Bounds); cy > y-24 && cy < y+24 && !slices.ContainsFunc(hits, same) {
				hits = append(hits, candidate)
			}
		}
	}
	if len(hits) != 1 {
		return ui.AccessInfo{}, false
	}
	return hits[0], true
}

// clickRow waits for the row's enabled button and clicks it.
func clickRow(w *terminalUI, anchor, name string) {
	w.t.Helper()
	var button ui.AccessInfo
	settle(w, fmt.Sprintf("the %q button of the row %q", name, anchor), modelsUIWait, func() bool {
		found, exists := rowAction(w, anchor, name)
		button = found
		return exists && !found.Disabled
	})
	w.clickOn(button)
}

// press clicks the single enabled element named name.
func press(w *terminalUI, name string) {
	w.t.Helper()
	var target ui.AccessInfo
	settle(w, fmt.Sprintf("one enabled %q", name), modelsUIWait, func() bool {
		var found []ui.AccessInfo
		for _, entry := range w.tester.AccessAll() {
			if entry.Name == name && entry.Role != ui.RoleText && entry.Bounds.W > 0 && entry.Bounds.H > 0 {
				found = append(found, entry)
			}
		}
		if len(found) != 1 || found[0].Disabled {
			return false
		}
		target = found[0]
		return true
	})
	w.clickOn(target)
}

// pickOption opens the select named picker and chooses option.
func pickOption(w *terminalUI, picker, option string) {
	w.t.Helper()
	var chooser ui.AccessInfo
	settle(w, fmt.Sprintf("the enabled select %q", picker), modelsUIWait, func() bool {
		found := w.named(ui.RoleMenuButton, picker)
		if len(found) != 1 || found[0].Disabled {
			return false
		}
		chooser = found[0]
		return true
	})
	w.clickOn(chooser)
	var item ui.AccessInfo
	settle(w, fmt.Sprintf("the option %q of %q", option, picker), modelsUIWait, func() bool {
		var found []ui.AccessInfo
		for _, entry := range w.tester.AccessAll() {
			switch entry.Role {
			case ui.RoleMenuItem, ui.RoleMenuItemCheckBox, ui.RoleMenuItemRadio, ui.RoleOption:
				if entry.Name == option && entry.Bounds.W > 0 && entry.Bounds.H > 0 {
					found = append(found, entry)
				}
			}
		}
		if len(found) != 1 {
			return false
		}
		item = found[0]
		return true
	})
	w.clickOn(item)
}

// toggled is the single switch named name.
func toggled(w *terminalUI, name string) (ui.AccessInfo, bool) {
	found := w.named(ui.RoleSwitch, name)
	if len(found) != 1 {
		return ui.AccessInfo{}, false
	}
	return found[0], true
}

func modelsUIDigest(t *testing.T, path string) string {
	t.Helper()
	file, err := os.Open(path)
	if err != nil {
		t.Fatal(err)
	}
	defer file.Close()
	digest := sha256.New()
	if _, err := io.Copy(digest, file); err != nil {
		t.Fatal(err)
	}
	return hex.EncodeToString(digest.Sum(nil))
}

// libraryRow is the library dialog's row of a repository and the text of
// its detail line, which anchors its buttons.
func libraryRow(w *terminalUI, repository string) (viewmodel.LibraryModel, string, bool) {
	rows := w.runtime.App.Settings.Library
	index := slices.IndexFunc(rows, func(row viewmodel.LibraryModel) bool { return row.Repo == repository })
	if index < 0 {
		return viewmodel.LibraryModel{}, "", false
	}
	for _, text := range w.tester.Texts() {
		if strings.HasPrefix(text, repository+" · ") {
			return rows[index], text, true
		}
	}
	return rows[index], "", false
}

// clickLibraryRow clicks a button of the library row of a repository. The
// row's detail line changes as the model is registered, so it is read anew.
func clickLibraryRow(w *terminalUI, repository, name string) {
	w.t.Helper()
	var button ui.AccessInfo
	settle(w, fmt.Sprintf("the %q button of the library row %q", name, repository), modelsUIWait, func() bool {
		_, anchor, visible := libraryRow(w, repository)
		if !visible {
			return false
		}
		found, exists := rowAction(w, anchor, name)
		button = found
		return exists && !found.Disabled
	})
	w.clickOn(button)
}

// searchLibrary opens the page's model library, which lists the hub's
// models of the page's kind, and searches the hub for query.
func searchLibrary(w *terminalUI, hub *modelsUIHub, query string) {
	w.t.Helper()
	press(w, "Manage models")
	settle(w, "the model library dialog", modelsUIWait, func() bool {
		return w.runtime.App.Settings.LibraryRequest().Open && w.runtime.App.Settings.LibraryLoad.Loaded
	})
	w.replace("Search models", query)
	press(w, "Search")
	settle(w, fmt.Sprintf("the hub search for %q", query), modelsUIWait, func() bool {
		return len(hub.seen("search:"+query)) > 0 && w.runtime.App.Settings.LibraryLoad.Loaded
	})
}

// TestModelSettingsUI is the B11 acceptance of Settings › Knowledge, OCR and
// Decisions and of the model library dialog, against a real Node that runs
// the real ONNX Runtime and llama.cpp engines.
func TestModelSettingsUI(t *testing.T) {
	ctx, cancel := context.WithTimeout(t.Context(), 900*time.Second)
	defer cancel()
	desk := newModelsUIDesk(t, ctx)
	hub := desk.hub
	catalog := desk.models()
	for _, kind := range []protocol.ModelsKind{protocol.ModelsKindEmbedding, protocol.ModelsKindOcr, protocol.ModelsKindDecision} {
		if catalog.Availability[kind].Kind != "available" {
			t.Fatalf("the Node cannot run %s with the runtimes beside the test binary: %+v", kind, catalog.Availability[kind])
		}
	}
	ocrDefinition := localmodels.OCR()[0]
	var embedding, recognition protocol.ModelsLocalModel
	for _, model := range catalog.Models {
		switch model.ID {
		case localmodels.DefaultEmbedding:
			embedding = model
		case localmodels.ID(&ocrDefinition):
			recognition = model
		}
	}
	if embedding.ID == "" || recognition.ID == "" || embedding.State.Kind != "missing" || recognition.State.Kind != "missing" {
		t.Fatalf("managed models before installation: %+v, %+v", embedding, recognition)
	}
	hub.definition(embedding.Definition, modelsUIEnvironment(t, "SAILRY_EMBEDDING_MODEL_DIR"))
	hub.definition(recognition.Definition, modelsUIEnvironment(t, "SAILRY_OCR_MODEL_DIR"))
	julia := modelsUIEnvironment(t, "SAILRY_DECISION_JULIA")
	juliaInfo, err := os.Stat(julia)
	if err != nil {
		t.Fatal(err)
	}
	juliaSource := protocol.ModelsSource{Hub: protocol.ModelsHubHuggingFace, Repository: modelsUIJuliaRepository, Revision: modelsUIJuliaRevision}
	juliaKey := modelsUIHubKey(juliaSource, filepath.Base(julia))
	hub.add(juliaKey, julia)
	hub.mu.Lock()
	hub.repositories[modelsUIJuliaRepository] = modelsUIHubRepository{commit: modelsUIJuliaRevision, license: "apache-2.0",
		path: filepath.Base(julia), local: julia, sha256: modelsUIDigest(t, julia)}
	hub.mu.Unlock()

	window := desk.open()
	defer func() { window.close() }()
	app := window.runtime.App
	embeddingRepository, embeddingName := embedding.Definition.Source.Repository, embedding.Definition.Name
	recognitionRepository, recognitionName := recognition.Definition.Source.Repository, recognition.Definition.Name

	// (1) Knowledge: the managed embedding model is offered, its download
	// shows a percent, cancels, resumes and installs.
	showSection(window, settings.Knowledge)
	settle(window, "the Knowledge page reads the Node", modelsUIWait, func() bool {
		return !app.Settings.Knowledge.Enabled.Pending && shown(window, "No installed models") == 1
	})
	if len(app.Settings.Knowledge.Installed()) != 0 {
		t.Fatalf("installed embedding models before installation: %+v", app.Settings.Knowledge.Installed())
	}
	press(window, "Available")
	var graph protocol.ModelsAsset
	for _, asset := range embedding.Definition.Assets {
		if asset.Bytes > graph.Bytes {
			graph = asset
		}
	}
	graphKey := modelsUIHubKey(graph.Source, graph.Path)
	hub.stall(graphKey, int64(graph.Bytes/2))
	clickRow(window, embeddingRepository, "Install")
	hub.awaitStall(graphKey)
	percent := ""
	settle(window, "the embedding download's percent", modelsUIWait, func() bool {
		for _, row := range app.Settings.Knowledge.Catalog {
			if row.ID == embedding.ID && row.Status == viewmodel.ModelDownloading && row.Percent > 0 && row.Percent < 100 {
				percent = fmt.Sprintf("%.0f%%", row.Percent)
			}
		}
		return percent != "" && shown(window, percent) == 1
	})
	clickRow(window, embeddingRepository, "Cancel")
	requested := false
	settle(window, "the cancelled embedding download", modelsUIWait, func() bool {
		requested = requested || shown(window, "Cancellation requested") > 0
		_, installable := rowAction(window, embeddingRepository, "Install")
		return requested && installable && desk.state(protocol.ModelsKindEmbedding, embeddingRepository) == "missing"
	})
	hub.stall(graphKey, 0)
	clickRow(window, embeddingRepository, "Install")
	saved := false
	settle(window, "the embedding model installs", modelsUIInstallWait, func() bool {
		saved = saved || shown(window, "Saved") > 0
		return saved && desk.state(protocol.ModelsKindEmbedding, embeddingRepository) == "installed" && shown(window, embeddingRepository) == 0
	})
	if requests := hub.seen(graphKey); len(requests) != 2 || requests[0].ranged != "" || requests[1].ranged == "" || requests[1].host != "huggingface.co" {
		t.Fatalf("the resumed embedding download asked %+v", requests)
	}
	t.Logf("step 1 passed: Knowledge › Available installed %s; the row showed %s, Cancel kept the partial file and the second install resumed it", embeddingName, percent)

	// (2) Knowledge: the installed model shows, and the picker and Select
	// save the default.
	press(window, "Installed")
	settle(window, "the installed embedding row", modelsUIWait, func() bool {
		installed := app.Settings.Knowledge.Installed()
		return len(installed) == 1 && installed[0].ID == embedding.ID && shown(window, embeddingRepository) == 1
	})
	if desk.choice(protocol.ModelsKindEmbedding).Enabled {
		pickOption(window, "Vector model", "Keyword only")
	}
	settle(window, "the Node saves keyword-only knowledge", modelsUIWait, func() bool {
		return !desk.choice(protocol.ModelsKindEmbedding).Enabled && app.Settings.Knowledge.Model == -1 && !app.Settings.Knowledge.Enabled.Pending
	})
	clickRow(window, embeddingRepository, "Select")
	settle(window, "the Node saves the selected embedding model", modelsUIWait, func() bool {
		choice := desk.choice(protocol.ModelsKindEmbedding)
		_, selected := rowAction(window, embeddingRepository, "Selected")
		return choice.Enabled && chose(choice, embedding.ID) && app.Settings.Knowledge.Model == 0 && selected
	})
	pickOption(window, "Vector model", "Keyword only")
	settle(window, "the picker saves keyword-only knowledge", modelsUIWait, func() bool {
		return !desk.choice(protocol.ModelsKindEmbedding).Enabled && app.Settings.Knowledge.Model == -1 && !app.Settings.Knowledge.Enabled.Pending
	})
	pickOption(window, "Vector model", embeddingName)
	settle(window, "the picker saves the embedding model", modelsUIWait, func() bool {
		choice := desk.choice(protocol.ModelsKindEmbedding)
		return choice.Enabled && chose(choice, embedding.ID) && app.Settings.Knowledge.Model == 0 && !app.Settings.Knowledge.Enabled.Pending
	})
	t.Log("step 2 passed: the Knowledge picker and the row's Select saved keyword-only and the vector model on the Node")

	// (3) OCR: the managed model installs from ModelScope; Select and the
	// switch save.
	showSection(window, settings.OCR)
	settle(window, "the OCR page", modelsUIWait, func() bool {
		return !app.Settings.OCR.Enabled.Pending && shown(window, "No installed models") == 1
	})
	press(window, "Available")
	clickRow(window, recognitionName, "Install")
	saved = false
	settle(window, "the OCR model installs", modelsUIInstallWait, func() bool {
		saved = saved || shown(window, "Saved") > 0
		return saved && desk.installed(recognition.ID) && shown(window, recognitionName) == 0
	})
	for _, asset := range recognition.Definition.Assets {
		if requests := hub.seen(modelsUIHubKey(asset.Source, asset.Path)); len(requests) != 1 || requests[0].host != "modelscope.cn" {
			t.Fatalf("requests for %s = %+v", asset.Path, requests)
		}
	}
	press(window, "Installed")
	settle(window, "the installed OCR row", modelsUIWait, func() bool {
		installed := app.Settings.OCR.Installed()
		return len(installed) == 1 && installed[0].ID == recognition.ID && shown(window, recognitionRepository) == 1
	})
	if !chose(desk.choice(protocol.ModelsKindOcr), recognition.ID) {
		clickRow(window, recognitionName, "Select")
	}
	settle(window, "the Node saves the selected OCR model", modelsUIWait, func() bool {
		_, selected := rowAction(window, recognitionName, "Selected")
		return chose(desk.choice(protocol.ModelsKindOcr), recognition.ID) && app.Settings.OCR.Model == 0 && selected && !app.Settings.OCR.Enabled.Pending
	})
	ocrEnabled := desk.choice(protocol.ModelsKindOcr).Enabled
	guard, found := toggled(window, "Enable OCR")
	if !found || guard.Disabled || guard.Checked != ocrEnabled {
		t.Fatalf("the OCR switch before the change: %+v, Node enabled %v", guard, ocrEnabled)
	}
	window.clickOn(guard)
	settle(window, "the Node saves the OCR switch", modelsUIWait, func() bool {
		guard, found := toggled(window, "Enable OCR")
		choice := desk.choice(protocol.ModelsKindOcr)
		return found && guard.Checked == !ocrEnabled && choice.Enabled == !ocrEnabled && chose(choice, recognition.ID) && !app.Settings.OCR.Enabled.Pending
	})
	t.Logf("step 3 passed: OCR › Available installed %s from modelscope.cn, Select saved it and the switch saved enabled=%v", recognitionName, !ocrEnabled)

	// (4) Decisions: the library searches the hub, shows the result,
	// downloads with progress, cancels, resumes and installs.
	showSection(window, settings.Decision)
	settle(window, "the Decisions page", modelsUIWait, func() bool {
		return !app.Settings.Decision.Enabled.Pending && shown(window, "No installed models") == 1
	})
	searchLibrary(window, hub, "julia")
	settle(window, "the hub's result", modelsUIWait, func() bool {
		row, _, visible := libraryRow(window, modelsUIJuliaRepository)
		return visible && row.Status == viewmodel.ModelMissing && len(app.Settings.Library) == 1
	})
	if requests := hub.seen("search:julia"); len(requests) == 0 || requests[0].host != "huggingface.co" {
		t.Fatalf("the library search asked %+v", requests)
	}
	hub.stall(juliaKey, juliaInfo.Size()/2)
	clickLibraryRow(window, modelsUIJuliaRepository, "Install")
	hub.awaitStall(juliaKey)
	var downloading viewmodel.LibraryModel
	settle(window, "the library download's progress", modelsUIWait, func() bool {
		row, _, visible := libraryRow(window, modelsUIJuliaRepository)
		downloading = row
		return visible && row.Status == viewmodel.ModelDownloading && row.Percent > 0 && row.Percent < 100 && shown(window, "Downloading") == 1
	})
	clickLibraryRow(window, modelsUIJuliaRepository, "Cancel")
	requested = false
	settle(window, "the cancelled library download", modelsUIWait, func() bool {
		requested = requested || shown(window, "Cancellation requested") > 0
		row, _, visible := libraryRow(window, modelsUIJuliaRepository)
		return requested && visible && row.Status == viewmodel.ModelMissing && desk.state(protocol.ModelsKindDecision, modelsUIJuliaRepository) == "missing"
	})
	hub.stall(juliaKey, 0)
	clickLibraryRow(window, modelsUIJuliaRepository, "Install")
	saved = false
	var installedRow viewmodel.LibraryModel
	settle(window, "the decision model installs", modelsUIInstallWait, func() bool {
		saved = saved || shown(window, "Saved") > 0
		row, _, visible := libraryRow(window, modelsUIJuliaRepository)
		installedRow = row
		return saved && visible && row.Status == viewmodel.ModelInstalled && desk.state(protocol.ModelsKindDecision, modelsUIJuliaRepository) == "installed"
	})
	if requests := hub.seen(juliaKey); len(requests) != 2 || requests[0].ranged != "" || requests[1].ranged == "" || requests[1].host != "huggingface.co" {
		t.Fatalf("the resumed decision download asked %+v", requests)
	}
	if installedRow.Size == "" || installedRow.License != "apache-2.0" {
		t.Fatalf("the installed library row = %+v", installedRow)
	}
	decision, _ := desk.model(protocol.ModelsKindDecision, modelsUIJuliaRepository)
	t.Logf("step 4 passed: the library found %s on huggingface.co, its download showed the progress bar at %.0f%%, Cancel stopped it and the second install resumed and installed %s (%s)",
		modelsUIJuliaRepository, downloading.Percent, decision.ID, installedRow.Size)

	// (5) Decisions: the switch and the picker save.
	press(window, "Close")
	settle(window, "the library closes", modelsUIWait, func() bool { return !app.Settings.LibraryRequest().Open })
	var decisionName string
	settle(window, "the installed decision row", modelsUIWait, func() bool {
		installed := app.Settings.Decision.Installed()
		if len(installed) != 1 || installed[0].ID != decision.ID {
			return false
		}
		decisionName = installed[0].Name
		_, selectable := rowAction(window, modelsUIJuliaRepository, "Select")
		return selectable && !app.Settings.Decision.Enabled.Pending
	})
	decisionEnabled := desk.choice(protocol.ModelsKindDecision).Enabled
	// The switch is left on, which the picker needs: from on it goes off
	// and on again, and each change is saved.
	flips := []bool{true}
	if decisionEnabled {
		flips = []bool{false, true}
	}
	for _, want := range flips {
		guard, found = toggled(window, "Enable decisions")
		if !found || guard.Disabled || guard.Checked == want {
			t.Fatalf("the decision switch before turning %v: %+v", want, guard)
		}
		window.clickOn(guard)
		settle(window, fmt.Sprintf("the Node saves the decision switch as %v", want), modelsUIWait, func() bool {
			guard, found := toggled(window, "Enable decisions")
			return found && guard.Checked == want && !guard.Disabled && desk.choice(protocol.ModelsKindDecision).Enabled == want && !app.Settings.Decision.Enabled.Pending
		})
	}
	if !chose(desk.choice(protocol.ModelsKindDecision), decision.ID) {
		pickOption(window, "Model", decisionName)
	}
	settle(window, "the Node saves the decision model", modelsUIWait, func() bool {
		choice := desk.choice(protocol.ModelsKindDecision)
		_, selected := rowAction(window, modelsUIJuliaRepository, "Selected")
		return choice.Enabled && chose(choice, decision.ID) && app.Settings.Decision.Model == 0 && selected
	})
	t.Logf("step 5 passed: the Decisions switch saved %v in turn and the picker saved %s on the Node", flips, decisionName)

	// (6) A new Runtime shows what was saved.
	window.close()
	window = desk.open()
	app = window.runtime.App
	showSection(window, settings.Knowledge)
	settle(window, "the relaunched Knowledge page", modelsUIWait, func() bool {
		page := app.Settings.Knowledge
		installed := page.Installed()
		_, selected := rowAction(window, embeddingRepository, "Selected")
		return !page.Enabled.Pending && len(installed) == 1 && page.Model == 0 && installed[0].ID == embedding.ID && selected && shown(window, embeddingName) == 2
	})
	showSection(window, settings.OCR)
	settle(window, "the relaunched OCR page", modelsUIWait, func() bool {
		page := app.Settings.OCR
		installed := page.Installed()
		guard, found := toggled(window, "Enable OCR")
		_, selected := rowAction(window, recognitionName, "Selected")
		return !page.Enabled.Pending && len(installed) == 1 && page.Model == 0 && installed[0].ID == recognition.ID && selected && found && guard.Checked == !ocrEnabled
	})
	showSection(window, settings.Decision)
	settle(window, "the relaunched Decisions page", modelsUIWait, func() bool {
		page := app.Settings.Decision
		installed := page.Installed()
		guard, found := toggled(window, "Enable decisions")
		_, selected := rowAction(window, modelsUIJuliaRepository, "Selected")
		return !page.Enabled.Pending && len(installed) == 1 && page.Model == 0 && installed[0].ID == decision.ID && selected && found && guard.Checked
	})
	t.Log("step 6 passed: a new Runtime showed the installed models, the saved switches and the saved pickers of all three pages")

	// (7) The library deletes after a confirmation that names the model.
	searchLibrary(window, hub, "julia")
	var listed viewmodel.LibraryModel
	settle(window, "the installed result", modelsUIWait, func() bool {
		row, _, visible := libraryRow(window, modelsUIJuliaRepository)
		listed = row
		return visible && row.Status == viewmodel.ModelInstalled
	})
	named := shown(window, listed.Name)
	_, anchor, _ := libraryRow(window, modelsUIJuliaRepository)
	remove, found := rowAction(window, anchor, "Delete")
	if !found {
		t.Fatalf("the installed library row has no Delete button; elements: %s", window.elements())
	}
	window.clickOn(remove)
	var confirm ui.AccessInfo
	settle(window, "the delete confirmation", modelsUIWait, func() bool {
		var others []ui.AccessInfo
		for _, candidate := range window.named(ui.RoleButton, "Delete") {
			if candidate.Bounds != remove.Bounds {
				others = append(others, candidate)
			}
		}
		if len(others) != 1 || shown(window, listed.Name) != named+1 {
			return false
		}
		confirm = others[0]
		return true
	})
	if desk.state(protocol.ModelsKindDecision, modelsUIJuliaRepository) != "installed" {
		t.Fatal("the model was deleted before the confirmation")
	}
	window.clickOn(confirm)
	deleted := false
	settle(window, "the deleted decision model", modelsUIWait, func() bool {
		deleted = deleted || shown(window, "Deleted") > 0
		row, _, visible := libraryRow(window, modelsUIJuliaRepository)
		return deleted && visible && row.Status == viewmodel.ModelMissing && desk.state(protocol.ModelsKindDecision, modelsUIJuliaRepository) != "installed"
	})
	if len(app.Settings.Decision.Installed()) != 0 {
		t.Fatalf("the Decisions page still lists %+v", app.Settings.Decision.Installed())
	}
	t.Logf("step 7 passed: Delete asked once naming %q, and the confirmation deleted the model with the Deleted toast", listed.Name)
}

// fixtureWave reads a mono 16-bit PCM WAVE file.
func fixtureWave(t *testing.T, path string) ([]float32, uint32) {
	t.Helper()
	content, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	if len(content) < 12 || string(content[:4]) != "RIFF" || string(content[8:12]) != "WAVE" {
		t.Fatalf("%s is not a WAVE file", path)
	}
	var (
		rate    uint32
		samples []float32
	)
	for offset := 12; offset+8 <= len(content); {
		size := int(binary.LittleEndian.Uint32(content[offset+4:]))
		body := content[offset+8 : min(offset+8+size, len(content))]
		switch string(content[offset : offset+4]) {
		case "fmt ":
			if len(body) < 16 || binary.LittleEndian.Uint16(body) != 1 || binary.LittleEndian.Uint16(body[2:]) != 1 || binary.LittleEndian.Uint16(body[14:]) != 16 {
				t.Fatalf("%s is not mono 16-bit PCM", path)
			}
			rate = binary.LittleEndian.Uint32(body[4:])
		case "data":
			for index := 0; index+2 <= len(body); index += 2 {
				samples = append(samples, float32(int16(binary.LittleEndian.Uint16(body[index:])))/32768)
			}
		}
		offset += 8 + size + size%2
	}
	if rate == 0 || len(samples) == 0 {
		t.Fatalf("%s holds no audio", path)
	}
	return samples, rate
}

// fixtureMicrophone stands for the capture device: it delivers the samples
// the test feeds and nothing else. No system microphone is opened.
type fixtureMicrophone struct {
	mu      sync.Mutex
	rate    uint32
	pending []float32
	opened  int
	stopped int
	closed  int
}

func (m *fixtureMicrophone) feed(samples []float32) {
	m.mu.Lock()
	m.pending = append(m.pending, samples...)
	m.mu.Unlock()
}

func (m *fixtureMicrophone) counts() (opened, stopped, closed, pending int) {
	m.mu.Lock()
	defer m.mu.Unlock()
	return m.opened, m.stopped, m.closed, len(m.pending)
}

func (m *fixtureMicrophone) open(*string) (dictationSource, error) {
	m.mu.Lock()
	defer m.mu.Unlock()
	m.opened++
	return dictationSource{
		rate: m.rate, channels: 1,
		read: func(buffer []float32) int {
			m.mu.Lock()
			defer m.mu.Unlock()
			frames := copy(buffer, m.pending)
			m.pending = m.pending[frames:]
			return frames
		},
		failed: func() bool { return false },
		stop:   func() { m.mu.Lock(); m.stopped++; m.mu.Unlock() },
		close:  func() { m.mu.Lock(); m.closed++; m.mu.Unlock() },
	}, nil
}

// TestDictationUI is the B11 acceptance of Settings › Voice input and of the
// composer's microphone button: the recommended speech model installs from
// the hub into the desktop data directory, Select persists, and a dictation
// prepares, records, transcribes and inserts with the real sherpa-onnx
// recognizer. The microphone is the fixture's.
func TestDictationUI(t *testing.T) {
	ctx, cancel := context.WithTimeout(t.Context(), 600*time.Second)
	defer cancel()
	desk := newModelsUIDesk(t, ctx)
	hub := desk.hub
	speechDirectory := modelsUIEnvironment(t, "SAILRY_SPEECH_MODEL_DIR")
	source := protocol.ModelsSource{Hub: protocol.ModelsHubHuggingFace, Repository: modelsUISpeechRepository, Revision: modelsUISpeechRevision}
	files := []string{"model.int8.onnx", "tokens.txt"}
	for _, name := range files {
		hub.add(modelsUIHubKey(source, name), filepath.Join(speechDirectory, name))
	}
	samples, rate := fixtureWave(t, filepath.Join(speechDirectory, "test_wavs", "en.wav"))

	// The stored selection names a model that is not registered, so that
	// the recommended model is not the selected one until Select is used.
	path := filepath.Join(desk.profile, "desktop", "preferences.json")
	unknown := protocol.NewModelID()
	seed := preferences.Open(path)
	seed.Data.Dictation = &preferences.Dictation{Language: preferences.DictationLanguages[0], Model: &unknown}
	if err := seed.Save(); err != nil {
		t.Fatal(err)
	}

	window := desk.open()
	defer func() { window.close() }()
	runtime := window.runtime
	app := runtime.App
	if stored := runtime.Preferences.Data.DictationModel(); stored == nil || *stored != unknown {
		t.Fatalf("the launched window read the speech selection %v", stored)
	}
	data := filepath.Dir(runtime.Preferences.Path)
	// The manager is production's except for its transport, which reaches
	// the hub instead of the network.
	manager, err := models.Open(runtime.ctx, models.Options{Directory: data, Transport: hub, Validate: validateSpeechModel, Changed: runtime.wake})
	if err != nil {
		t.Fatal(err)
	}
	runtime.dictation.manager = manager

	// (1) The page lists the recommended model as not installed.
	const recommended = "SenseVoice INT8"
	showSection(window, settings.Dictation)
	settle(window, "the Voice input page", modelsUIWait, func() bool {
		return shown(window, "No installed models") == 1 && len(app.Settings.Speech) == 1 && !runtime.dictation.page.checking
	})
	if row := app.Settings.Speech[0]; row.ID != "" || row.Status != viewmodel.ModelMissing || app.Settings.SpeechModel != -1 {
		t.Fatalf("the speech rows before installation: %+v, selected %d", app.Settings.Speech, app.Settings.SpeechModel)
	}
	press(window, "Available")
	settle(window, "the recommended speech model", modelsUIWait, func() bool { return shown(window, recommended) == 1 })

	// (2) Install downloads both files from the hub into the desktop data
	// directory, shows a percent and validates with the real recognizer.
	// The graph's first response stops half way, so that the percent holds
	// still to be read; Cancel then ends that download and the second
	// Install resumes it.
	graphKey := modelsUIHubKey(source, files[0])
	graphInfo, err := os.Stat(filepath.Join(speechDirectory, files[0]))
	if err != nil {
		t.Fatal(err)
	}
	hub.stall(graphKey, graphInfo.Size()/2)
	clickRow(window, recommended, "Install")
	hub.awaitStall(graphKey)
	percent := ""
	settle(window, "the speech download's percent", modelsUIWait, func() bool {
		if row := app.Settings.Speech[0]; row.Status == viewmodel.ModelDownloading && row.Percent > 0 && row.Percent < 100 {
			percent = fmt.Sprintf("%.0f%%", row.Percent)
		}
		return percent != "" && shown(window, percent) == 1
	})
	clickRow(window, recommended, "Cancel")
	settle(window, "the cancelled speech download", modelsUIWait, func() bool {
		state, known := manager.RecommendedState()
		_, installable := rowAction(window, recommended, "Install")
		return installable && (!known || state.Status != models.Downloading) && app.Settings.Speech[0].Status == viewmodel.ModelMissing
	})
	hub.stall(graphKey, 0)
	notices := shown(window, "Installed")
	clickRow(window, recommended, "Install")
	toasted := false
	settle(window, "the recommended speech model installs", modelsUIInstallWait, func() bool {
		toasted = toasted || shown(window, "Installed") > notices
		state, known := manager.RecommendedState()
		return known && state.Status == models.Installed && toasted && shown(window, "No installed models") == 1
	})
	if requests := hub.seen(graphKey); len(requests) != 2 || requests[0].ranged != "" || requests[1].ranged == "" {
		t.Fatalf("the resumed speech download asked %+v", requests)
	}
	for _, name := range files {
		requests := hub.seen(modelsUIHubKey(source, name))
		if len(requests) == 0 || slices.ContainsFunc(requests, func(request modelsUIHubRequest) bool { return request.host != "huggingface.co" }) {
			t.Fatalf("requests for %s = %+v", name, requests)
		}
	}
	installed := map[string]string{}
	if err := filepath.WalkDir(data, func(entry string, info fs.DirEntry, err error) error {
		if err == nil && info.Type().IsRegular() && slices.Contains(files, info.Name()) {
			installed[info.Name()] = entry
		}
		return err
	}); err != nil {
		t.Fatal(err)
	}
	for _, name := range files {
		if installed[name] == "" || modelsUIDigest(t, installed[name]) != modelsUIDigest(t, filepath.Join(speechDirectory, name)) {
			t.Fatalf("%s is not installed under %s: %v", name, data, installed)
		}
	}
	t.Logf("step 1 passed: Voice input › Available installed %s into %s; the row showed %s, Cancel stopped the download, the second Install resumed it and ended with the Installed toast", recommended, filepath.Dir(installed[files[0]]), percent)

	// (3) Select stores the recommended model, and a new Runtime shows it.
	press(window, "Installed")
	clickRow(window, recommended, "Select")
	settle(window, "the selection is written to the preferences", modelsUIWait, func() bool {
		_, selected := rowAction(window, recommended, "Selected")
		stored := preferences.Open(path).Data
		return selected && app.Settings.SpeechModel == 0 && stored.Dictation != nil && stored.Dictation.Model == nil
	})
	manager.Wait()
	window.close()
	window = desk.open()
	runtime, app = window.runtime, window.runtime.App
	showSection(window, settings.Dictation)
	settle(window, "the relaunched Voice input page", modelsUIWait, func() bool {
		_, selected := rowAction(window, recommended, "Selected")
		return selected && app.Settings.SpeechModel == 0 && len(app.Settings.Speech) == 1 && app.Settings.Speech[0].Status == viewmodel.ModelInstalled
	})
	if runtime.Preferences.Data.DictationModel() != nil {
		t.Fatalf("the relaunched window selects %v", runtime.Preferences.Data.DictationModel())
	}
	t.Log("step 2 passed: Select stored the recommended model and a new Runtime, with its own manager, verified the files and showed it Selected")

	// (4) The composer's microphone button. The recognizer is the system's;
	// the permission answer and the microphone are the fixture's.
	microphone := &fixtureMicrophone{rate: rate}
	permit := make(chan struct{})
	backend := systemDictation()
	backend.permission = func(ctx context.Context) (bool, error) {
		select {
		case <-permit:
			return true, nil
		case <-ctx.Done():
			return false, ctx.Err()
		}
	}
	backend.open = microphone.open
	// The system's recognizer runs every decode; the test only notes them.
	var (
		decoding sync.Mutex
		lengths  []int
		heard    []string
	)
	system := backend.recognizer
	backend.recognizer = func(graph, tokens string, language speech.Language) (dictationRecognizer, error) {
		recognizer, err := system(graph, tokens, language)
		if err != nil {
			return recognizer, err
		}
		recognize := recognizer.recognize
		recognizer.recognize = func(audio []float32) (string, error) {
			text, err := recognize(audio)
			decoding.Lock()
			lengths, heard = append(lengths, len(audio)), append(heard, text)
			decoding.Unlock()
			return text, err
		}
		return recognizer, nil
	}
	runtime.dictation.backend = backend
	app.Navigate(shell.ConversationPage)
	comp := app.Composer(desk.session.ID.String())
	dictation := &comp.Dictation
	idle := func() bool { return runtime.dictation.active == nil && dictation.Phase == composer.Idle }
	settle(window, "the composer's microphone button", modelsUIWait, func() bool {
		found := window.named(ui.RoleButton, "Voice input")
		return len(found) == 1 && !found[0].Disabled && idle()
	})

	// A click while the model prepares cancels: nothing is recorded.
	press(window, "Voice input")
	settle(window, "the preparing dictation", modelsUIWait, func() bool {
		return dictation.Phase == composer.PreparingSpeech && len(window.named(ui.RoleButton, "Preparing local speech model")) == 1
	})
	press(window, "Preparing local speech model")
	if dictation.Phase != composer.Idle {
		t.Fatalf("a click while preparing left phase %d", dictation.Phase)
	}
	settle(window, "the cancelled dictation's worker", modelsUIWait, idle)
	if opened, _, _, _ := microphone.counts(); opened != 0 || comp.Draft != "" {
		t.Fatalf("a cancelled preparation opened the microphone %d times and left the draft %q", opened, comp.Draft)
	}
	t.Log("step 3 passed: a click while Preparing cancelled the dictation before the microphone opened")

	// Prepare, record, transcribe, insert.
	close(permit)
	press(window, "Voice input")
	settle(window, "the recording dictation", modelsUIInstallWait, func() bool {
		return dictation.Phase == composer.Recording && len(window.named(ui.RoleButton, "Finish recording")) == 1
	})
	// The first two seconds give a partial transcript; the rest follows.
	head := min(2*int(rate), len(samples))
	microphone.feed(samples[:head])
	settle(window, "a partial transcript in the draft", modelsUIWait, func() bool { return comp.Draft != "" && len(dictation.Levels) > 0 })
	partial := comp.Draft
	if partial == modelsUITranscript {
		t.Fatalf("two seconds of the fixture already gave the whole transcript %q", partial)
	}
	microphone.feed(samples[head:])
	settle(window, "the recorded fixture", modelsUIWait, func() bool {
		_, _, _, pending := microphone.counts()
		return pending == 0
	})
	press(window, "Finish recording")
	// The final decode of the short fixture can end within the click's own
	// frame, so Transcribing is noted when it is seen and not required.
	transcribing := dictation.Phase == composer.Transcribing
	settle(window, "the transcribed dictation", modelsUIWait, idle)
	if comp.Draft != modelsUITranscript {
		t.Fatalf("the draft is %q, want %q; partial %q; visible text: %q", comp.Draft, modelsUITranscript, partial, window.tester.Texts())
	}
	// The recording was finished, not cancelled: the microphone was stopped
	// and the last decode heard the whole fixture.
	decoding.Lock()
	decodes, last, final := len(lengths), lengths[len(lengths)-1], heard[len(heard)-1]
	decoding.Unlock()
	if rate != speech.SampleRate || decodes < 2 || last != len(samples) || final != modelsUITranscript {
		t.Fatalf("%d decodes at %d Hz; the last heard %d of %d samples as %q", decodes, rate, last, len(samples), final)
	}
	settle(window, "the transcript in the composer", modelsUIWait, func() bool {
		return slices.ContainsFunc(window.tester.AccessAll(), func(entry ui.AccessInfo) bool { return entry.Value == modelsUITranscript }) ||
			shown(window, modelsUITranscript) > 0
	})
	if opened, stopped, closed, _ := microphone.counts(); opened != 1 || stopped != 1 || closed != 1 {
		t.Fatalf("the microphone was opened %d, stopped %d and closed %d times", opened, stopped, closed)
	}
	for _, failure := range []string{"Local voice input unavailable", "Transcription failed; retry", "No speech recognized", "Draft changed; voice input stopped"} {
		if shown(window, failure) > 0 {
			t.Fatalf("the dictation toasted %q", failure)
		}
	}
	t.Logf("step 4 passed: the button prepared, recorded %d samples at %d Hz, inserted the partial %q and after %d decodes ended with the exact transcript in the draft (Transcribing seen after the click: %v)", len(samples), rate, partial, decodes, transcribing)
}
