package live

import (
	"bytes"
	"context"
	"io"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/plugins"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
	"lukechampine.com/blake3"
)

// connectionFixtureTransport is a deterministic Node for saved connections:
// it publishes one snapshot and answers database browsing and SSH transfers.
type connectionFixtureTransport struct {
	mu        sync.Mutex
	snapshot  protocol.Snapshot
	requests  []protocol.Request
	catalogs  map[string]protocol.DatabaseCatalog
	failure   protocol.ErrorCode
	gate      chan struct{}
	conflict  bool
	uncertain bool
	specs     map[protocol.StreamID]protocol.SshUploadSpec
	uploaded  map[protocol.StreamID][]byte
	outcomes  map[protocol.RequestID]protocol.Output
	remote    map[string][]byte
	listings  map[string][]protocol.FileEntry
	downloads map[protocol.StreamID][]byte
	admitted  chan struct{}
}

func newConnectionFixtureTransport(snapshot protocol.Snapshot) *connectionFixtureTransport {
	return &connectionFixtureTransport{
		snapshot: snapshot, catalogs: map[string]protocol.DatabaseCatalog{}, specs: map[protocol.StreamID]protocol.SshUploadSpec{},
		uploaded: map[protocol.StreamID][]byte{}, outcomes: map[protocol.RequestID]protocol.Output{}, remote: map[string][]byte{},
		listings: map[string][]protocol.FileEntry{}, downloads: map[protocol.StreamID][]byte{}, admitted: make(chan struct{}, 1),
	}
}

func (f *connectionFixtureTransport) Target() protocol.NodeID { return f.snapshot.Node }
func (f *connectionFixtureTransport) WatchConnection(ctx context.Context, changed func(link.ConnectionInfo)) error {
	return link.NewLocal(f.Target(), f.Target(), nil).WatchConnection(ctx, changed)
}
func (f *connectionFixtureTransport) Subscribe(context.Context, protocol.Topic) (link.Subscription, error) {
	subscription := &conversationTestSubscription{updates: make(chan protocol.Update, 1), closed: make(chan struct{})}
	subscription.updates <- protocol.Update{Kind: "snapshot", Data: f.snapshot}
	return subscription, nil
}
func (f *connectionFixtureTransport) Open(_ context.Context, id protocol.StreamID) (link.Stream, error) {
	controller, server := link.StreamPair()
	f.mu.Lock()
	body, download := f.downloads[id]
	f.mu.Unlock()
	go func() {
		defer server.Close()
		if download {
			_, _ = io.Copy(io.Discard, server)
			_, _ = server.Write(body)
			_ = server.CloseWrite()
			return
		}
		received, err := io.ReadAll(server)
		if err == nil {
			f.mu.Lock()
			f.uploaded[id] = received
			f.mu.Unlock()
			if _, err = server.Write([]byte{protocol.FILE_UPLOAD_READY}); err == nil {
				_ = server.CloseWrite()
			}
		}
	}()
	return controller, nil
}
func (f *connectionFixtureTransport) Dispatch(ctx context.Context, request protocol.Request) (link.Admission, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	f.requests = append(f.requests, request)
	select {
	case f.admitted <- struct{}{}:
	default:
	}
	response := link.Response{}
	sshOutcome := func(kind string, data any) protocol.Output {
		return protocol.Output{Kind: "ssh_outcome", Data: protocol.SshOutcome{Kind: kind, Data: data}}
	}
	switch request.Command.Kind {
	case "browse_database":
		input, _ := detail[protocol.CommandBrowseDatabasePayload](request.Command.Data)
		key := input.Profile.String()
		if input.Database != nil {
			key += "/" + *input.Database
		}
		catalog, exists := f.catalogs[key]
		if f.failure != "" || !exists {
			response.Fault = &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "database is unavailable"}
		} else {
			response.Output = protocol.Output{Kind: "database_outcome", Data: protocol.DatabaseOutcome{Kind: "catalog", Data: catalog}}
		}
	case "stage_ssh_upload":
		spec, _ := detail[protocol.SshUploadSpec](request.Command.Data)
		if f.conflict && !spec.Overwrite {
			response.Fault = &protocol.Fault{Code: protocol.ErrorCodeConflict, Message: "destination exists"}
		} else {
			stream := protocol.NewStreamID()
			f.specs[stream] = spec
			response.Output = protocol.Output{Kind: "ssh_upload", Data: protocol.SshUpload{Stream: stream, Spec: spec}}
		}
	case "finish_ssh_upload":
		input, _ := detail[protocol.CommandFinishSshUploadPayload](request.Command.Data)
		response.Output = sshOutcome("transferred", protocol.SshOutcomeTransferredPayload{Bytes: f.specs[input.Stream].Size})
		f.outcomes[request.ID] = response.Output
		if f.uncertain {
			response.Fault = &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "publication observation lost"}
		}
	case "modify_ssh_file":
		response.Output = sshOutcome("files_changed", nil)
		f.outcomes[request.ID] = response.Output
	case "browse_ssh_directory":
		input, _ := detail[protocol.CommandBrowseSshDirectoryPayload](request.Command.Data)
		response.Output = sshOutcome("directory", protocol.SshDirectory{Path: input.Path, Entries: f.listings[input.Path]})
	case "download_ssh_file":
		input, _ := detail[protocol.CommandDownloadSshFilePayload](request.Command.Data)
		body, exists := f.remote[input.Path]
		if !exists {
			response.Fault = &protocol.Fault{Code: protocol.ErrorCodeNotFound, Message: "remote file is missing"}
			break
		}
		stream := protocol.NewStreamID()
		f.downloads[stream] = body
		digest := blake3.Sum256(body)
		response.Output = sshOutcome("download", protocol.SshDownload{Stream: stream, Path: input.Path, Size: uint64(len(body)), Revision: fmtAttachmentDigest(digest[:])})
	case "inspect_request":
		input, _ := detail[protocol.CommandInspectRequestPayload](request.Command.Data)
		outcome := protocol.RequestOutcome{Kind: "not_admitted"}
		if output, exists := f.outcomes[input.ID]; exists {
			outcome = protocol.RequestOutcome{Kind: "completed", Data: protocol.Result[protocol.Output, protocol.Fault]{Ok: &output}}
		}
		response.Output = protocol.Output{Kind: "request_outcome", Data: protocol.OutputRequestOutcomePayload{ID: input.ID, Outcome: outcome}}
	case "cancel_file_transfer":
		response.Output = protocol.Output{Kind: "file_transfer_cancelled"}
	default:
		response.Fault = &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "unexpected fixture command"}
	}
	completion := make(chan link.Response, 1)
	if gate := f.gate; gate != nil && request.Command.Kind == "browse_database" {
		go func() {
			select {
			case <-gate:
				completion <- response
			case <-ctx.Done():
			}
		}()
	} else {
		completion <- response
	}
	return link.Admission{Receipt: protocol.Receipt{ID: request.ID, Durable: request.Command.Durable()}, Completion: completion}, nil
}

// await blocks until the Node admitted the given number of commands of one kind.
func (f *connectionFixtureTransport) await(ctx context.Context, kind string, total int) bool {
	for f.count(kind) < total {
		select {
		case <-f.admitted:
		case <-ctx.Done():
			return false
		}
	}
	return f.count(kind) == total
}

func (f *connectionFixtureTransport) count(kind string) int {
	f.mu.Lock()
	defer f.mu.Unlock()
	total := 0
	for _, request := range f.requests {
		if request.Command.Kind == kind {
			total++
		}
	}
	return total
}

// testSshTransferBridge covers the SSH transfer store and workers behind the
// `sailry/ssh-transfers` module. Register it under an existing controller IO
// test: t.Run("ssh transfers", testSshTransferBridge).
func testSshTransferBridge(t *testing.T) {
	node := protocol.NodeID{1}
	reference := protocol.PluginReference{Name: "dev.sailry.platform", Digest: strings.Repeat("a", 64), SettingsRevision: 2}
	profile := protocol.SshProfile{ID: protocol.SshID{7}, Revision: 3, Name: "build", Host: "build.example", Port: 22, Username: "ci", Authentication: protocol.SshAuthenticationPassword}
	scope := protocol.DefaultPluginContext()
	scope.Package, scope.Surface = reference, protocol.PluginDesktopSurface{Kind: "workspace"}
	payload := []byte{'b', 0, 0xff, 'y', 't', 'e', 's'}

	type fixture struct {
		transport *connectionFixtureTransport
		runtime   *Runtime
		store     *sshTransferStore
		access    sshTransferAccess
		ctx       context.Context
	}
	start := func(t *testing.T) *fixture {
		ctx, cancel := context.WithTimeout(t.Context(), 30*time.Second)
		snapshot := protocol.DefaultSnapshot()
		snapshot.Node, snapshot.Ssh = node, []protocol.SshProfile{profile}
		transport := newConnectionFixtureTransport(snapshot)
		controller := client.New(transport)
		if err := controller.Start(ctx); err != nil {
			cancel()
			t.Fatal(err)
		}
		runtime := &Runtime{ctx: ctx, controller: controller}
		t.Cleanup(func() { cancel(); runtime.workers.Wait(); controller.Close() })
		return &fixture{transport: transport, runtime: runtime, store: runtime.sshTransferStore(), access: sshTransferAccess{owner: runtime, scope: scope, profile: profile}, ctx: ctx}
	}
	settle := func(t *testing.T, f *fixture, id protocol.RequestID, done func(map[string]any) bool) map[string]any {
		t.Helper()
		for {
			values, changed := f.store.values(node, reference)
			for _, value := range values {
				if value["id"] == id && done(value) {
					return value
				}
			}
			select {
			case <-changed:
			case <-f.ctx.Done():
				t.Fatalf("transfer did not settle: %+v", values)
			}
		}
	}
	stage := func(want string) func(map[string]any) bool {
		return func(value map[string]any) bool { return value["stage"] == want }
	}
	source := func(t *testing.T) string {
		path := filepath.Join(t.TempDir(), "source.bin")
		if err := os.WriteFile(path, payload, 0o600); err != nil {
			t.Fatal(err)
		}
		return path
	}

	t.Run("upload keeps scope and bytes", func(t *testing.T) {
		f := start(t)
		ids, err := f.store.insertUploads(f.access, "/srv/", []string{source(t)})
		if err != nil || len(ids) != 1 {
			t.Fatal("selected source was not queued", err)
		}
		ready := settle(t, f, ids[0], stage("ready"))
		if ready["kind"] != "upload" || ready["source"] != "source.bin" || ready["path"] != "/srv/source.bin" || ready["can_start"] != true || ready["can_cancel"] != false {
			t.Fatalf("queued upload snapshot: %+v", ready)
		}
		for key, value := range ready {
			if text, ok := value.(string); ok && key != "source" && key != "path" && strings.Contains(text, string(filepath.Separator)+"source.bin") {
				t.Fatalf("snapshot exposed a local path in %q", key)
			}
		}
		if err := f.store.action(ids[0], "replace"); err == nil {
			t.Fatal("a ready upload accepted replace")
		}
		if err := f.store.action(ids[0], "start"); err != nil {
			t.Fatal(err)
		}
		done := settle(t, f, ids[0], stage("done"))
		if done["can_start"] != false || done["error"] != nil {
			t.Fatalf("finished upload snapshot: %+v", done)
		}
		f.transport.mu.Lock()
		defer f.transport.mu.Unlock()
		if len(f.transport.uploaded) != 1 {
			t.Fatal("upload did not stage exactly one stream")
		}
		for stream, body := range f.transport.uploaded {
			spec := f.transport.specs[stream]
			if !bytes.Equal(body, payload) || spec.Profile != profile.ID || spec.ExpectedRevision != profile.Revision || spec.Path != "/srv/source.bin" || spec.Overwrite {
				t.Fatalf("staged upload changed: %+v", spec)
			}
		}
		for _, request := range f.transport.requests {
			switch request.Command.Kind {
			case "stage_ssh_upload", "finish_ssh_upload":
				if request.Plugin == nil || request.Plugin.Package != reference {
					t.Fatalf("%s lost the plugin scope", request.Command.Kind)
				}
			}
		}
	})
	t.Run("directory is created before its files", func(t *testing.T) {
		f := start(t)
		root := filepath.Join(t.TempDir(), "tree")
		if err := os.Mkdir(root, 0o700); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(filepath.Join(root, "inner.bin"), payload, 0o600); err != nil {
			t.Fatal(err)
		}
		ids, err := f.store.insertUploads(f.access, "/srv", []string{root})
		if err != nil {
			t.Fatal(err)
		}
		if err := f.store.action(ids[0], "start"); err != nil {
			t.Fatal(err)
		}
		settle(t, f, ids[0], stage("done"))
		f.transport.mu.Lock()
		defer f.transport.mu.Unlock()
		var order []string
		for _, request := range f.transport.requests {
			switch request.Command.Kind {
			case "modify_ssh_file":
				input, _ := detail[protocol.CommandModifySshFilePayload](request.Command.Data)
				order = append(order, "directory "+input.Path)
			case "stage_ssh_upload":
				input, _ := detail[protocol.SshUploadSpec](request.Command.Data)
				order = append(order, "file "+input.Path)
			}
		}
		if strings.Join(order, ",") != "directory /srv/tree,file /srv/tree/inner.bin" {
			t.Fatal("upload order", order)
		}
	})
	t.Run("uncertain publication is checked by its original request", func(t *testing.T) {
		f := start(t)
		f.transport.uncertain = true
		ids, _ := f.store.insertUploads(f.access, "/srv", []string{source(t)})
		if err := f.store.action(ids[0], "start"); err != nil {
			t.Fatal(err)
		}
		uncertain := settle(t, f, ids[0], func(value map[string]any) bool { return value["stage"] == "uncertain" && value["can_check"] == true })
		if uncertain["can_start"] != false || uncertain["can_replace"] != false {
			t.Fatalf("uncertain upload can be replayed: %+v", uncertain)
		}
		if err := f.store.action(ids[0], "start"); err == nil {
			t.Fatal("uncertain upload restarted staged IO")
		}
		if err := f.store.action(ids[0], "check"); err != nil {
			t.Fatal(err)
		}
		settle(t, f, ids[0], stage("done"))
		if finishes, inspections, stages := f.transport.count("finish_ssh_upload"), f.transport.count("inspect_request"), f.transport.count("stage_ssh_upload"); finishes != 1 || inspections != 1 || stages != 1 {
			t.Fatal("outcome inspection replayed staged IO", finishes, inspections, stages)
		}
	})
	t.Run("existing destination waits for replace", func(t *testing.T) {
		f := start(t)
		f.transport.conflict = true
		ids, _ := f.store.insertUploads(f.access, "/srv", []string{source(t)})
		if err := f.store.action(ids[0], "start"); err != nil {
			t.Fatal(err)
		}
		exists := settle(t, f, ids[0], func(value map[string]any) bool { return value["stage"] == "exists" && value["can_replace"] == true })
		if exists["can_start"] != false || f.transport.count("finish_ssh_upload") != 0 {
			t.Fatalf("conflict published or stayed startable: %+v", exists)
		}
		if err := f.store.action(ids[0], "replace"); err != nil {
			t.Fatal(err)
		}
		settle(t, f, ids[0], stage("done"))
		f.transport.mu.Lock()
		defer f.transport.mu.Unlock()
		if len(f.transport.specs) != 1 {
			t.Fatal("replacement staged more than one stream")
		}
		for _, spec := range f.transport.specs {
			if !spec.Overwrite {
				t.Fatal("replacement lost its overwrite decision")
			}
		}
	})
	t.Run("download publishes file and directory", func(t *testing.T) {
		f := start(t)
		f.transport.remote["/srv/report.bin"] = payload
		f.transport.remote["/srv/tree/inner.bin"] = payload
		f.transport.listings["/srv/tree"] = []protocol.FileEntry{{Name: "inner.bin", Kind: protocol.EntryKindFile, Size: uint64(len(payload))}}
		local := t.TempDir()
		file := f.store.insert(f.access, false, filepath.Join(local, "report.bin"), false, "/srv/report.bin")
		tree := f.store.insert(f.access, false, filepath.Join(local, "tree"), true, "/srv/tree")
		if queued := settle(t, f, tree, stage("ready")); queued["kind"] != "download_directory" || queued["source"] != "/srv/tree" {
			t.Fatalf("queued download snapshot: %+v", queued)
		}
		for _, id := range []protocol.RequestID{file, tree} {
			if err := f.store.action(id, "start"); err != nil {
				t.Fatal(err)
			}
			settle(t, f, id, stage("done"))
		}
		for _, path := range []string{filepath.Join(local, "report.bin"), filepath.Join(local, "tree", "inner.bin")} {
			if body, err := os.ReadFile(path); err != nil || !bytes.Equal(body, payload) {
				t.Fatal("downloaded bytes changed", path, err)
			}
		}
		existing := f.store.insert(f.access, false, filepath.Join(local, "report.bin"), false, "/srv/report.bin")
		if value := settle(t, f, existing, stage("exists")); value["can_replace"] != true || value["can_start"] != false {
			t.Fatalf("existing local destination snapshot: %+v", value)
		}
		missing := f.store.insert(f.access, false, filepath.Join(local, "missing.bin"), false, "/srv/missing.bin")
		if err := f.store.action(missing, "start"); err != nil {
			t.Fatal(err)
		}
		failed := settle(t, f, missing, stage("failed"))
		fault, ok := failed["error"].(protocol.Fault)
		if !ok || fault.Message != "files_download_failed" || failed["can_start"] != true {
			t.Fatalf("failed download snapshot: %+v", failed)
		}
		if _, err := os.Lstat(filepath.Join(local, "missing.bin")); err == nil {
			t.Fatal("failed download published a destination")
		}
		if err := f.store.action(missing, "dismiss"); err != nil {
			t.Fatal(err)
		}
		if values, _ := f.store.values(node, reference); len(values) != 3 {
			t.Fatal("dismissed transfer stayed listed", len(values))
		}
		if values, _ := f.store.values(protocol.NodeID{2}, reference); len(values) != 0 {
			t.Fatal("transfers leaked to another Node")
		}
		other := reference
		other.Name = "other.package"
		if values, _ := f.store.values(node, other); len(values) != 0 {
			t.Fatal("transfers leaked to another package")
		}
	})
	t.Run("access requires the declared action and current profile", func(t *testing.T) {
		f := start(t)
		native := plugins.NewNativeBinding(f.ctx)
		state := &pluginNativeState{runtime: f.runtime, scope: scope, native: &native}
		if _, err := state.sshTransferAccess(profile, false, true); err == nil {
			t.Fatal("an unknown package was authorized")
		}
		info := protocol.PluginInfo{Extension: &protocol.PluginExtension{Actions: []protocol.PluginAction{protocol.PluginActionReadSsh}}}
		info.Summary.Name, info.Summary.Digest = reference.Name, reference.Digest
		info.Summary.SettingsRevision, info.Summary.Enabled, info.Summary.Compatibility.Kind = reference.SettingsRevision, true, "compatible"
		f.runtime.packages = newPackageState()
		f.runtime.packages.infos[reference.Name] = info
		if _, err := state.sshTransferAccess(profile, false, true); err != nil {
			t.Fatal("declared read access was refused", err)
		}
		if _, err := state.sshTransferAccess(profile, true, true); err == nil {
			t.Fatal("upload was authorized without ssh.control")
		}
		stale := profile
		stale.Revision++
		if _, err := state.sshTransferAccess(stale, false, true); err == nil {
			t.Fatal("a stale profile started a transfer")
		}
		if _, err := state.sshTransferAccess(stale, false, false); err != nil {
			t.Fatal("a queued job could not be cancelled after its profile changed", err)
		}
	})
}
