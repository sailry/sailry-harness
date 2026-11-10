package localmodels

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"fmt"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"runtime"
	"slices"
	"strings"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	"github.com/jonboulle/clockwork"

	"github.com/sailry/sailry-harness/internal/protocol"
)

const (
	pinnedRevision = "0123456789abcdef0123456789abcdef01234567"
	otherRevision  = "89abcdef0123456789abcdef0123456789abcdef"
)

// decisionModel pins one small decision model file.
func decisionModel(body []byte, revision string, size uint64) protocol.ModelsDefinition {
	digest := sha256.Sum256(body)
	source := protocol.ModelsSource{Hub: protocol.ModelsHubHuggingFace, Repository: "org/model", Revision: revision}
	return protocol.ModelsDefinition{
		Source:  source,
		Name:    "Fixture",
		License: "mit",
		Format:  protocol.ModelsFormat{Kind: "decision", Data: protocol.ModelsFormatDecisionPayload{}},
		Assets:  []protocol.ModelsAsset{{Source: source, Path: "model.gguf", Name: "model.gguf", Bytes: size, Sha256: hex.EncodeToString(digest[:])}},
	}
}

// engines is a decision runtime that records loads and releases.
type engines struct {
	loads    atomic.Int64
	released chan string
}

func newEngines() *engines { return &engines{released: make(chan string, 16)} }

func (runtime *engines) capabilities() Capabilities {
	return Capabilities{protocol.ModelsKindDecision: {Load: func(resource Resource, _ bool) (Engine, error) {
		runtime.loads.Add(1)
		return Engine{Value: resource.ID, Release: func() { runtime.released <- resource.ID }}, nil
	}}}
}

// configured starts a registry whose Hugging Face mirror is the test server.
func configured(t *testing.T, server *httptest.Server, capabilities Capabilities, definition protocol.ModelsDefinition) (*Registry, string, error) {
	t.Helper()
	registry := New(Options{Profile: t.TempDir(), Capabilities: capabilities, Transport: server.Client().Transport, Clock: clockwork.NewFakeClock()})
	t.Cleanup(registry.Shutdown)
	settings := protocol.DefaultModelsLocalSettings()
	settings.Mirror = &server.URL
	id := ID(&definition)
	return registry, id, registry.Configure(settings, []Registration{{ID: id, Definition: definition}})
}

func state(registry *Registry, id string) string {
	for _, model := range registry.Cached(protocol.ModelsKindDecision) {
		if model.ID == id {
			return model.State.Kind
		}
	}
	return ""
}

func TestRegistryVerification(t *testing.T) {
	body := bytes.Repeat([]byte("sailry local model "), 512)
	flipped := slices.Clone(body)
	flipped[len(flipped)/2] ^= 0xff
	cases := []struct {
		name     string
		served   []byte
		revision string
		runtime  bool
		code     protocol.ErrorCode
		state    string
	}{
		{name: "verified", served: body, revision: pinnedRevision, runtime: true, state: "installed"},
		{name: "size", served: body[:len(body)-1], revision: pinnedRevision, runtime: true, code: protocol.ErrorCodeInvalidRequest, state: "missing"},
		{name: "sha256", served: flipped, revision: pinnedRevision, runtime: true, code: protocol.ErrorCodeInvalidRequest, state: "missing"},
		{name: "revision", served: body, revision: otherRevision, runtime: true, code: protocol.ErrorCodeUnavailable, state: "missing"},
		{name: "runtime absent", served: body, revision: pinnedRevision, code: protocol.ErrorCodeUnavailable, state: "missing"},
	}
	for _, test := range cases {
		t.Run(test.name, func(t *testing.T) {
			// The Hub serves the file only at its pinned revision.
			server := httptest.NewTLSServer(http.HandlerFunc(func(writer http.ResponseWriter, request *http.Request) {
				if request.URL.Path != "/org/model/resolve/"+pinnedRevision+"/model.gguf" {
					http.NotFound(writer, request)
					return
				}
				_, _ = writer.Write(test.served)
			}))
			t.Cleanup(server.Close)
			var capabilities Capabilities
			runtime := newEngines()
			if test.runtime {
				capabilities = runtime.capabilities()
			}
			registry, id, err := configured(t, server, capabilities, decisionModel(body, test.revision, uint64(len(body))))
			if err != nil {
				t.Fatalf("configure: %v", err)
			}
			err = registry.Install(t.Context(), id, nil)
			if code(err) != test.code || (err == nil) != (test.code == "") {
				t.Fatalf("install: got %v, want code %q", err, test.code)
			}
			if _, err := registry.List(protocol.ModelsKindDecision); err != nil {
				t.Fatalf("list: %v", err)
			}
			if got := state(registry, id); got != test.state {
				t.Fatalf("state: got %q, want %q", got, test.state)
			}
			resource, err := registry.Resource(protocol.ModelsKindDecision, id, nil)
			if test.state != "installed" {
				if code(err) != protocol.ErrorCodeNotConfigured {
					t.Fatalf("resource of a missing model: %v", err)
				}
				return
			}
			if err != nil {
				t.Fatalf("resource: %v", err)
			}
			value, err := Execute(t.Context(), registry, resource, Interactive, func(_ context.Context, engine any) (string, error) {
				return engine.(string), nil
			})
			if err != nil || value != id {
				t.Fatalf("execute: got %q, %v", value, err)
			}
			// Replaced contents of the same size fail the next inspection.
			path := filepath.Join(resource.Directory, "model.gguf")
			if err := os.Remove(path); err != nil {
				t.Fatal(err)
			}
			if err := os.WriteFile(path, flipped, 0o600); err != nil {
				t.Fatal(err)
			}
			if _, err := Execute(t.Context(), registry, resource, Interactive, func(context.Context, any) (string, error) { return "", nil }); code(err) != protocol.ErrorCodeRevisionConflict {
				t.Fatalf("execute on replaced files: %v", err)
			}
			if _, err := registry.List(protocol.ModelsKindDecision); err != nil {
				t.Fatalf("list: %v", err)
			}
			if got := state(registry, id); got != "missing" {
				t.Fatalf("state after replacement: got %q", got)
			}
		})
	}
	t.Run("start accepts receipts and hashes later", func(t *testing.T) {
		server := httptest.NewTLSServer(http.HandlerFunc(func(writer http.ResponseWriter, _ *http.Request) { _, _ = writer.Write(body) }))
		t.Cleanup(server.Close)
		definition := decisionModel(body, pinnedRevision, uint64(len(body)))
		installed, id, err := configured(t, server, newEngines().capabilities(), definition)
		if err == nil {
			err = installed.Install(t.Context(), id, nil)
		}
		if err != nil {
			t.Fatal(err)
		}
		resource, err := installed.Resource(protocol.ModelsKindDecision, id, nil)
		if err != nil {
			t.Fatal(err)
		}
		// restarted is a registry over the same files whose background
		// inspections never run, so only Start decides the state.
		restarted := func() *Registry {
			registry := New(Options{Profile: installed.profile, Capabilities: installed.capabilities, Clock: clockwork.NewFakeClock()})
			t.Cleanup(registry.Shutdown)
			registry.tasksMu.Lock()
			registry.closed = true
			registry.tasksMu.Unlock()
			if err := registry.Configure(protocol.DefaultModelsLocalSettings(), []Registration{{ID: id, Definition: definition}}); err != nil {
				t.Fatal(err)
			}
			if err := registry.Start(); err != nil {
				t.Fatal(err)
			}
			return registry
		}
		if got := state(restarted(), id); got != "installed" {
			t.Fatalf("state with a receipt: got %q", got)
		}
		receipt := filepath.Join(resource.Directory, ".verified")
		if err := os.Remove(receipt); err != nil {
			t.Fatal(err)
		}
		registry := restarted()
		if _, err := os.Lstat(receipt); state(registry, id) != "missing" || err == nil {
			t.Fatalf("start hashed files without a receipt: state %q, receipt %v", state(registry, id), err)
		}
		if _, err := registry.List(""); err != nil {
			t.Fatal(err)
		}
		if _, err := os.Lstat(receipt); state(registry, id) != "installed" || err != nil {
			t.Fatalf("after the later inspection: state %q, receipt %v", state(registry, id), err)
		}
	})
	t.Run("operations wait for the hash start left running", func(t *testing.T) {
		server := httptest.NewTLSServer(http.HandlerFunc(func(writer http.ResponseWriter, _ *http.Request) { _, _ = writer.Write(body) }))
		t.Cleanup(server.Close)
		definition := decisionModel(body, pinnedRevision, uint64(len(body)))
		installed, id, err := configured(t, server, newEngines().capabilities(), definition)
		if err == nil {
			err = installed.Install(t.Context(), id, nil)
		}
		if err != nil {
			t.Fatal(err)
		}
		resource, err := installed.Resource(protocol.ModelsKindDecision, id, nil)
		if err != nil {
			t.Fatal(err)
		}
		if err := os.Remove(filepath.Join(resource.Directory, ".verified")); err != nil {
			t.Fatal(err)
		}
		registry := New(Options{Profile: installed.profile, Capabilities: installed.capabilities, Clock: clockwork.NewFakeClock()})
		t.Cleanup(registry.Shutdown)
		// Configuration's own background inspection is kept from running,
		// then holding the task lock keeps Start's hash from starting, so
		// the window between its receipt pass and the hash stays open.
		registry.tasksMu.Lock()
		registry.closed = true
		registry.tasksMu.Unlock()
		if err := registry.Configure(protocol.DefaultModelsLocalSettings(), []Registration{{ID: id, Definition: definition}}); err != nil {
			t.Fatal(err)
		}
		registry.tasksMu.Lock()
		registry.closed = false
		started := make(chan error, 1)
		go func() { started <- registry.Start() }()
		for waiting := false; !waiting; runtime.Gosched() {
			registry.mu.Lock()
			waiting = registry.pending[id] != nil
			registry.mu.Unlock()
		}
		if got := state(registry, id); got != "missing" {
			t.Fatalf("state before the hash: got %q", got)
		}
		cancelled, cancel := context.WithCancel(t.Context())
		cancel()
		if err := registry.Settle(cancelled, id); code(err) != protocol.ErrorCodeCancelled {
			t.Fatalf("settle with a cancelled caller = %v", err)
		}
		if err := registry.Settle(t.Context(), "unregistered"); err != nil {
			t.Fatalf("settle without a pending model = %v", err)
		}
		settled := make(chan error, 1)
		go func() { settled <- registry.Settle(t.Context(), id) }()
		select {
		case err := <-settled:
			t.Fatalf("settle returned before the hash: %v", err)
		default:
		}
		registry.tasksMu.Unlock()
		if err := <-started; err != nil {
			t.Fatal(err)
		}
		if err := <-settled; err != nil {
			t.Fatal(err)
		}
		if _, err := registry.Resource(protocol.ModelsKindDecision, id, nil); err != nil || state(registry, id) != "installed" {
			t.Fatalf("after the wait: state %q, resource %v", state(registry, id), err)
		}
	})
	t.Run("unpinned revision", func(t *testing.T) {
		server := httptest.NewTLSServer(http.NotFoundHandler())
		t.Cleanup(server.Close)
		for _, revision := range []string{"main", strings.ToUpper(pinnedRevision[:39]) + "g", pinnedRevision[:39]} {
			if _, _, err := configured(t, server, newEngines().capabilities(), decisionModel(body, revision, uint64(len(body)))); code(err) != protocol.ErrorCodeInvalidRequest {
				t.Fatalf("revision %q: %v", revision, err)
			}
		}
	})
}

// gated starts a scheduler whose worker is held inside one operation until
// the returned release runs.
func gated(t *testing.T, ctx context.Context, runtime *engines, resource Resource) (*scheduler, func()) {
	t.Helper()
	scheduler := newScheduler(ctx, clockwork.NewFakeClock(), runtime.capabilities())
	t.Cleanup(scheduler.shutdown)
	entered, gate := make(chan struct{}), make(chan struct{})
	go func() {
		_, _ = scheduler.request(ctx, resource, Interactive, func(bool) error { return nil }, false, func(context.Context, any) (any, error) {
			close(entered)
			select {
			case <-gate:
			case <-ctx.Done():
			}
			return nil, nil
		})
	}()
	select {
	case <-entered:
	case <-ctx.Done():
		t.Fatal("the worker did not start")
	}
	return scheduler, func() { close(gate) }
}

// queued waits until the lanes hold the given numbers of operations.
func queued(t *testing.T, ctx context.Context, scheduler *scheduler, interactive, background int) {
	t.Helper()
	for {
		scheduler.mu.Lock()
		reached := len(scheduler.interactive) == interactive && len(scheduler.background) == background
		scheduler.mu.Unlock()
		if reached {
			return
		}
		select {
		case <-ctx.Done():
			t.Fatal("operations were not queued")
		case <-time.After(time.Millisecond):
		}
	}
}

func TestSchedulerInteractiveFirst(t *testing.T) {
	definition := decisionModel([]byte("model"), pinnedRevision, 5)
	resource := Resource{ID: "model", Definition: &definition, Scope: context.Background()}
	current := func(bool) error { return nil }
	cases := []struct {
		name     string
		arrivals []string
		want     []string
	}{
		{name: "background queued first", arrivals: []string{"b1", "b2", "i1", "i2"}, want: []string{"i1", "i2", "b1", "b2"}},
		{name: "interleaved", arrivals: []string{"b1", "i1", "b2", "i2"}, want: []string{"i1", "i2", "b1", "b2"}},
		{name: "interactive keeps its order", arrivals: []string{"i1", "i2", "i3"}, want: []string{"i1", "i2", "i3"}},
		{name: "background keeps its order", arrivals: []string{"b1", "b2", "b3"}, want: []string{"b1", "b2", "b3"}},
	}
	for _, test := range cases {
		t.Run(test.name, func(t *testing.T) {
			ctx, cancel := context.WithTimeout(t.Context(), 30*time.Second)
			defer cancel()
			scheduler, release := gated(t, ctx, newEngines(), resource)
			var mu sync.Mutex
			var order []string
			var running, overlapped atomic.Int64
			var finished sync.WaitGroup
			var interactive, background int
			for _, name := range test.arrivals {
				lane := Interactive
				if strings.HasPrefix(name, "b") {
					lane = Background
					background++
				} else {
					interactive++
				}
				finished.Go(func() {
					_, err := scheduler.request(ctx, resource, lane, current, false, func(context.Context, any) (any, error) {
						if running.Add(1) > 1 {
							overlapped.Add(1)
						}
						mu.Lock()
						order = append(order, name)
						mu.Unlock()
						running.Add(-1)
						return nil, nil
					})
					if err != nil {
						t.Errorf("%s: %v", name, err)
					}
				})
				queued(t, ctx, scheduler, interactive, background)
			}
			release()
			finished.Wait()
			if !slices.Equal(order, test.want) || overlapped.Load() != 0 {
				t.Fatalf("order: got %v (overlaps %d), want %v", order, overlapped.Load(), test.want)
			}
		})
	}
	t.Run("full lanes report busy", func(t *testing.T) {
		ctx, cancel := context.WithTimeout(t.Context(), 30*time.Second)
		defer cancel()
		scheduler, release := gated(t, ctx, newEngines(), resource)
		var finished sync.WaitGroup
		wait := func(lane Lane) {
			finished.Go(func() {
				_, _ = scheduler.request(ctx, resource, lane, current, false, func(context.Context, any) (any, error) { return nil, nil })
			})
		}
		for index := range interactiveCapacity {
			wait(Interactive)
			queued(t, ctx, scheduler, index+1, 0)
		}
		for index := range backgroundCapacity {
			wait(Background)
			queued(t, ctx, scheduler, interactiveCapacity, index+1)
		}
		for _, lane := range []Lane{Interactive, Background} {
			if _, err := scheduler.request(ctx, resource, lane, current, false, func(context.Context, any) (any, error) { return nil, nil }); code(err) != protocol.ErrorCodeBusy {
				t.Fatalf("lane %v: %v", lane, err)
			}
		}
		release()
		finished.Wait()
	})
}

func TestIdleRelease(t *testing.T) {
	model := func(id string, size uint64) Resource {
		definition := decisionModel([]byte(id), pinnedRevision, size)
		return Resource{ID: id, Definition: &definition, Scope: context.Background()}
	}
	small := model("small", 5)
	type step struct {
		// use runs one operation on the model; advance moves the clock; unload
		// drops the model; released names the engine a step must release.
		use      *Resource
		advance  time.Duration
		unload   string
		released string
		loads    int64
	}
	first, second := model("first", memoryBytes/2+1), model("second", memoryBytes/2)
	cases := []struct {
		name  string
		steps []step
	}{
		{name: "released after the idle period", steps: []step{
			{use: &small, loads: 1},
			{advance: idle, released: "small", loads: 1},
			{use: &small, loads: 2},
		}},
		{name: "use restarts the idle period", steps: []step{
			{use: &small, loads: 1},
			{advance: idle - time.Second, loads: 1},
			{use: &small, loads: 1},
			{advance: idle - time.Second, loads: 1},
			{use: &small, loads: 1},
			{advance: idle, released: "small", loads: 1},
		}},
		{name: "unload releases at once", steps: []step{
			{use: &small, loads: 1},
			{unload: "small", released: "small", loads: 1},
			{use: &small, loads: 2},
		}},
		{name: "memory bound releases the least recently used", steps: []step{
			{use: &first, loads: 1},
			{use: &second, released: "first", loads: 2},
			{use: &second, loads: 2},
		}},
	}
	for _, test := range cases {
		t.Run(test.name, func(t *testing.T) {
			ctx, cancel := context.WithTimeout(t.Context(), 30*time.Second)
			defer cancel()
			clock := clockwork.NewFakeClock()
			runtime := newEngines()
			scheduler := newScheduler(ctx, clock, runtime.capabilities())
			t.Cleanup(scheduler.shutdown)
			for index, step := range test.steps {
				// The idle worker always waits on one timer before a step runs.
				if err := clock.BlockUntilContext(ctx, 1); err != nil {
					t.Fatalf("step %d: the worker did not become idle", index)
				}
				switch {
				case step.use != nil:
					value, err := scheduler.request(ctx, *step.use, Interactive, func(bool) error { return nil }, false, func(_ context.Context, engine any) (any, error) {
						return engine, nil
					})
					if err != nil || value != step.use.ID {
						t.Fatalf("step %d: got %v, %v", index, value, err)
					}
				case step.unload != "":
					if err := scheduler.unload(step.unload); err != nil {
						t.Fatalf("step %d: %v", index, err)
					}
				default:
					clock.Advance(step.advance)
				}
				if step.released != "" {
					select {
					case released := <-runtime.released:
						if released != step.released {
							t.Fatalf("step %d: released %q, want %q", index, released, step.released)
						}
					case <-ctx.Done():
						t.Fatalf("step %d: %q was not released", index, step.released)
					}
				}
				if got := runtime.loads.Load(); got != step.loads {
					t.Fatalf("step %d: %d loads, want %d", index, got, step.loads)
				}
			}
			if len(runtime.released) != 0 {
				t.Fatalf("unexpected release of %q", <-runtime.released)
			}
		})
	}
}

func TestDownloadCancel(t *testing.T) {
	body := bytes.Repeat([]byte("resumable local model "), 1024)
	half := len(body) / 2
	cases := []struct {
		name   string
		cancel func(registry *Registry, id string, caller context.CancelFunc)
	}{
		{name: "caller", cancel: func(_ *Registry, _ string, caller context.CancelFunc) { caller() }},
		{name: "cancel command", cancel: func(registry *Registry, id string, _ context.CancelFunc) {
			if !registry.Cancel(id) {
				t.Error("no active installation to cancel")
			}
		}},
	}
	for _, test := range cases {
		t.Run(test.name, func(t *testing.T) {
			var stalled atomic.Bool
			stalled.Store(true)
			var mu sync.Mutex
			var ranges []string
			// The first response stalls after half of the file; later ones serve ranges.
			server := httptest.NewTLSServer(http.HandlerFunc(func(writer http.ResponseWriter, request *http.Request) {
				mu.Lock()
				ranges = append(ranges, request.Header.Get("Range"))
				mu.Unlock()
				writer.Header().Set("ETag", `"fixture"`)
				if !stalled.Load() {
					http.ServeContent(writer, request, "", time.Time{}, bytes.NewReader(body))
					return
				}
				writer.Header().Set("Content-Length", fmt.Sprint(len(body)))
				_, _ = writer.Write(body[:half])
				writer.(http.Flusher).Flush()
				<-request.Context().Done()
			}))
			t.Cleanup(server.Close)
			runtime := newEngines()
			registry, id, err := configured(t, server, runtime.capabilities(), decisionModel(body, pinnedRevision, uint64(len(body))))
			if err != nil {
				t.Fatalf("configure: %v", err)
			}
			ctx, cancel := context.WithTimeout(t.Context(), 30*time.Second)
			defer cancel()
			caller, stop := context.WithCancel(ctx)
			defer stop()
			received := make(chan struct{})
			var once sync.Once
			result := make(chan error, 1)
			go func() {
				result <- registry.Install(caller, id, func(phase protocol.InstallationPhase, progress *protocol.InstallationProgress) {
					if phase != protocol.InstallationPhaseDownloading || progress == nil {
						return
					}
					if data, ok := progress.Data.(protocol.InstallationProgressPercentPayload); ok && data.Percent > 0 {
						once.Do(func() { close(received) })
					}
				})
			}()
			select {
			case <-received:
			case err := <-result:
				t.Fatalf("install ended before receiving data: %v", err)
			case <-ctx.Done():
				t.Fatal("no data was received")
			}
			if got := state(registry, id); got != "downloading" {
				t.Fatalf("state during the transfer: got %q", got)
			}
			test.cancel(registry, id, stop)
			select {
			case err := <-result:
				if code(err) != protocol.ErrorCodeCancelled {
					t.Fatalf("cancelled install: %v", err)
				}
			case <-ctx.Done():
				t.Fatal("the install did not stop")
			}
			if got := state(registry, id); got != "missing" || runtime.loads.Load() != 0 {
				t.Fatalf("after cancellation: state %q, %d loads", got, runtime.loads.Load())
			}
			root, err := registry.Root()
			if err != nil {
				t.Fatal(err)
			}
			entries, err := os.ReadDir(filepath.Join(root, "decision", id))
			if err != nil {
				t.Fatal(err)
			}
			var names []string
			for _, entry := range entries {
				names = append(names, entry.Name())
			}
			if len(names) != 2 || !strings.HasSuffix(names[0], ".part") || !strings.HasSuffix(names[1], ".part.json") {
				t.Fatalf("checkpoint files: %v", names)
			}
			// A later install continues from the checkpoint with a ranged request.
			stalled.Store(false)
			if err := registry.Install(ctx, id, nil); err != nil {
				t.Fatalf("resumed install: %v", err)
			}
			mu.Lock()
			defer mu.Unlock()
			if len(ranges) != 2 || ranges[0] != "" || !strings.HasPrefix(ranges[1], "bytes=") || ranges[1] == "bytes=0-" {
				t.Fatalf("range requests: %q", ranges)
			}
			if got := state(registry, id); got != "installed" || runtime.loads.Load() != 1 {
				t.Fatalf("after resuming: state %q, %d loads", got, runtime.loads.Load())
			}
		})
	}
}
