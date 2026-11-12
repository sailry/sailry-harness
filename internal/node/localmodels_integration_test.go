//go:build integration

package node

import (
	"bytes"
	"context"
	"crypto/rand"
	"crypto/sha256"
	"database/sql"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"math"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"reflect"
	"slices"
	"strconv"
	"strings"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	"charm.land/fantasy"
	"github.com/jonboulle/clockwork"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/localmodels"
	"github.com/sailry/sailry-harness/internal/localmodels/ocr"
	"github.com/sailry/sailry-harness/internal/localmodels/onnx"
	"github.com/sailry/sailry-harness/internal/node/agent/model"
	"github.com/sailry/sailry-harness/internal/node/agent/operations"
	"github.com/sailry/sailry-harness/internal/node/process"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
	"lukechampine.com/blake3"
)

// The acceptance tests drive local models through Node.Dispatch with the
// pinned native runtimes and the real model files. A loopback TLS server
// stands in for Hugging Face and ModelScope: the Node's outbound transport
// sends every Hub request to it, and it serves the real files.
//
//	SAILRY_ONNXRUNTIME_DIR      pinned ONNX Runtime library
//	SAILRY_EMBEDDING_MODEL_DIR  granite-multi: config.json, tokenizer.json, model.onnx
//	SAILRY_OCR_MODEL_DIR        PP-OCRv6 tiny files under their asset names
//	SAILRY_LOCALMODELS_FIXTURES documents and their recorded reference text
//	SAILRY_DECISION_RUNTIME     packaged llama.cpp runtime with llama-runtime.json
//	SAILRY_DECISION_FIXTURES    requests.json, golden.json and oracle-inputs.json
//	SAILRY_DECISION_JULIA       pinned Julia decision model
//
// The ONNX Runtime is loaded from SAILRY_ONNXRUNTIME_DIR, so the packaged
// location check of embedding and OCR is bypassed as in ocrNode. The decision
// runtime has no such loader: its files are copied beside the test binary,
// which is the only place the production code reads them from.

const (
	// localModelWait bounds every wait that does not load a real model.
	localModelWait = 15 * time.Second
	// localModelInstallWait bounds an installation: it transfers, hashes and
	// loads up to 415 MB of real model files into a native runtime.
	localModelInstallWait = 180 * time.Second

	juliaRepository = "ggml-org/Julia-1-GGUF"
	juliaRevision   = "1e6b48a6983d7467f8eedc2cb0d21267b5fc052d"
)

func requiredEnvironment(t *testing.T, name string) string {
	t.Helper()
	value := os.Getenv(name)
	if value == "" {
		t.Fatalf("%s must be set", name)
	}
	return value
}

// hubFile is one real file the model hub serves.
type hubFile struct {
	local string
	size  int64
}

type hubSibling struct {
	path   string
	local  string
	sha256 string
}

type hubRepository struct {
	commit   string
	license  string
	siblings []hubSibling
}

type hubRequest struct {
	host, key, ranged string
}

// modelHub is a loopback stand-in for both model hubs.
type modelHub struct {
	t      *testing.T
	server *httptest.Server
	done   chan struct{}
	// stalled receives the key of a response that stopped part way.
	stalled chan string

	mu           sync.Mutex
	files        map[string]hubFile
	repositories map[string]hubRepository
	stalls       map[string]int64
	corrupt      map[string]bool
	requests     []hubRequest
}

func hubKey(source protocol.ModelsSource, path string) string {
	return fmt.Sprintf("%s/%s/%s/%s", source.Hub, source.Repository, source.Revision, path)
}

func newModelHub(t *testing.T) *modelHub {
	t.Helper()
	hub := &modelHub{
		t: t, done: make(chan struct{}), stalled: make(chan string, 16),
		files: map[string]hubFile{}, repositories: map[string]hubRepository{}, stalls: map[string]int64{}, corrupt: map[string]bool{},
	}
	hub.server = httptest.NewTLSServer(http.HandlerFunc(hub.serve))
	t.Cleanup(func() {
		close(hub.done)
		hub.server.Close()
	})
	return hub
}

// RoundTrip sends a request for any host to the loopback server.
func (hub *modelHub) RoundTrip(request *http.Request) (*http.Response, error) {
	redirected := request.Clone(request.Context())
	redirected.Header.Set("X-Hub-Host", request.URL.Host)
	redirected.URL.Host = hub.server.Listener.Addr().String()
	redirected.Host = ""
	return hub.server.Client().Transport.RoundTrip(redirected)
}

func (hub *modelHub) add(key, local string) {
	hub.t.Helper()
	info, err := os.Stat(local)
	if err != nil {
		hub.t.Fatal(err)
	}
	hub.mu.Lock()
	hub.files[key] = hubFile{local: local, size: info.Size()}
	hub.mu.Unlock()
}

// definition serves the assets of a pinned definition from a directory that
// holds them under their asset names.
func (hub *modelHub) definition(definition *protocol.ModelsDefinition, directory string) {
	hub.t.Helper()
	for _, asset := range definition.Assets {
		hub.add(hubKey(asset.Source, asset.Path), filepath.Join(directory, asset.Name))
	}
}

// repository publishes a Hugging Face repository at one commit.
func (hub *modelHub) repository(name string, repository hubRepository) {
	hub.t.Helper()
	source := protocol.ModelsSource{Hub: protocol.ModelsHubHuggingFace, Repository: name, Revision: repository.commit}
	for _, sibling := range repository.siblings {
		hub.add(hubKey(source, sibling.path), sibling.local)
	}
	hub.mu.Lock()
	hub.repositories[name] = repository
	hub.mu.Unlock()
}

func (hub *modelHub) set(change func()) {
	hub.mu.Lock()
	change()
	hub.mu.Unlock()
}

// seen returns the recorded requests for one file.
func (hub *modelHub) seen(key string) []hubRequest {
	hub.mu.Lock()
	defer hub.mu.Unlock()
	var found []hubRequest
	for _, request := range hub.requests {
		if request.key == key {
			found = append(found, request)
		}
	}
	return found
}

func (hub *modelHub) serve(writer http.ResponseWriter, request *http.Request) {
	path := request.URL.Path
	switch {
	case path == "/api/models":
		query := strings.ToLower(request.URL.Query().Get("search"))
		hub.mu.Lock()
		rows := []map[string]string{}
		for _, name := range slices.Sorted(func(yield func(string) bool) {
			for name := range hub.repositories {
				if !yield(name) {
					return
				}
			}
		}) {
			if strings.Contains(strings.ToLower(name), query) {
				rows = append(rows, map[string]string{"id": name})
			}
		}
		hub.requests = append(hub.requests, hubRequest{host: request.Header.Get("X-Hub-Host"), key: "search:" + query})
		hub.mu.Unlock()
		json.NewEncoder(writer).Encode(rows)
	case strings.HasPrefix(path, "/api/v1/models/") && strings.HasSuffix(path, "/repo"):
		name := strings.TrimSuffix(strings.TrimPrefix(path, "/api/v1/models/"), "/repo")
		query := request.URL.Query()
		source := protocol.ModelsSource{Hub: protocol.ModelsHubModelScope, Repository: name, Revision: query.Get("Revision")}
		hub.file(writer, request, hubKey(source, query.Get("FilePath")))
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
		hub.requests = append(hub.requests, hubRequest{host: request.Header.Get("X-Hub-Host"), key: "metadata:" + name + "@" + revision})
		hub.mu.Unlock()
		if !found || !known || revision != "main" && revision != repository.commit {
			http.NotFound(writer, request)
			return
		}
		siblings := []map[string]any{}
		for _, sibling := range repository.siblings {
			info, err := os.Stat(sibling.local)
			if err != nil {
				http.Error(writer, err.Error(), http.StatusInternalServerError)
				return
			}
			siblings = append(siblings, map[string]any{"rfilename": sibling.path, "size": info.Size(),
				"lfs": map[string]any{"sha256": sibling.sha256, "size": info.Size()}})
		}
		json.NewEncoder(writer).Encode(map[string]any{"sha": repository.commit, "private": false, "gated": false,
			"siblings": siblings, "cardData": map[string]string{"license": repository.license}})
	default:
		parts := strings.SplitN(strings.TrimPrefix(path, "/"), "/", 5)
		if len(parts) != 5 || parts[2] != "resolve" {
			http.NotFound(writer, request)
			return
		}
		source := protocol.ModelsSource{Hub: protocol.ModelsHubHuggingFace, Repository: parts[0] + "/" + parts[1], Revision: parts[3]}
		hub.file(writer, request, hubKey(source, parts[4]))
	}
}

func (hub *modelHub) file(writer http.ResponseWriter, request *http.Request, key string) {
	hub.mu.Lock()
	file, known := hub.files[key]
	stall, corrupt := hub.stalls[key], hub.corrupt[key]
	hub.requests = append(hub.requests, hubRequest{host: request.Header.Get("X-Hub-Host"), key: key, ranged: request.Header.Get("Range")})
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
	switch {
	case corrupt:
		// The true length with one changed byte.
		writer.Header().Set("Content-Length", strconv.FormatInt(file.size, 10))
		first := make([]byte, 1)
		if _, err := io.ReadFull(source, first); err != nil {
			return
		}
		first[0] ^= 0xff
		writer.Write(first)
		io.Copy(writer, source)
	case stall > 0 && request.Header.Get("Range") == "":
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
		case <-time.After(localModelInstallWait):
		}
	default:
		http.ServeContent(writer, request, "", time.Time{}, source)
	}
}

// engineCounts counts the engines the scheduler loads and releases.
type engineCounts struct {
	mu         sync.Mutex
	loads      map[protocol.ModelsKind]int
	validating map[protocol.ModelsKind]int
	releases   map[protocol.ModelsKind]int
	probes     int
}

func (counts *engineCounts) read(kind protocol.ModelsKind) (loads, validating, releases, probes int) {
	counts.mu.Lock()
	defer counts.mu.Unlock()
	return counts.loads[kind], counts.validating[kind], counts.releases[kind], counts.probes
}

// countedCapabilities are the build's real runtimes with their loads and
// releases counted. Embedding and OCR skip the packaged location check
// because the test loads the ONNX Runtime from SAILRY_ONNXRUNTIME_DIR; the
// decision capability is unchanged.
func countedCapabilities(counts *engineCounts) localmodels.Capabilities {
	counts.loads, counts.validating, counts.releases = map[protocol.ModelsKind]int{}, map[protocol.ModelsKind]int{}, map[protocol.ModelsKind]int{}
	capabilities := LocalModelCapabilities()
	for kind, capability := range capabilities {
		load := capability.Load
		capability.Load = func(resource localmodels.Resource, validating bool) (localmodels.Engine, error) {
			engine, err := load(resource, validating)
			if err != nil {
				return engine, err
			}
			counts.mu.Lock()
			counts.loads[kind]++
			if validating {
				counts.validating[kind]++
			}
			counts.mu.Unlock()
			release := engine.Release
			engine.Release = func() {
				release()
				counts.mu.Lock()
				counts.releases[kind]++
				counts.mu.Unlock()
			}
			return engine, nil
		}
		if probe := capability.Probe; probe != nil {
			capability.Probe = func(ctx context.Context, engine any) error {
				counts.mu.Lock()
				counts.probes++
				counts.mu.Unlock()
				return probe(ctx, engine)
			}
		}
		if kind != protocol.ModelsKindDecision {
			capability.Available = nil
		}
		capabilities[kind] = capability
	}
	return capabilities
}

// localModelNode starts a Node whose Hub requests reach the model hub.
func localModelNode(t *testing.T, hub *modelHub, adjust func(*Options)) (*Node, *engineCounts) {
	t.Helper()
	if err := onnx.Load(requiredEnvironment(t, "SAILRY_ONNXRUNTIME_DIR")); err != nil {
		t.Fatal(err)
	}
	counts := &engineCounts{}
	environment := process.NewEnvironment([]string{"PATH=/usr/bin:/bin"})
	options := Options{
		Environment: &environment, MCP: fixtureMCPOptions(),
		LocalModels: localmodels.Options{Capabilities: countedCapabilities(counts), Transport: hub},
	}
	if adjust != nil {
		adjust(&options)
	}
	node, err := StartWithOptions(t.Context(), filepath.Join(t.TempDir(), "profile"), options)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { node.Stop(context.Background()) })
	return node, counts
}

// localStart dispatches a command and returns its completion.
func localStart(t *testing.T, ctx context.Context, node *Node, command protocol.Command) <-chan link.Response {
	t.Helper()
	accepted, err := node.Dispatch(ctx, node.ID(), protocol.NewRequest(node.ID(), command))
	if err != nil {
		t.Fatalf("%s was not admitted: %v", command.Kind, err)
	}
	return accepted.Completion
}

func localFinish(t *testing.T, kind string, completion <-chan link.Response, wait time.Duration) link.Response {
	t.Helper()
	select {
	case response, delivered := <-completion:
		if !delivered {
			t.Fatalf("%s lost its completion", kind)
		}
		return response
	case <-time.After(wait):
		t.Fatalf("%s did not complete within %s", kind, wait)
	}
	return link.Response{}
}

// localCommand runs one command through Node.Dispatch within wait.
func localCommand(t *testing.T, node *Node, command protocol.Command, wait time.Duration) link.Response {
	t.Helper()
	return localFinish(t, command.Kind, localStart(t, t.Context(), node, command), wait)
}

// payload reads a value the Node returned as itself, as a pointer or, after
// a durable replay, as decoded JSON.
func payload[T any](t *testing.T, data any) T {
	t.Helper()
	switch value := data.(type) {
	case T:
		return value
	case *T:
		if value != nil {
			return *value
		}
	}
	var decoded T
	body, err := json.Marshal(data)
	if err == nil {
		err = json.Unmarshal(body, &decoded)
	}
	if err != nil {
		t.Fatalf("payload %T is not %T: %v", data, decoded, err)
	}
	return decoded
}

func localOutput[T any](t *testing.T, node *Node, command protocol.Command, wait time.Duration) T {
	t.Helper()
	response := localCommand(t, node, command, wait)
	if response.Fault != nil {
		t.Fatalf("%s: %v", command.Kind, response.Fault)
	}
	return payload[T](t, response.Output.Data)
}

func wantFault(t *testing.T, what string, response link.Response, code protocol.ErrorCode, message string) {
	t.Helper()
	if response.Fault == nil || response.Fault.Code != code || !strings.Contains(response.Fault.Message, message) {
		t.Fatalf("%s: fault = %+v, output = %+v; want %s %q", what, response.Fault, response.Output, code, message)
	}
}

func localModels(t *testing.T, node *Node, kind protocol.ModelsKind) protocol.ModelsCatalog {
	t.Helper()
	return localOutput[protocol.ModelsCatalog](t, node, protocol.Command{Kind: "read_local_models", Data: protocol.CommandReadLocalModelsPayload{Kind: &kind}}, localModelWait)
}

func localModelState(t *testing.T, node *Node, kind protocol.ModelsKind, id string) string {
	t.Helper()
	for _, model := range localModels(t, node, kind).Models {
		if model.ID == id {
			return model.State.Kind
		}
	}
	t.Fatalf("model %s is not listed", id)
	return ""
}

func localSettings(t *testing.T, node *Node) protocol.ModelsLocalSettings {
	t.Helper()
	return localOutput[protocol.ModelsLocalSettings](t, node, protocol.Command{Kind: "read_local_model_settings"}, localModelWait)
}

// saveChoice saves one kind's choice at the current revision.
func saveChoice(t *testing.T, node *Node, kind protocol.ModelsKind, choice protocol.ModelsLocalChoice) protocol.ModelsLocalSettings {
	t.Helper()
	settings := localSettings(t, node)
	settings.Choices[kind] = choice
	return localOutput[protocol.ModelsLocalSettings](t, node, protocol.Command{Kind: "save_local_model_settings", Data: settings}, localModelWait)
}

func installModel(t *testing.T, node *Node, id string) {
	t.Helper()
	installed := localOutput[protocol.OutputLocalModelPayload](t, node, protocol.Command{Kind: "install_local_model", Data: protocol.CommandInstallLocalModelPayload{ID: id}}, localModelInstallWait)
	if installed.ID != id {
		t.Fatalf("installed %q, want %q", installed.ID, id)
	}
}

// installationWatch collects the installations topic.
type installationWatch struct {
	mu      sync.Mutex
	updates [][]protocol.InstallationTask
}

func watchInstallations(t *testing.T, node *Node) *installationWatch {
	t.Helper()
	subscription, err := node.Subscribe(t.Context(), node.ID(), protocol.Topic{Kind: "installations"})
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { subscription.Close() })
	watch := &installationWatch{}
	ctx := t.Context()
	go func() {
		for {
			update, err := subscription.Next(ctx)
			if err != nil {
				return
			}
			if update.Kind != "installations" {
				continue
			}
			var tasks []protocol.InstallationTask
			switch value := update.Data.(type) {
			case protocol.UpdateInstallationsPayload:
				tasks = value.Tasks
			case *protocol.UpdateInstallationsPayload:
				tasks = value.Tasks
			}
			watch.mu.Lock()
			watch.updates = append(watch.updates, slices.Clone(tasks))
			watch.mu.Unlock()
		}
	}()
	return watch
}

// progress is what one published task says about a model.
type progress struct {
	state   string
	phase   protocol.InstallationPhase
	percent int
	fault   *protocol.Fault
}

// history lists the published states of one model's installation, from the
// update at index from on.
func (watch *installationWatch) history(kind protocol.ModelsKind, id string, from int) []progress {
	watch.mu.Lock()
	defer watch.mu.Unlock()
	var found []progress
	for _, tasks := range watch.updates[min(from, len(watch.updates)):] {
		for _, task := range tasks {
			var resource protocol.InstallationResourceModelPayload
			switch value := task.Resource.Data.(type) {
			case protocol.InstallationResourceModelPayload:
				resource = value
			case *protocol.InstallationResourceModelPayload:
				resource = *value
			}
			if task.Resource.Kind != "model" || resource.Kind != kind || resource.ID != id {
				continue
			}
			entry := progress{state: task.State.Kind, percent: -1}
			switch value := task.State.Data.(type) {
			case protocol.InstallationStateRunningPayload:
				entry.phase = value.Phase
				if value.Progress != nil {
					if percent, valid := value.Progress.Data.(protocol.InstallationProgressPercentPayload); valid {
						entry.percent = int(percent.Percent)
					}
				}
			case protocol.InstallationStateFailedPayload:
				entry.fault = &value.Error
			}
			found = append(found, entry)
		}
	}
	return found
}

func (watch *installationWatch) mark() int {
	watch.mu.Lock()
	defer watch.mu.Unlock()
	return len(watch.updates)
}

// eventually polls until ready within localModelWait.
func eventually(t *testing.T, what string, ready func() bool) {
	t.Helper()
	deadline := time.Now().Add(localModelWait)
	for !ready() {
		if time.Now().After(deadline) {
			t.Fatalf("timed out waiting for %s", what)
		}
		time.Sleep(20 * time.Millisecond)
	}
}

func fileDigest(t *testing.T, path string) string {
	t.Helper()
	source, err := os.Open(path)
	if err != nil {
		t.Fatal(err)
	}
	defer source.Close()
	hash := sha256.New()
	if _, err := io.Copy(hash, source); err != nil {
		t.Fatal(err)
	}
	return hex.EncodeToString(hash.Sum(nil))
}

// TestLocalModelInstallation installs the managed embedding model from the
// hub through the Node: observable progress, pinned-revision requests,
// verified files, the compatibility probe on the real ONNX Runtime, the
// Knowledge default round trip with its revision conflict, removal, a refused
// corrupted file, and a cancelled installation that keeps its checkpoint and
// resumes from it.
func TestLocalModelInstallation(t *testing.T) {
	hub := newModelHub(t)
	node, counts := localModelNode(t, hub, nil)
	registry := node.localModels.registry
	const id = localmodels.DefaultEmbedding
	definition, err := registry.Definition(id)
	if err != nil {
		t.Fatal(err)
	}
	hub.definition(definition, requiredEnvironment(t, "SAILRY_EMBEDDING_MODEL_DIR"))
	var graph protocol.ModelsAsset
	for _, asset := range definition.Assets {
		if asset.Name == "model.onnx" {
			graph = asset
		}
	}
	graphKey := hubKey(graph.Source, graph.Path)
	watch := watchInstallations(t, node)
	embedding := protocol.ModelsKindEmbedding

	catalog := localModels(t, node, embedding)
	if catalog.Availability[embedding].Kind != "available" {
		t.Fatalf("embedding availability = %+v", catalog.Availability[embedding])
	}
	if state := localModelState(t, node, embedding, id); state != "missing" {
		t.Fatalf("state before installation = %s", state)
	}

	// Install: every file comes from its pinned revision at the Hub's own
	// host, progress is published, and the probe runs on the loaded engine.
	installModel(t, node, id)
	history := watch.history(embedding, id, 0)
	downloading, validating := false, false
	for _, entry := range history {
		downloading = downloading || entry.state == "running" && entry.phase == protocol.InstallationPhaseDownloading && entry.percent > 0 && entry.percent < 100
		validating = validating || entry.state == "running" && entry.phase == protocol.InstallationPhaseValidating
	}
	if !downloading || !validating {
		t.Fatalf("published installation states = %+v", history)
	}
	eventually(t, "the finished installation to leave the topic", func() bool {
		watch.mu.Lock()
		defer watch.mu.Unlock()
		return len(watch.updates) > 0 && len(watch.updates[len(watch.updates)-1]) == 0
	})
	for _, asset := range definition.Assets {
		requests := hub.seen(hubKey(asset.Source, asset.Path))
		if len(requests) != 1 || requests[0].host != "huggingface.co" || requests[0].ranged != "" {
			t.Fatalf("requests for %s at revision %s = %+v", asset.Path, asset.Source.Revision, requests)
		}
	}
	resource, err := registry.Resource(embedding, id, nil)
	if err != nil {
		t.Fatal(err)
	}
	for _, asset := range definition.Assets {
		if digest := fileDigest(t, filepath.Join(resource.Directory, asset.Name)); digest != asset.Sha256 {
			t.Fatalf("installed %s has SHA-256 %s, want %s", asset.Name, digest, asset.Sha256)
		}
	}
	if loads, validated, _, probes := counts.read(embedding); loads != 1 || validated != 1 || probes != 1 {
		t.Fatalf("installation loaded %d engines (%d validating) and probed %d times", loads, validated, probes)
	}
	if state := localModelState(t, node, embedding, id); state != "installed" {
		t.Fatalf("state after installation = %s", state)
	}
	// An installed model installs again without a request.
	installModel(t, node, id)
	if requests := hub.seen(graphKey); len(requests) != 1 {
		t.Fatalf("an installed model was downloaded again: %+v", requests)
	}

	// Knowledge default: keyword only, then the installed model; a stale
	// revision conflicts and changes nothing.
	initial := localSettings(t, node)
	keyword := saveChoice(t, node, embedding, protocol.ModelsLocalChoice{})
	if keyword.Revision != initial.Revision+1 || keyword.Choices[embedding].Enabled || keyword.Choices[embedding].Model != nil {
		t.Fatalf("keyword-only settings = %+v", keyword)
	}
	selected := id
	saved := saveChoice(t, node, embedding, protocol.ModelsLocalChoice{Enabled: true, Model: &selected})
	read := localSettings(t, node)
	if saved.Revision != initial.Revision+2 || !reflect.DeepEqual(read, saved) || !read.Choices[embedding].Enabled ||
		read.Choices[embedding].Model == nil || *read.Choices[embedding].Model != id || localmodels.Selected(new(registry.Settings()), embedding) != id {
		t.Fatalf("saved settings = %+v, read back %+v", saved, read)
	}
	stale := keyword
	stale.Choices = map[protocol.ModelsKind]protocol.ModelsLocalChoice{embedding: {}}
	wantFault(t, "stale settings", localCommand(t, node, protocol.Command{Kind: "save_local_model_settings", Data: stale}, localModelWait),
		protocol.ErrorCodeRevisionConflict, "")
	if after := localSettings(t, node); !reflect.DeepEqual(after, saved) {
		t.Fatalf("a conflicting save changed the settings: %+v", after)
	}

	// Delete removes the files.
	remove := func() {
		t.Helper()
		removed := localOutput[protocol.OutputLocalModelPayload](t, node, protocol.Command{Kind: "delete_local_model", Data: protocol.CommandDeleteLocalModelPayload{ID: id}}, localModelWait)
		if removed.ID != id {
			t.Fatalf("removed %q", removed.ID)
		}
		if state := localModelState(t, node, embedding, id); state != "missing" {
			t.Fatalf("state after removal = %s", state)
		}
		if _, err := os.Stat(resource.Directory); !errors.Is(err, os.ErrNotExist) {
			t.Fatalf("model directory after removal: %v", err)
		}
	}
	remove()
	if _, _, releases, _ := counts.read(embedding); releases != 1 {
		t.Fatalf("removal released %d engines", releases)
	}

	// A file that differs from its pinned SHA-256 is refused.
	config := hubKey(definition.Assets[0].Source, definition.Assets[0].Path)
	hub.set(func() { hub.corrupt[config] = true })
	mark := watch.mark()
	wantFault(t, "corrupted file", localCommand(t, node, protocol.Command{Kind: "install_local_model", Data: protocol.CommandInstallLocalModelPayload{ID: id}}, localModelInstallWait),
		protocol.ErrorCodeInvalidRequest, "local model asset validation failed")
	hub.set(func() { delete(hub.corrupt, config) })
	if state := localModelState(t, node, embedding, id); state != "missing" {
		t.Fatalf("state after a corrupted file = %s", state)
	}
	if _, err := os.Stat(filepath.Join(resource.Directory, definition.Assets[0].Name)); !errors.Is(err, os.ErrNotExist) {
		t.Fatalf("the corrupted file was published: %v", err)
	}
	eventually(t, "the failed installation on the topic", func() bool {
		for _, entry := range watch.history(embedding, id, mark) {
			if entry.state == "failed" && entry.fault != nil && entry.fault.Code == protocol.ErrorCodeInvalidRequest {
				return true
			}
		}
		return false
	})

	// Cancel during a download: the installation stops and its checkpoint
	// stays.
	const served = 64 << 20
	hub.set(func() { hub.stalls[graphKey] = served })
	mark = watch.mark()
	installing := localStart(t, t.Context(), node, protocol.Command{Kind: "install_local_model", Data: protocol.CommandInstallLocalModelPayload{ID: id}})
	select {
	case key := <-hub.stalled:
		if key != graphKey {
			t.Fatalf("stalled on %s", key)
		}
	case <-time.After(localModelWait):
		t.Fatal("the download did not reach the stalled file")
	}
	var total uint64
	for _, asset := range definition.Assets {
		total += asset.Bytes
	}
	expected := int((total - graph.Bytes + served) * 100 / total)
	eventually(t, "the stalled download's percent", func() bool {
		entries := watch.history(embedding, id, mark)
		return len(entries) > 0 && entries[len(entries)-1].phase == protocol.InstallationPhaseDownloading && entries[len(entries)-1].percent == expected
	})
	if state := localModelState(t, node, embedding, id); state != "downloading" {
		t.Fatalf("state during the download = %s", state)
	}
	cancelled := localOutput[protocol.OutputLocalModelPayload](t, node, protocol.Command{Kind: "cancel_local_model", Data: protocol.CommandCancelLocalModelPayload{ID: id}}, localModelWait)
	if cancelled.ID != id {
		t.Fatalf("cancelled %q", cancelled.ID)
	}
	wantFault(t, "cancelled installation", localFinish(t, "install_local_model", installing, localModelWait), protocol.ErrorCodeCancelled, "local model operation was cancelled")
	if state := localModelState(t, node, embedding, id); state != "missing" {
		t.Fatalf("state after cancellation = %s", state)
	}
	part := filepath.Join(resource.Directory, fmt.Sprintf(".%s-%d-%s.part", graph.Name, graph.Bytes, graph.Sha256))
	var checkpoint struct {
		Received uint64 `json:"received"`
	}
	body, err := os.ReadFile(part + ".json")
	if err != nil {
		t.Fatalf("checkpoint after cancellation: %v", err)
	}
	if err := json.Unmarshal(body, &checkpoint); err != nil {
		t.Fatal(err)
	}
	info, err := os.Stat(part)
	if err != nil || checkpoint.Received != served || uint64(info.Size()) < checkpoint.Received {
		t.Fatalf("checkpoint records %d bytes of %d served, partial file: %v", checkpoint.Received, served, err)
	}

	// The next installation resumes from the checkpoint.
	hub.set(func() { delete(hub.stalls, graphKey) })
	installModel(t, node, id)
	requests := hub.seen(graphKey)
	if last := requests[len(requests)-1]; last.ranged != fmt.Sprintf("bytes=%d-", checkpoint.Received) {
		t.Fatalf("resumed request = %+v", last)
	}
	if digest := fileDigest(t, filepath.Join(resource.Directory, graph.Name)); digest != graph.Sha256 {
		t.Fatalf("resumed model.onnx has SHA-256 %s", digest)
	}
	if _, err := os.Stat(part); !errors.Is(err, os.ErrNotExist) {
		t.Fatalf("partial file after publication: %v", err)
	}
	if loads, validated, _, probes := counts.read(embedding); loads != 2 || validated != 2 || probes != 2 {
		t.Fatalf("second installation: %d loads (%d validating), %d probes", loads, validated, probes)
	}
	if state := localModelState(t, node, embedding, id); state != "installed" {
		t.Fatalf("state after the resumed installation = %s", state)
	}
	remove()

	// A file set that matches its pins but that the runtime cannot load
	// fails the compatibility check and is not installed.
	broken := filepath.Join(t.TempDir(), "model.onnx")
	garbage := make([]byte, 4096)
	if _, err := rand.Read(garbage); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(broken, garbage, 0o600); err != nil {
		t.Fatal(err)
	}
	source := protocol.ModelsSource{Hub: protocol.ModelsHubHuggingFace, Repository: "fixture/broken-embedding", Revision: strings.Repeat("ab", 20)}
	custom := protocol.ModelsDefinition{Source: source, Name: "Broken", License: "MIT", Format: definition.Format}
	for _, asset := range definition.Assets {
		local := filepath.Join(requiredEnvironment(t, "SAILRY_EMBEDDING_MODEL_DIR"), asset.Name)
		pinned := protocol.ModelsAsset{Source: source, Path: asset.Name, Name: asset.Name, Bytes: asset.Bytes, Sha256: asset.Sha256}
		if asset.Name == "model.onnx" {
			sum := sha256.Sum256(garbage)
			local, pinned.Bytes, pinned.Sha256 = broken, uint64(len(garbage)), hex.EncodeToString(sum[:])
		}
		hub.add(hubKey(source, asset.Name), local)
		custom.Assets = append(custom.Assets, pinned)
	}
	registered := localOutput[protocol.OutputLocalModelPayload](t, node, protocol.Command{Kind: "register_local_model", Data: protocol.CommandRegisterLocalModelPayload{Definition: custom}}, localModelWait)
	wantFault(t, "incompatible model", localCommand(t, node, protocol.Command{Kind: "install_local_model", Data: protocol.CommandInstallLocalModelPayload{ID: registered.ID}}, localModelInstallWait),
		protocol.ErrorCodeUnavailable, "local model failed: ")
	if state := localModelState(t, node, embedding, registered.ID); state != "missing" {
		t.Fatalf("state of an incompatible model = %s", state)
	}
	if loads, _, _, probes := counts.read(embedding); loads != 2 || probes != 2 {
		t.Fatalf("an incompatible model was loaded: %d loads, %d probes", loads, probes)
	}
}

// installOCR installs the first pinned OCR model from the hub's ModelScope.
func installOCR(t *testing.T, node *Node, hub *modelHub) string {
	t.Helper()
	definition := localmodels.OCR()[0]
	id := localmodels.ID(&definition)
	hub.definition(&definition, requiredEnvironment(t, "SAILRY_OCR_MODEL_DIR"))
	installModel(t, node, id)
	for _, asset := range definition.Assets {
		requests := hub.seen(hubKey(asset.Source, asset.Path))
		if len(requests) != 1 || requests[0].host != "modelscope.cn" {
			t.Fatalf("requests for %s = %+v", asset.Path, requests)
		}
	}
	return id
}

// documentProject registers a project holding copies of the OCR fixtures and
// returns its worktree.
func documentProject(t *testing.T, node *Node, names ...string) protocol.Project {
	t.Helper()
	fixtures := requiredEnvironment(t, "SAILRY_LOCALMODELS_FIXTURES")
	root, err := filepath.EvalSymlinks(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	for _, name := range names {
		data, err := os.ReadFile(filepath.Join(fixtures, "ocr", name))
		if err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(filepath.Join(root, name), data, 0o600); err != nil {
			t.Fatal(err)
		}
	}
	return localOutput[protocol.Project](t, node, protocol.Command{Kind: "create_project", Data: protocol.ProjectsDraft{
		Name: "Documents", Path: root, Source: protocol.ProjectsSource{Kind: "local"}, Appearance: protocol.DefaultProjectsAppearance()}}, localModelWait)
}

func projectWorktree(t *testing.T, node *Node, project protocol.Project) protocol.WorktreeID {
	t.Helper()
	snapshot, err := node.Snapshot(t.Context())
	if err != nil {
		t.Fatal(err)
	}
	for _, worktree := range snapshot.Worktrees {
		if worktree.Project != nil && *worktree.Project == project.ID {
			return worktree.ID
		}
	}
	t.Fatal("the project has no worktree")
	return protocol.WorktreeID{}
}

func readOffice(worktree protocol.WorktreeID, path string) protocol.Command {
	return protocol.Command{Kind: "read_office", Data: protocol.CommandReadOfficePayload{Worktree: worktree,
		Options: protocol.OfficeRead{Source: protocol.OfficeSource{Kind: "path", Data: path}}}}
}

// officeScript is a model that discovers the Office read tool, calls it once
// and keeps the tool results it is then shown.
type officeScript struct {
	fantasy.LanguageModel
	name string
	args atomic.Pointer[string]
	step atomic.Int32

	mu      sync.Mutex
	results []string
}

func (script *officeScript) Stream(_ context.Context, call fantasy.Call) (fantasy.StreamResponse, error) {
	step := script.step.Add(1)
	if step > 3 {
		return nil, errors.New("unexpected extra model request")
	}
	if step == 3 {
		script.mu.Lock()
		for _, message := range call.Prompt {
			for _, part := range message.Content {
				result, found := fantasy.AsMessagePart[fantasy.ToolResultPart](part)
				if !found || result.ToolCallID != "read" {
					continue
				}
				if text, found := fantasy.AsToolResultOutputType[fantasy.ToolResultOutputContentText](result.Output); found {
					script.results = append(script.results, text.Text)
				} else {
					script.results = append(script.results, fmt.Sprintf("unexpected tool output %#v", result.Output))
				}
			}
		}
		script.mu.Unlock()
	}
	return func(yield func(fantasy.StreamPart) bool) {
		switch step {
		case 1:
			args, _ := json.Marshal(map[string]any{"query": "select:" + script.name})
			if !yield(fantasy.StreamPart{Type: fantasy.StreamPartTypeToolCall, ID: "discover", ToolCallName: "tool_search", ToolCallInput: string(args)}) {
				return
			}
		case 2:
			if !yield(fantasy.StreamPart{Type: fantasy.StreamPartTypeToolCall, ID: "read", ToolCallName: script.name, ToolCallInput: *script.args.Load()}) {
				return
			}
		case 3:
			if !yield(fantasy.StreamPart{Type: fantasy.StreamPartTypeTextDelta, ID: "answer", Delta: "Read the attachment"}) {
				return
			}
		}
		yield(fantasy.StreamPart{Type: fantasy.StreamPartTypeFinish, FinishReason: fantasy.FinishReasonStop})
	}, nil
}

// TestLocalModelOCR reads an image and a scanned PDF through read_office
// with an OCR model installed from the hub: the text equals the recorded
// reference, a missing model and a disabled OCR report their faults,
// cancellation is reported, and an agent turn reads an image attachment with
// the bundled Office tool.
func TestLocalModelOCR(t *testing.T) {
	catalog, err := operations.Bundled()
	if err != nil {
		t.Fatal(err)
	}
	script := &officeScript{}
	for _, tool := range catalog.Tools {
		if tool.Operation == protocol.ToolOperationReadOffice && tool.Available(true) {
			script.name = tool.Name
			break
		}
	}
	if script.name == "" {
		t.Fatal("the bundled Office read tool is missing")
	}
	hub := newModelHub(t)
	node, counts := localModelNode(t, hub, func(options *Options) {
		options.Model = func(context.Context, model.Binding) (fantasy.LanguageModel, error) { return script, nil }
	})
	fixtures := requiredEnvironment(t, "SAILRY_LOCALMODELS_FIXTURES")
	images := recordedPages(t, filepath.Join(fixtures, "rust-ocr.json"))
	scanned := recordedPages(t, filepath.Join(fixtures, "rust-pdf-scan.json"))
	project := documentProject(t, node, "english.png", "scan.pdf")
	worktree := projectWorktree(t, node, project)
	recognition := protocol.ModelsKindOcr

	// OCR is on by default; without an installed model it is not configured.
	wantFault(t, "image without a model", localCommand(t, node, readOffice(worktree, "english.png"), localModelWait),
		protocol.ErrorCodeNotConfigured, "an OCR model must be installed")

	id := installOCR(t, node, hub)
	if loads, validated, _, _ := counts.read(recognition); loads != 1 || validated != 1 {
		t.Fatalf("OCR installation loaded %d engines (%d validating)", loads, validated)
	}
	if state := localModelState(t, node, recognition, id); state != "installed" {
		t.Fatalf("OCR model state = %s", state)
	}

	image := localOutput[protocol.OfficeInspection](t, node, readOffice(worktree, "english.png"), localModelWait)
	if len(image.Sections) != 1 || image.Sections[0].Name != "Image" || image.Sections[0].Text != images[0] || image.Next != nil {
		t.Fatalf("image inspection = %+v, want text %q", image, images[0])
	}
	document := localOutput[protocol.OfficeInspection](t, node, readOffice(worktree, "scan.pdf"), localModelWait)
	if len(document.Sections) != len(scanned) {
		t.Fatalf("scanned PDF has %d sections, want %d: %+v", len(document.Sections), len(scanned), document.Sections)
	}
	for index, section := range document.Sections {
		if section.Text != scanned[index] {
			t.Fatalf("%s = %q, want %q", section.Name, section.Text, scanned[index])
		}
	}

	// A caller that leaves while its pages are recognized is told so.
	leaving, leave := context.WithCancel(t.Context())
	reading := localStart(t, leaving, node, readOffice(worktree, "scan.pdf"))
	leave()
	wantFault(t, "cancelled read", localFinish(t, "read_office", reading, localModelWait), protocol.ErrorCodeCancelled, "")

	// An agent turn reads an image attachment with the bundled Office tool.
	provider := protocol.DefaultConversationProvider()
	provider.ID = protocol.NewProviderID()
	provider.Revision = 1
	provider.Enabled = true
	provider.Name = "Fixture"
	provider.Api = protocol.ConversationModelApiChatCompletions
	provider.Authentication = protocol.AuthenticationApiKey
	provider.Endpoint = "http://fixture.invalid/v1"
	provider.DefaultModel = "fixture"
	selected := protocol.DefaultConversationModel()
	selected.ID = "fixture"
	selected.Context = 32000
	selected.Output = 8000
	selected.Tools = true
	provider.Models = []protocol.ConversationModel{selected}
	err = node.profile.Store.Node.Write(t.Context(), func(db *sql.Conn) error {
		body, err := json.Marshal(map[string]any{"id": provider.ID, "provider": provider, "listed": true, "authorizations": map[string]any{}, "quota": nil})
		if err != nil {
			return err
		}
		_, err = db.ExecContext(t.Context(), "INSERT INTO providers(id,body) VALUES(?1,?2)", provider.ID.String(), body)
		return err
	})
	if err != nil {
		t.Fatal(err)
	}
	config := protocol.DefaultSessionConfig()
	config.Provider = provider.ID
	config.Model = "fixture"
	config.Permission = protocol.PermissionAsk
	config.Mode = protocol.WorkModeCode
	session := localOutput[protocol.Session](t, node, protocol.Command{Kind: "create_session", Data: protocol.CommandCreateSessionPayload{Project: &project.ID, Config: &config}}, localModelWait)

	bounded, stop := context.WithTimeout(t.Context(), localModelWait)
	defer stop()
	controller := client.New(link.NewLocal(node.ID(), node.ID(), node))
	if err := controller.Start(bounded); err != nil {
		t.Fatal(err)
	}
	defer controller.Close()
	data, err := os.ReadFile(filepath.Join(fixtures, "ocr", "english.png"))
	if err != nil {
		t.Fatal(err)
	}
	spec := protocol.AttachmentSpec{Worktree: session.Worktree, Name: "english.png", MediaType: "image/png", Size: uint64(len(data)),
		Revision: fmt.Sprintf("%x", blake3.Sum256(data))}
	output, err := controller.Do(bounded, protocol.Command{Kind: "upload_attachment", Data: &spec})
	if err != nil {
		t.Fatal(err)
	}
	upload := payload[protocol.AttachmentUpload](t, output.Data)
	if err := controller.Upload(bounded, upload.Stream, spec.Size, spec.Revision, io.NopCloser(bytes.NewReader(data)), func(uint64) {}); err != nil {
		t.Fatal(err)
	}
	output, err = controller.Execute(bounded, controller.Prepare(protocol.Command{Kind: "finish_attachment_upload",
		Data: &protocol.CommandFinishAttachmentUploadPayload{Worktree: session.Worktree, Stream: upload.Stream}}))
	if err != nil {
		t.Fatal(err)
	}
	attachment := payload[protocol.Attachment](t, output.Data)
	args, _ := json.Marshal(map[string]any{"source": map[string]string{"kind": "attachment", "value": attachment.ID.String()}})
	text := string(args)
	script.args.Store(&text)

	subscription, err := node.Subscribe(bounded, node.ID(), protocol.Topic{Kind: "node"})
	if err != nil {
		t.Fatal(err)
	}
	defer subscription.Close()
	localOutput[any](t, node, protocol.Command{Kind: "submit_turn", Data: protocol.CommandSubmitTurnPayload{Session: session.ID, ExpectedRevision: session.Revision,
		Message: protocol.ConversationInput{Text: "Read the attached image", Attachments: []protocol.AttachmentID{attachment.ID}}}}, localModelWait)
	for finished := false; !finished; {
		var history protocol.ConversationHistory
		err = node.profile.Store.Node.Read(bounded, func(db *sql.Conn) error {
			var err error
			history, err = store.ReadConversation(bounded, db, session.ID, nil, 100)
			return err
		})
		if err != nil {
			t.Fatal(err)
		}
		for _, approval := range history.Page.Approvals {
			if approval.State == protocol.ConversationApprovalStatePending {
				localOutput[any](t, node, protocol.Command{Kind: "resolve_approval", Data: protocol.CommandResolveApprovalPayload{Session: session.ID, Approval: approval.ID,
					Decision: protocol.ConversationDecision{Kind: "approve"}}}, localModelWait)
			}
		}
		for _, run := range history.Page.Runs {
			switch run.Status {
			case protocol.ConversationStatusCompleted:
				finished = true
			case protocol.ConversationStatusFailed, protocol.ConversationStatusInterrupted, protocol.ConversationStatusCancelled:
				t.Fatalf("the turn ended as %+v", run)
			}
		}
		if !finished {
			if _, err = subscription.Next(bounded); err != nil {
				t.Fatalf("the turn did not finish within %s: %v", localModelWait, err)
			}
		}
	}
	script.mu.Lock()
	results := slices.Clone(script.results)
	script.mu.Unlock()
	if len(results) != 1 {
		t.Fatalf("the model was shown %d Office tool results: %q", len(results), results)
	}
	t.Logf("Office tool result: %s", results[0])
	var result struct {
		Kind string                    `json:"kind"`
		Data protocol.OfficeInspection `json:"data"`
	}
	if err := json.Unmarshal([]byte(results[0]), &result); err != nil || result.Kind != "office_content" {
		t.Fatalf("the Office tool result is not an inspection: %v: %s", err, results[0])
	}
	if shown := result.Data; shown.Path != "english.png" || len(shown.Sections) != 1 || shown.Sections[0].Name != "Image" || shown.Sections[0].Text != images[0] {
		t.Fatalf("attachment inspection = %+v, want text %q", shown, images[0])
	}

	// OCR switched off: an image reports it, and a scanned PDF keeps its
	// pages as extracted, without recognized text.
	disabled := saveChoice(t, node, recognition, protocol.ModelsLocalChoice{})
	if disabled.Choices[recognition].Enabled {
		t.Fatalf("disabled settings = %+v", disabled)
	}
	wantFault(t, "image with OCR disabled", localCommand(t, node, readOffice(worktree, "english.png"), localModelWait),
		protocol.ErrorCodeNotConfigured, "OCR is disabled")
	blank := localOutput[protocol.OfficeInspection](t, node, readOffice(worktree, "scan.pdf"), localModelWait)
	for _, section := range blank.Sections {
		if strings.TrimSpace(section.Text) != "" {
			t.Fatalf("a scanned page was recognized with OCR disabled: %+v", section)
		}
	}

	// On again with the installed model chosen, then with the model removed.
	saveChoice(t, node, recognition, protocol.ModelsLocalChoice{Enabled: true, Model: &id})
	again := localOutput[protocol.OfficeInspection](t, node, readOffice(worktree, "english.png"), localModelWait)
	if len(again.Sections) != 1 || again.Sections[0].Text != images[0] {
		t.Fatalf("image inspection with the chosen model = %+v", again)
	}
	localOutput[protocol.OutputLocalModelPayload](t, node, protocol.Command{Kind: "delete_local_model", Data: protocol.CommandDeleteLocalModelPayload{ID: id}}, localModelWait)
	wantFault(t, "image after the model was removed", localCommand(t, node, readOffice(worktree, "english.png"), localModelWait),
		protocol.ErrorCodeNotConfigured, "an OCR model must be installed")
}

// stageDecisionRuntime copies the packaged llama.cpp runtime beside the test
// binary, where the production code looks for it.
func stageDecisionRuntime(t *testing.T) {
	t.Helper()
	source := requiredEnvironment(t, "SAILRY_DECISION_RUNTIME")
	directory := executableDirectory(t)
	entries, err := os.ReadDir(source)
	if err != nil {
		t.Fatal(err)
	}
	for _, entry := range entries {
		data, err := os.ReadFile(filepath.Join(source, entry.Name()))
		if err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(filepath.Join(directory, entry.Name()), data, 0o755); err != nil {
			t.Fatal(err)
		}
	}
}

func executableDirectory(t *testing.T) string {
	t.Helper()
	executable, err := os.Executable()
	if err == nil {
		executable, err = filepath.EvalSymlinks(executable)
	}
	if err != nil {
		t.Fatal(err)
	}
	return filepath.Dir(executable)
}

// installJulia resolves the Julia repository on the hub, registers the
// pinned definition and installs it.
func installJulia(t *testing.T, node *Node, hub *modelHub) string {
	t.Helper()
	local := requiredEnvironment(t, "SAILRY_DECISION_JULIA")
	digest := fileDigest(t, local)
	hub.repository(juliaRepository, hubRepository{commit: juliaRevision, license: "apache-2.0",
		siblings: []hubSibling{{path: filepath.Base(local), local: local, sha256: digest}}})
	definition := localOutput[protocol.ModelsDefinition](t, node, protocol.Command{Kind: "resolve_model", Data: protocol.CommandResolveModelPayload{
		Kind: protocol.ModelsKindDecision, Source: protocol.ModelsSource{Hub: protocol.ModelsHubHuggingFace, Repository: juliaRepository, Revision: "main"}}}, localModelWait)
	if len(definition.Assets) != 1 || definition.Assets[0].Name != "model.gguf" || definition.Assets[0].Source.Revision != juliaRevision ||
		definition.Assets[0].Sha256 != digest || definition.Format.Kind != "decision" {
		t.Fatalf("resolved definition = %+v", definition)
	}
	registered := localOutput[protocol.OutputLocalModelPayload](t, node, protocol.Command{Kind: "register_local_model", Data: protocol.CommandRegisterLocalModelPayload{Definition: definition}}, localModelWait)
	installModel(t, node, registered.ID)
	requests := hub.seen(hubKey(definition.Assets[0].Source, definition.Assets[0].Path))
	if len(requests) != 1 || requests[0].host != "huggingface.co" {
		t.Fatalf("requests for the decision model = %+v", requests)
	}
	return registered.ID
}

// decisionRequests are the recorded requests as the official server saw
// them, and its responses for the Julia model.
func decisionRequests(t *testing.T) ([]protocol.DecisionRequest, []map[string]any) {
	t.Helper()
	fixtures := requiredEnvironment(t, "SAILRY_DECISION_FIXTURES")
	load := func(name string, value any) {
		t.Helper()
		data, err := os.ReadFile(filepath.Join(fixtures, name))
		if err != nil {
			t.Fatal(err)
		}
		if err := json.Unmarshal(data, value); err != nil {
			t.Fatal(err)
		}
	}
	var golden struct {
		Models map[string]struct {
			SHA256    string           `json:"model_sha256"`
			Responses []map[string]any `json:"responses"`
		} `json:"models"`
	}
	var oracle struct {
		Models map[string]map[string]struct {
			Raw   string `json:"raw_state"`
			State string `json:"state"`
		} `json:"models"`
	}
	var requests []protocol.DecisionRequest
	load("golden.json", &golden)
	load("oracle-inputs.json", &oracle)
	load("requests.json", &requests)
	recorded := golden.Models["julia"]
	if len(requests) == 0 || len(recorded.Responses) != len(requests) {
		t.Fatalf("golden fixture has %d responses for %d requests", len(recorded.Responses), len(requests))
	}
	if digest := fileDigest(t, requiredEnvironment(t, "SAILRY_DECISION_JULIA")); digest != recorded.SHA256 {
		t.Fatalf("the Julia model has SHA-256 %s, the golden fixture %s", digest, recorded.SHA256)
	}
	for index := range requests {
		if substitute, present := oracle.Models["julia"][strconv.Itoa(index)]; present {
			if substitute.Raw != requests[index].State {
				t.Fatal("oracle input does not match the request")
			}
			requests[index].State = substitute.State
		}
	}
	return requests, recorded.Responses
}

// sameDecision compares a response with the official server's within 1e-4
// and returns the largest numeric difference.
func sameDecision(t *testing.T, response protocol.DecisionResponse, expected map[string]any) float64 {
	t.Helper()
	usage := expected["usage"].(map[string]any)
	if float64(response.Usage.InputTokens) != usage["input_tokens"] || float64(response.Usage.OutputTokens) != usage["output_tokens"] {
		t.Fatalf("usage = %+v, want %v", response.Usage, usage)
	}
	data, err := json.Marshal(response.Answers)
	if err != nil {
		t.Fatal(err)
	}
	var actual map[string]any
	if err := json.Unmarshal(data, &actual); err != nil {
		t.Fatal(err)
	}
	answers := expected["answers"].(map[string]any)
	for _, answer := range answers {
		// The server reports score fields by level index.
		if answer := answer.(map[string]any); answer["type"] == "score" {
			for _, field := range []string{"legend", "probabilities"} {
				levels, indexed := answer[field].(map[string]any)
				if !indexed {
					continue
				}
				ordered := make([]any, len(levels))
				for level := range ordered {
					ordered[level] = levels[strconv.Itoa(level)]
				}
				answer[field] = ordered
			}
		}
	}
	return decisionDifference(t, actual, answers)
}

func decisionDifference(t *testing.T, actual, expected any) float64 {
	t.Helper()
	maximum := 0.0
	switch expected := expected.(type) {
	case float64:
		value, numeric := actual.(float64)
		if maximum = math.Abs(value - expected); !numeric || maximum > 1e-4 {
			t.Fatalf("decision differs from the official server: %v / %v", actual, expected)
		}
	case map[string]any:
		value, _ := actual.(map[string]any)
		if len(value) != len(expected) {
			t.Fatalf("answer keys differ: %v / %v", actual, expected)
		}
		for key, item := range expected {
			found, present := value[key]
			if !present {
				t.Fatalf("answer key %s is missing", key)
			}
			maximum = max(maximum, decisionDifference(t, found, item))
		}
	case []any:
		value, _ := actual.([]any)
		if len(value) != len(expected) {
			t.Fatalf("answer lists differ: %v / %v", actual, expected)
		}
		for index, item := range expected {
			maximum = max(maximum, decisionDifference(t, value[index], item))
		}
	default:
		if !reflect.DeepEqual(actual, expected) {
			t.Fatalf("answers differ: %v / %v", actual, expected)
		}
	}
	return maximum
}

// TestLocalModelDecision answers the recorded requests through the Node's
// decide command with the Julia model installed from the hub, and reports
// the documented fault without a runtime, with the switch off and with no
// model selected. It must run before any test that stages the runtime.
func TestLocalModelDecision(t *testing.T) {
	requests, golden := decisionRequests(t)
	if _, err := os.Stat(filepath.Join(executableDirectory(t), "llama-runtime.json")); err == nil {
		t.Fatal("the decision runtime is already beside the test binary; the missing-runtime case cannot run")
	}
	hub := newModelHub(t)
	node, counts := localModelNode(t, hub, nil)
	decision := protocol.ModelsKindDecision
	decide := func(request protocol.DecisionRequest) link.Response {
		t.Helper()
		return localCommand(t, node, protocol.Command{Kind: "decide", Data: request}, localModelWait)
	}

	// No runtime beside the executable: unavailable, with no fallback.
	wantFault(t, "decide without a runtime", decide(requests[0]), protocol.ErrorCodeUnavailable, "")
	if availability := localModels(t, node, decision).Availability[decision]; availability.Kind != "unavailable" {
		t.Fatalf("decision availability without a runtime = %+v", availability)
	}
	local := requiredEnvironment(t, "SAILRY_DECISION_JULIA")
	unresolved := protocol.ModelsSource{Hub: protocol.ModelsHubHuggingFace, Repository: juliaRepository, Revision: juliaRevision}
	wantFault(t, "registration without a runtime", localCommand(t, node, protocol.Command{Kind: "register_local_model", Data: protocol.CommandRegisterLocalModelPayload{
		Definition: protocol.ModelsDefinition{Source: unresolved, Name: "Julia", License: "apache-2.0", Format: protocol.ModelsFormat{Kind: "decision", Data: &protocol.ModelsFormatDecisionPayload{}},
			Assets: []protocol.ModelsAsset{{Source: unresolved, Path: filepath.Base(local), Name: "model.gguf", Bytes: 1, Sha256: strings.Repeat("0", 64)}}}}}, localModelWait),
		protocol.ErrorCodeUnavailable, "")
	if loads, _, _, _ := counts.read(decision); loads != 0 {
		t.Fatalf("%d decision engines were loaded without a runtime", loads)
	}

	stageDecisionRuntime(t)
	if availability := localModels(t, node, decision).Availability[decision]; availability.Kind != "available" {
		t.Fatalf("decision availability with the runtime = %+v", availability)
	}

	// The switch is on by default with no model selected.
	if choice := localSettings(t, node).Choices[decision]; !choice.Enabled || choice.Model != nil {
		t.Fatalf("initial decision choice = %+v", choice)
	}
	wantFault(t, "decide without a selected model", decide(requests[0]), protocol.ErrorCodeNotConfigured, "a decision model must be selected")

	id := installJulia(t, node, hub)
	if loads, validated, _, _ := counts.read(decision); loads != 1 || validated != 1 {
		t.Fatalf("decision installation loaded %d engines (%d validating)", loads, validated)
	}
	// Installed but still not selected.
	wantFault(t, "decide with an installed, unselected model", decide(requests[0]), protocol.ErrorCodeNotConfigured, "a decision model must be selected")

	saveChoice(t, node, decision, protocol.ModelsLocalChoice{Enabled: false, Model: &id})
	wantFault(t, "decide with the switch off", decide(requests[0]), protocol.ErrorCodeNotConfigured, "local decision service is disabled")

	saved := saveChoice(t, node, decision, protocol.ModelsLocalChoice{Enabled: true, Model: &id})
	if choice := saved.Choices[decision]; !choice.Enabled || choice.Model == nil || *choice.Model != id {
		t.Fatalf("saved decision choice = %+v", choice)
	}
	maximum := 0.0
	for index, request := range requests {
		response := decide(request)
		if response.Fault != nil || response.Output.Kind != "decision" {
			t.Fatalf("request %d: %+v", index, response)
		}
		maximum = max(maximum, sameDecision(t, payload[protocol.DecisionResponse](t, response.Output.Data), golden[index]))
	}
	t.Logf("%d requests; maximum probability/score difference from the b11756 server: %g", len(requests), maximum)

	// A request may name its model while none is selected.
	saveChoice(t, node, decision, protocol.ModelsLocalChoice{Enabled: true})
	named := requests[0]
	named.Model = &id
	response := decide(named)
	if response.Fault != nil {
		t.Fatalf("decide with a named model: %v", response.Fault)
	}
	sameDecision(t, payload[protocol.DecisionResponse](t, response.Output.Data), golden[0])
	unknown := requests[0]
	unknown.Model = new("not-registered")
	if response := decide(unknown); response.Fault == nil {
		t.Fatalf("an unregistered model answered: %+v", response.Output)
	}
}

// TestLocalModelScheduler runs the real OCR and decision engines on the
// Node's scheduler with an injected clock: an idle engine is released after
// the idle period and reloaded by the next call, and interactive commands
// run before queued background work.
func TestLocalModelScheduler(t *testing.T) {
	requests, golden := decisionRequests(t)
	stageDecisionRuntime(t)
	hub := newModelHub(t)
	clock := clockwork.NewFakeClock()
	node, counts := localModelNode(t, hub, func(options *Options) { options.LocalModels.Clock = clock })
	registry := node.localModels.registry
	fixtures := requiredEnvironment(t, "SAILRY_LOCALMODELS_FIXTURES")
	images := recordedPages(t, filepath.Join(fixtures, "rust-ocr.json"))
	worktree := projectWorktree(t, node, documentProject(t, node, "english.png"))
	recognition, decision := protocol.ModelsKindOcr, protocol.ModelsKindDecision

	recognizer := installOCR(t, node, hub)
	julia := installJulia(t, node, hub)
	saveChoice(t, node, decision, protocol.ModelsLocalChoice{Enabled: true, Model: &julia})
	decide := protocol.Command{Kind: "decide", Data: requests[0]}
	answered := func(response link.Response) {
		t.Helper()
		if response.Fault != nil {
			t.Fatalf("decide: %v", response.Fault)
		}
		sameDecision(t, payload[protocol.DecisionResponse](t, response.Output.Data), golden[0])
	}
	read := func(response link.Response) {
		t.Helper()
		if response.Fault != nil {
			t.Fatalf("read_office: %v", response.Fault)
		}
		inspection := payload[protocol.OfficeInspection](t, response.Output.Data)
		if len(inspection.Sections) != 1 || inspection.Sections[0].Text != images[0] {
			t.Fatalf("image inspection = %+v", inspection)
		}
	}
	loaded := func(kind protocol.ModelsKind) (int, int) {
		loads, _, releases, _ := counts.read(kind)
		return loads, releases
	}

	// Both engines stay loaded after their installation and are reused.
	answered(localCommand(t, node, decide, localModelWait))
	read(localCommand(t, node, readOffice(worktree, "english.png"), localModelWait))
	for _, kind := range []protocol.ModelsKind{recognition, decision} {
		if loads, releases := loaded(kind); loads != 1 || releases != 0 {
			t.Fatalf("%s before the idle period: %d loads, %d releases", kind, loads, releases)
		}
	}

	// One second short of the idle period nothing is released; at the
	// period both engines are.
	bounded, stop := context.WithTimeout(t.Context(), localModelWait)
	defer stop()
	if err := clock.BlockUntilContext(bounded, 1); err != nil {
		t.Fatalf("the scheduler did not wait on its idle timer: %v", err)
	}
	clock.Advance(299 * time.Second)
	time.Sleep(200 * time.Millisecond)
	for _, kind := range []protocol.ModelsKind{recognition, decision} {
		if _, releases := loaded(kind); releases != 0 {
			t.Fatalf("%s was released before the idle period", kind)
		}
	}
	clock.Advance(time.Second)
	eventually(t, "the idle engines to be released", func() bool {
		_, first := loaded(recognition)
		_, second := loaded(decision)
		return first == 1 && second == 1
	})

	// The next calls load the engines again and answer as before.
	answered(localCommand(t, node, decide, localModelWait))
	read(localCommand(t, node, readOffice(worktree, "english.png"), localModelWait))
	for _, kind := range []protocol.ModelsKind{recognition, decision} {
		if loads, releases := loaded(kind); loads != 2 || releases != 1 {
			t.Fatalf("%s after the idle period: %d loads, %d releases", kind, loads, releases)
		}
	}

	// Interactive before background. A background operation holds the
	// worker while another background operation and then two interactive
	// commands queue. The queued background operation must find both
	// interactive commands already answered; were it run first, it would
	// hold the worker and they could not complete.
	data, err := os.ReadFile(filepath.Join(fixtures, "ocr", "english.png"))
	if err != nil {
		t.Fatal(err)
	}
	picture, err := ocr.Decode(data)
	if err != nil {
		t.Fatal(err)
	}
	resource, err := registry.Resource(recognition, recognizer, nil)
	if err != nil {
		t.Fatal(err)
	}
	recognize := func(engine any) (string, error) {
		lines, err := engine.(*ocr.Engine).Recognize(picture, func() bool { return false })
		if err != nil {
			return "", err
		}
		texts := make([]string, len(lines))
		for index, line := range lines {
			texts[index] = line.Text
		}
		return strings.Join(texts, "\n"), nil
	}
	bounded, stop = context.WithTimeout(t.Context(), localModelWait)
	defer stop()
	started, hold, interactive := make(chan struct{}), make(chan struct{}), make(chan struct{})
	type result struct {
		text string
		err  error
	}
	first, second := make(chan result, 1), make(chan result, 1)
	go func() {
		text, err := localmodels.Execute(bounded, registry, resource, localmodels.Background, func(_ context.Context, engine any) (string, error) {
			close(started)
			<-hold
			return recognize(engine)
		})
		first <- result{text, err}
	}()
	select {
	case <-started:
	case <-bounded.Done():
		t.Fatal("the background operation did not start")
	}
	go func() {
		text, err := localmodels.Execute(bounded, registry, resource, localmodels.Background, func(ctx context.Context, engine any) (string, error) {
			select {
			case <-interactive:
			case <-ctx.Done():
				return "", errors.New("background work ran before the interactive commands were answered")
			}
			return recognize(engine)
		})
		second <- result{text, err}
	}()
	// The scheduler's queues are not visible from this package: give the
	// second background operation time to queue before the interactive
	// commands, and those time to queue before the worker is let go.
	time.Sleep(300 * time.Millisecond)
	deciding := localStart(t, bounded, node, decide)
	reading := localStart(t, bounded, node, readOffice(worktree, "english.png"))
	time.Sleep(300 * time.Millisecond)
	select {
	case response := <-deciding:
		t.Fatalf("a command was answered while the worker was held: %+v", response)
	case response := <-reading:
		t.Fatalf("a command was answered while the worker was held: %+v", response)
	case outcome := <-second:
		t.Fatalf("queued background work finished while the worker was held: %+v", outcome)
	default:
	}
	close(hold)
	answered(localFinish(t, "decide", deciding, localModelWait))
	read(localFinish(t, "read_office", reading, localModelWait))
	close(interactive)
	for name, outcome := range map[string]chan result{"held": first, "queued": second} {
		select {
		case finished := <-outcome:
			if finished.err != nil || finished.text != images[0] {
				t.Fatalf("%s background operation = %q, %v", name, finished.text, finished.err)
			}
		case <-bounded.Done():
			t.Fatalf("the %s background operation did not finish", name)
		}
	}
}
