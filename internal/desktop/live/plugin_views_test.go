package live

import (
	"context"
	"encoding/json"
	"errors"
	"path/filepath"
	"strings"
	"sync"
	"testing"
	"testing/synctest"

	"github.com/sailry/sailry-harness/internal/client"
	desktopplugins "github.com/sailry/sailry-harness/internal/desktop/plugins"
	"github.com/sailry/sailry-harness/internal/desktop/preferences"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type pluginViewTestNode struct {
	snapshot protocol.Snapshot
	bundle   chan link.Response
}

type pluginViewTestSubscription struct {
	initial *protocol.Snapshot
	closed  chan struct{}
	once    sync.Once
}

func (source *pluginViewTestNode) Open(context.Context, protocol.NodeID, protocol.StreamID) (link.Stream, error) {
	return nil, errors.New("fixture streams are unavailable")
}

func (source *pluginViewTestNode) Dispatch(_ context.Context, _ protocol.NodeID, request protocol.Request) (link.Admission, error) {
	if request.Command.Kind != "read_plugin_view" || source.bundle == nil {
		return link.Admission{}, errors.New("unexpected fixture command")
	}
	return link.Admission{Receipt: protocol.Receipt{ID: request.ID}, Completion: source.bundle}, nil
}

func (source *pluginViewTestNode) Subscribe(_ context.Context, _ protocol.NodeID, topic protocol.Topic) (link.Subscription, error) {
	if topic.Kind != "node" {
		return nil, errors.New("fixture supports only Node observation")
	}
	initial := source.snapshot
	return &pluginViewTestSubscription{initial: &initial, closed: make(chan struct{})}, nil
}

func (subscription *pluginViewTestSubscription) Next(ctx context.Context) (protocol.Update, error) {
	if subscription.initial != nil {
		initial := *subscription.initial
		subscription.initial = nil
		return protocol.Update{Kind: "snapshot", Data: initial}, nil
	}
	select {
	case <-ctx.Done():
		return protocol.Update{}, ctx.Err()
	case <-subscription.closed:
		return protocol.Update{}, context.Canceled
	}
}

func (subscription *pluginViewTestSubscription) Close() error {
	subscription.once.Do(func() { close(subscription.closed) })
	return nil
}

func pluginViewFixture(t *testing.T, source *pluginViewTestNode) *Runtime {
	t.Helper()
	ctx, cancel := context.WithCancel(t.Context())
	controller := client.New(link.NewLocal(source.snapshot.Node, source.snapshot.Node, source))
	if err := controller.Start(ctx); err != nil {
		cancel()
		t.Fatal(err)
	}
	data := &viewmodel.Data{Workspace: &viewmodel.Workspace{}}
	for _, id := range []string{"draft/first", "draft/second"} {
		data.Unassigned = append(data.Unassigned, &viewmodel.Conversation{ID: id, Node: controller.Target().String()})
	}
	root := &Runtime{
		ctx: ctx, cancel: cancel, controller: controller, packages: newPackageState(),
		App: shell.NewLive(data), Preferences: preferences.Open(filepath.Join(t.TempDir(), "preferences.json")),
	}
	for _, draft := range data.Unassigned {
		root.App.Composer(draft.ID).Host = controller.Target().String()
	}
	root.initPluginViews()
	t.Cleanup(func() {
		root.closePluginViews()
		cancel()
		root.workers.Wait()
		root.observers.Wait()
		_ = controller.Close()
	})
	return root
}

func pluginViewRunQueue(root *Runtime) {
	root.mu.Lock()
	queued := root.queue
	root.queue = nil
	root.mu.Unlock()
	for _, apply := range queued {
		apply(nil)
	}
}

func pluginViewRejectedIntent(t *testing.T, root *Runtime, scope protocol.PluginContext, presentation string) {
	t.Helper()
	result := make(chan error, 1)
	go func() {
		value := json.RawMessage("null")
		_, err := root.pluginIntent(root.ctx, scope, presentation, "draft-action", &value)
		result <- err
	}()
	synctest.Wait()
	pluginViewRunQueue(root)
	synctest.Wait()
	select {
	case err := <-result:
		if err == nil || err.Error() != "contribution context changed" {
			t.Fatalf("stale draft intent was not rejected by its captured owner: %v", err)
		}
	default:
		t.Fatal("stale draft intent waited instead of rejecting its invalid captured owner")
	}
}

func testPluginViewOwnership(t *testing.T) {
	t.Run("mixed surface publication retains every declared contribution", func(t *testing.T) {
		synctest.Test(t, func(t *testing.T) {
			info := protocol.DefaultPluginInfo()
			info.Summary.Name, info.Summary.Digest, info.Summary.Enabled = "mixed-fixture", strings.Repeat("c", 64), true
			info.Extension = new(protocol.DefaultPluginExtension())
			if err := json.Unmarshal([]byte(`[{"id":"location","slot":"context","kind":"button","label":{"label":"Location"},"handler":"location"},{"id":"manage","slot":"project_menu","kind":"button","label":{"label":"Manage"},"handler":"manage"}]`), &info.Extension.Ui); err != nil {
				t.Fatal(err)
			}
			snapshot := protocol.DefaultSnapshot()
			snapshot.Node, snapshot.Plugins = protocol.NodeID{1}, []protocol.PluginSummary{info.Summary}
			root := pluginViewFixture(t, &pluginViewTestNode{snapshot: snapshot, bundle: make(chan link.Response)})
			scope := protocol.DefaultPluginContext()
			scope.Surface.Kind = "composer"
			registry := root.pluginRegistry(scope, "draft/first")
			mounted := root.ensurePluginView(nil, root, info, scope, "contribution", registry)
			if mounted == nil {
				t.Fatal("mixed-surface contribution mount was not captured")
			}
			if err := mounted.hooks.Publish(json.RawMessage(`[{"id":"location"},{"id":"manage"}]`)); err != nil {
				t.Fatalf("a declared contribution from another slot broke publication: %v", err)
			}
			if err := registry.SetActivity(mounted.scope.Package, true, false); err != nil {
				t.Fatal(err)
			}
			if len(registry.Entries("context")) != 1 || len(registry.Entries("project_menu")) != 0 {
				t.Fatal("published contributions escaped their presentation surface")
			}
		})
	})
	t.Run("independent drafts retain exact contribution callbacks", func(t *testing.T) {
		synctest.Test(t, func(t *testing.T) {
			snapshot := protocol.DefaultSnapshot()
			snapshot.Node = protocol.NodeID{1}
			root := pluginViewFixture(t, &pluginViewTestNode{snapshot: snapshot})
			scope := protocol.DefaultPluginContext()
			scope.Surface.Kind = "composer"
			reference := protocol.PluginReference{Name: "draft-fixture", Digest: strings.Repeat("a", 64)}
			declaration := protocol.DefaultPluginUiContribution()
			declaration.ID, declaration.Slot, declaration.Kind = "draft-action", "composer", "button"
			declaration.Label.Label, declaration.Handler, declaration.Intent = "Draft action", new("action"), new("draft-action")
			registries := make([]*desktopplugins.Registry, 0, 2)
			keys := make([]desktopplugins.ContributionKey, 0, 2)
			hooks := make([]desktopplugins.ContributionHooks, 0, 2)
			for _, presentation := range []string{"draft/first", "draft/second"} {
				root.App.Sidebar.Selected = "draft/second"
				registry := root.pluginRegistry(scope, presentation)
				binding, err := registry.Hooks(reference, []protocol.PluginUiContribution{declaration})
				if err != nil {
					t.Fatal(err)
				}
				if err := binding.Publish(json.RawMessage(`[{"id":"draft-action"}]`)); err != nil {
					t.Fatal(err)
				}
				if err := registry.SetActivity(reference, true, false); err != nil {
					t.Fatal(err)
				}
				key, exists := registry.Key(reference, declaration.ID)
				if !exists {
					t.Fatal("captured draft contribution was not registered")
				}
				registries, keys, hooks = append(registries, registry), append(keys, key), append(hooks, binding)
			}
			if registries[0] == registries[1] || keys[0].Generation == keys[1].Generation {
				t.Fatal("two unsent drafts shared a contribution owner or callback generation")
			}
			if err := registries[0].InvokeIntent(keys[1], json.RawMessage("null")); err == nil {
				t.Fatal("the second draft's callback reached the first draft's owner")
			}
			for index, registry := range registries {
				if err := registry.InvokeIntent(keys[index], json.RawMessage("null")); err != nil {
					t.Fatal(err)
				}
				body, err := hooks[index].Next(root.ctx)
				var event protocol.PluginUiEvent
				if err != nil || json.Unmarshal(body, &event) != nil || event.ID != declaration.ID || event.Sequence != 1 {
					t.Fatalf("captured draft callback was not delivered once: %s, %v", body, err)
				}
			}
			root.removeDraft("draft/first")
			pluginViewRejectedIntent(t, root, scope, "draft/first")
			if err := hooks[0].Publish(json.RawMessage(`[{"id":"draft-action"}]`)); err == nil {
				t.Fatal("a removed draft accepted a late contribution publication")
			}
			if err := hooks[1].Publish(json.RawMessage(`[{"id":"draft-action"}]`)); err != nil {
				t.Fatalf("removing one draft closed another draft's callback owner: %v", err)
			}
			root.App.Composer("draft/second").Host = protocol.NodeID{2}.String()
			pluginViewRejectedIntent(t, root, scope, "draft/second")
			for _, owner := range root.pluginViews.registries {
				if owner.draft == "draft/second" {
					root.syncPluginRegistry(nil, owner)
				}
			}
			if err := hooks[1].Publish(json.RawMessage(`[{"id":"draft-action"}]`)); err == nil {
				t.Fatal("a rebound draft accepted its former Node's callback publication")
			}
		})
	})
	t.Run("queued immutable bundle completion cannot mount after disconnect", func(t *testing.T) {
		synctest.Test(t, func(t *testing.T) {
			info := protocol.DefaultPluginInfo()
			info.Summary.Name, info.Summary.Digest, info.Summary.Enabled = "deferred-fixture", strings.Repeat("b", 64), true
			snapshot := protocol.DefaultSnapshot()
			snapshot.Node, snapshot.Plugins = protocol.NodeID{1}, []protocol.PluginSummary{info.Summary}
			source := &pluginViewTestNode{snapshot: snapshot, bundle: make(chan link.Response, 1)}
			root := pluginViewFixture(t, source)
			scope := protocol.DefaultPluginContext()
			scope.Package, scope.Surface.Kind = packageReference(info.Summary), "workspace"
			root.pluginViews.navigation["fixture"] = pluginNavigationEntry{owner: root, info: info, scope: scope}
			mounted := root.ensurePluginView(nil, root, info, scope, "navigation", nil)
			if mounted == nil {
				t.Fatal("immutable bundle read did not retain its captured owner")
			}
			synctest.Wait()
			source.bundle <- link.Response{Output: protocol.Output{Kind: "plugin_view", Data: protocol.PluginDesktopBundle{
				Package: scope.Package, Entry: "dev.sailry.platform/desktop/main.js",
				Files: map[string]string{"dev.sailry.platform/desktop/main.js": `import {View,div} from "gpui-kit";export default class extends View{render(){return div().child("Deferred view");}}`},
			}}}
			synctest.Wait()
			root.mu.Lock()
			queued := len(root.queue)
			root.mu.Unlock()
			if queued != 1 {
				t.Fatalf("completed immutable read did not queue its UI mount: %d", queued)
			}
			if err := root.controller.Close(); err != nil {
				t.Fatal(err)
			}
			root.disconnectPluginView(mounted)
			pluginViewRunQueue(root)
			if mounted.host != nil || mounted.runtime != nil || mounted.mounted != nil || mounted.failure != nil || !mounted.disconnected {
				t.Fatal("a late bundle completion created execution or failure state after its owner disconnected")
			}
		})
	})
}
