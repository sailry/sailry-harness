package live

import (
	"context"
	"errors"
	"io"
	"os"
	"path/filepath"
	"testing"
	"testing/synctest"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/fileview"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	view "github.com/sailry/sailry-harness/internal/desktop/viewapi"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

const servicesTestPage shell.Page = "fixture/window-services"

func newWindowServicesFixture(t *testing.T, source *terminalTabsTestNode, bind func(shell.WindowContext) shell.WindowBinding) (*Runtime, view.Services, context.Context) {
	t.Helper()
	if source.respond == nil {
		source.respond = func(protocol.Request) link.Response {
			return link.Response{Fault: &protocol.Fault{Code: protocol.ErrorCodeUnavailable}}
		}
	}
	return bindWindowServicesFixture(t, source.snapshot.Node, source, bind)
}

func bindWindowServicesFixture(t *testing.T, node protocol.NodeID, handler link.Handler, bind func(shell.WindowContext) shell.WindowBinding) (*Runtime, view.Services, context.Context) {
	t.Helper()
	controller := client.New(link.NewLocal(node, node, handler))
	t.Cleanup(func() { _ = controller.Close() })
	if err := controller.Start(t.Context()); err != nil {
		t.Fatal(err)
	}
	var services view.Services
	var window context.Context
	r, err := New(t.Context(), controller, t.TempDir(), shell.NativePage{
		Page: servicesTestPage, Feature: shell.Feature{ID: string(servicesTestPage)},
		BindWindow: func(binding shell.WindowContext) shell.WindowBinding {
			window = binding.Context
			var ok bool
			services, ok = view.WindowServices(window)
			if !ok || services.Node != node {
				t.Fatalf("native page did not bind its local Node: %v, %v", services.Node, ok)
			}
			if bind != nil {
				return bind(binding)
			}
			return shell.WindowBinding{Render: func(*ui.Context) {}}
		},
	})
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = r.Close() })
	if !services.Connected() {
		t.Fatal("the native binding is not connected to its window")
	}
	if count := windowServiceMountTrackers(r); count != 0 {
		t.Fatalf("binding eagerly installed %d resource mount trackers", count)
	}
	return r, services, window
}

func windowServiceMountTrackers(r *Runtime) int {
	// Mounted views track visibility after rendering; the file link hook runs before it.
	count := 0
	for _, resource := range r.nativeWindowResources {
		if resource.afterFrame != nil {
			count++
		}
	}
	return count
}

func TestWindowServicesRequestsAndClose(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		initial := protocol.DefaultSnapshot()
		initial.Node = protocol.NodeID{7}
		source := &terminalTabsTestNode{pluginViewTestNode: pluginViewTestNode{snapshot: initial, updates: make(chan protocol.Update)}}
		result := protocol.Output{Kind: "ssh_profiles", Data: []protocol.SshProfile{}}
		source.respond = func(request protocol.Request) link.Response {
			switch request.Command.Kind {
			case "remove_ssh":
				return link.Response{Output: result}
			case "inspect_request":
				query, _ := detail[protocol.CommandInspectRequestPayload](request.Command.Data)
				outcome := protocol.RequestOutcome{Kind: "completed", Data: protocol.Result[protocol.Output, protocol.Fault]{Ok: &result}}
				return link.Response{Output: protocol.Output{Kind: "request_outcome", Data: protocol.OutputRequestOutcomePayload{ID: query.ID, Outcome: outcome}}}
			}
			return link.Response{Fault: &protocol.Fault{Code: protocol.ErrorCodeUnavailable}}
		}
		closed := false
		r, services, window := newWindowServicesFixture(t, source, func(shell.WindowContext) shell.WindowBinding {
			return shell.WindowBinding{Close: func() { closed = true }}
		})
		ctx := t.Context()
		command := protocol.Command{Kind: "remove_ssh", Data: protocol.CommandRemoveSshPayload{Profile: protocol.SshID{1}, ExpectedRevision: 2}}
		request := services.Prepare(command)
		if request.Target != initial.Node || request.Plugin != nil || request.ID == (protocol.RequestID{}) {
			t.Fatal("request did not capture the authenticated local target and its own ID")
		}
		foreign := request
		foreign.Target = protocol.NodeID{9}
		if _, err := services.Execute(ctx, foreign); faultCode(err) != protocol.ErrorCodeWrongTarget {
			t.Fatalf("foreign request: %v", err)
		}
		foreign = request
		foreign.Plugin = &protocol.PluginContext{}
		if _, err := services.Settle(ctx, foreign); faultCode(err) != protocol.ErrorCodeInvalidRequest {
			t.Fatalf("spoofed package request: %v", err)
		}
		if got, err := services.Execute(ctx, request); err != nil || got.Kind != result.Kind {
			t.Fatalf("local execution: %v, %v", got.Kind, err)
		}
		if got, err := services.Settle(ctx, services.Prepare(command)); err != nil || got.Kind != result.Kind {
			t.Fatalf("local settlement: %v, %v", got.Kind, err)
		}
		if got, err := services.Resolve(ctx, request); err != nil || got.State != "applied" || got.Output.Kind != result.Kind {
			t.Fatalf("receipt: %+v, %v", got, err)
		}
		if got, err := services.Outcome(ctx, request); err != nil || got.Kind != "completed" {
			t.Fatalf("outcome: %v, %v", got.Kind, err)
		}
		queries := source.sent("inspect_request")
		if len(queries) != 2 || len(source.sent(command.Kind)) != 2 {
			t.Fatal("receipt inspection dispatched an additional or invalid operation")
		}
		for _, command := range queries {
			query, ok := detail[protocol.CommandInspectRequestPayload](command.Data)
			if !ok || query.ID != request.ID || query.Digest == "" {
				t.Fatal("receipt lookup did not use the captured request")
			}
		}
		initialChange, err := services.Next(ctx, "")
		if err != nil || initialChange.Cursor == "" || !initialChange.Connected {
			t.Fatalf("initial observation: %+v, %v", initialChange, err)
		}
		changes := make(chan view.Change, 1)
		observe := func(seen string) <-chan error {
			done := make(chan error, 1)
			go func() {
				change, err := services.Next(ctx, seen)
				if err == nil {
					changes <- change
				}
				done <- err
			}()
			synctest.Wait()
			return done
		}
		done := observe(initialChange.Cursor)
		updated := initial
		updated.Cursor++
		updated.Ssh = []protocol.SshProfile{{ID: protocol.SshID{1}}}
		source.updates <- protocol.Update{Kind: "snapshot", Data: updated}
		if err := <-done; err != nil {
			t.Fatal(err)
		}
		current := <-changes
		if current.Cursor == initialChange.Cursor || !current.Connected {
			t.Fatalf("Node change was not observed: %+v", current)
		}

		work := make(chan struct{})
		if !services.Async(func() { close(work) }) {
			t.Fatal("open window rejected work")
		}
		<-work
		applied := false
		if !services.Post(func(*ui.Context) { applied = true }) {
			t.Fatal("open window rejected UI delivery")
		}
		done = observe(current.Cursor)
		if err := r.Close(); err != nil {
			t.Fatal(err)
		}
		if err := <-done; !errors.Is(err, context.Canceled) {
			t.Fatalf("window close did not cancel observation: %v", err)
		}
		if _, ok := view.WindowServices(window); ok || !closed || applied || services.Connected() || services.Post(func(*ui.Context) {}) || services.Async(func() {}) {
			t.Fatal("closed window still owns a binding, delivery or worker")
		}
	})
}

func TestWindowServicesFileInteractions(t *testing.T) {
	dir := t.TempDir()
	key := filepath.Join(dir, "key.txt")
	if err := os.WriteFile(key, []byte("fixture"), 0o600); err != nil {
		t.Fatal(err)
	}
	snapshot := protocol.DefaultSnapshot()
	snapshot.Node = protocol.NodeID{7}
	r, services, _ := newWindowServicesFixture(t, &terminalTabsTestNode{pluginViewTestNode: pluginViewTestNode{snapshot: snapshot}}, nil)
	r.SetNativeFileActions(NativeFileActions{
		PickFiles: func(reply func([]string)) { reply([]string{key}) },
		SaveFile:  func(name string, reply func(string)) { reply(filepath.Join(dir, name)) },
	})
	r.SetPickDirectory(func(func(string)) {}) // The platform returned without a selection.
	tester := ui.NewTester(r.App.View, 960, 700)
	ctx := t.Context()
	if err := services.WriteClipboard("selected"); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	if tester.Clipboard() != "selected" {
		t.Fatal("clipboard write did not reach the window")
	}
	folder := runWindowInteraction(t, r, tester, func() (string, error) { return services.PickFolder(ctx) })
	if folder != "" {
		t.Fatal("dismissed picker reported a path")
	}
	content := runWindowInteraction(t, r, tester, func() ([]byte, error) {
		data, name, err := services.PickSecret(ctx, "key")
		if err == nil && name != filepath.Base(key) {
			return data, errors.New("credential picker exposed more than the file name")
		}
		return data, err
	})
	defer clear(content)
	if string(content) != "fixture" {
		t.Fatal("credential content did not match the selected file")
	}
	if _, err := services.PickDestination(ctx, "../unselected"); faultCode(err) != protocol.ErrorCodeInvalidRequest {
		t.Fatalf("unconfined save name: %v", err)
	}
	if _, err := services.OpenSource(ctx, "relative", 1024); faultCode(err) != protocol.ErrorCodeInvalidRequest {
		t.Fatalf("relative source: %v", err)
	}
	if _, err := services.OpenSource(ctx, key, 2); faultCode(err) != protocol.ErrorCodeInvalidRequest {
		t.Fatalf("oversized source: %v", err)
	}
	source, err := services.OpenSource(ctx, key, 1024)
	if err != nil {
		t.Fatal(err)
	}
	data, err := io.ReadAll(source.Reader)
	_ = source.Reader.Close()
	if err != nil || string(data) != "fixture" || source.Size != 7 || source.Revision == "" {
		t.Fatalf("invalid source descriptor: %d, %v", source.Size, err)
	}
	path := runWindowInteraction(t, r, tester, func() (string, error) { return services.PickDestination(ctx, "chosen.txt") })
	target, err := services.OpenDestination(path)
	if err != nil {
		t.Fatal(err)
	}
	defer target.Close()
	if _, err := target.Writer.Write([]byte("staged")); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(path, []byte("concurrent edit"), 0o600); err != nil {
		t.Fatal(err)
	}
	if err := target.Publish(ctx); faultCode(err) != protocol.ErrorCodeRevisionConflict {
		t.Fatalf("publication overwrote a concurrent edit: %v", err)
	}
	if saved, err := os.ReadFile(path); err != nil || string(saved) != "concurrent edit" {
		t.Fatal("publication changed the chosen destination after a conflict")
	}
	previewLimit := make(chan uint64, 1)
	if err := services.Preview("remote.md", func(_ context.Context, limit uint64, _ func(uint64, uint64)) ([]byte, error) {
		previewLimit <- limit
		return []byte("preview"), nil
	}); err != nil {
		t.Fatal(err)
	}
	tester.Frame()
	if limit := awaitWindowValue(t, previewLimit); limit != fileview.Limit {
		t.Fatalf("wrong preview byte limit: %d", limit)
	}
}

func runWindowInteraction[Value any](t *testing.T, r *Runtime, tester *ui.Tester, apply func() (Value, error)) Value {
	t.Helper()
	wake := make(chan struct{}, 1)
	r.SetInvalidate(func() {
		select {
		case wake <- struct{}{}:
		default:
		}
	})
	defer r.SetInvalidate(nil)
	var value Value
	var err error
	done := make(chan struct{})
	go func() { value, err = apply(); close(done) }()
	timeout := time.NewTimer(5 * time.Second)
	defer timeout.Stop()
	for {
		select {
		case <-wake:
			tester.Frame()
		case <-done:
			if err != nil {
				t.Fatal(err)
			}
			return value
		case <-timeout.C:
			t.Fatal("native interaction did not complete")
		}
	}
}

func awaitWindowValue[Value any](t *testing.T, values <-chan Value) Value {
	t.Helper()
	select {
	case value := <-values:
		return value
	case <-time.After(5 * time.Second):
		t.Fatal("window callback did not complete")
		var value Value
		return value
	}
}

func TestWindowServicesConversationMountLifecycle(t *testing.T) {
	snapshot := protocol.DefaultSnapshot()
	snapshot.Node = protocol.NodeID{7}
	profile := protocol.DatabaseID{3}
	snapshot.Databases = []protocol.DatabaseProfile{{ID: profile}}
	r, services, _ := newWindowServicesFixture(t, &terminalTabsTestNode{pluginViewTestNode: pluginViewTestNode{snapshot: snapshot}}, nil)
	spec := fixtureConversationSpec(profile)
	if unavailable := services.MountConversation(fixtureConversationSpec(protocol.DatabaseID{9})); unavailable.Render != nil || unavailable.Close != nil {
		t.Fatal("a resource absent on the local Node mounted a conversation")
	}
	var mount view.Conversation
	tester := ui.NewTester(func(c *ui.Context) {
		theme.Apply(c, "", theme.Light)
		launch, ok := services.TerminalLaunch(c).(protocol.SshTerminalLaunch)
		if !ok || launch.Viewport != terminalViewport || launch.Appearance != pluginNativeAppearance(theme.Of(c)) {
			t.Fatal("terminal launch did not receive the window's viewport and palette")
		}
		if mount.Header != nil {
			mount.Header(c, "Conversation")
		}
	}, 480, 320)
	for range 3 {
		mount = services.MountConversation(spec)
		if len(r.pluginViews.drafts) != 0 {
			t.Fatal("mount eagerly created a draft")
		}
		tester.Frame()
		if len(r.pluginViews.drafts) != 1 {
			t.Fatal("mounted conversation did not create one draft")
		}
		var draftID string
		for id := range r.pluginViews.drafts {
			draftID = id
			base := r.composerBases[r.App.Composer(id)]
			if !spec.Match(view.Session{Config: base}) || base.Assistant != nil || base.Permission != protocol.PermissionAsk {
				t.Fatal("draft did not retain its native scope and ask permission")
			}
		}
		mount.Close()
		mount.Close()
		tester.Frame()
		if len(r.pluginViews.drafts) != 0 || windowServiceMountTrackers(r) != 1 || r.App.DropComposer(draftID) != nil {
			t.Fatal("closing or redrawing a retired mount retained window resources")
		}
	}
	mount = services.MountConversation(spec)
	tester.Frame()
	if err := r.Close(); err != nil {
		t.Fatal(err)
	}
	if len(r.nativeWindowResources) != 0 || len(r.pluginViews.drafts) != 0 {
		t.Fatal("window close retained its remaining mounted conversation")
	}
}
