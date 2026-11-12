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
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	desktopplugins "github.com/sailry/sailry-harness/internal/desktop/plugins"
	"github.com/sailry/sailry-harness/internal/desktop/preferences"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
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
			// Only the bundled worktrees package's location and the bundled
			// Git package's branch are the composer's own controls again.
			entry := registry.Entries("context")[0]
			if pluginContextControl(entry, pluginWorktreesPackage, pluginLocationControl) || pluginContextControl(entry, pluginGitPackage, pluginBranchControl) {
				t.Fatal("another package's context control was taken for a bundled one")
			}
			entry.Key.Package.Name = pluginWorktreesPackage
			if !pluginContextControl(entry, pluginWorktreesPackage, pluginLocationControl) || pluginContextControl(entry, pluginGitPackage, pluginBranchControl) {
				t.Fatal("the worktrees package's location was not recognized")
			}
			// An indicator without a measure has no tooltip, whatever
			// placeholder its package sends; a measured or loading one keeps it.
			percent := 12.0
			for _, c := range []struct {
				indicator protocol.PluginUiIndicator
				want      string
			}{
				{protocol.PluginUiIndicator{Hint: "0"}, ""},
				{protocol.PluginUiIndicator{Hint: "compacting", Loading: true}, "compacting"},
				{protocol.PluginUiIndicator{Hint: "12%", Percent: &percent}, "12%"},
			} {
				if got := indicatorHint(c.indicator); got != c.want {
					t.Fatalf("indicator %+v has the tooltip %q, want %q", c.indicator, got, c.want)
				}
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
	// A package's manifest contributes to the slots around the composer and
	// the header: the statistics slot's figures and the footer slot's, a
	// provider's account status, are the context bar's with their detail
	// rows; the composer's indicator is a toolbar ring; the header's popover
	// is a header button the window opens, which its package never hears of.
	// Removed UI: a failed view draws nothing in its place. Its failure is one
	// toast, whose Retry mounts the view again.
	t.Run("a failed view draws no retry button of its own", func(t *testing.T) {
		info := protocol.DefaultPluginInfo()
		info.Summary.Name, info.Summary.Digest, info.Summary.Enabled = "failed-fixture", strings.Repeat("e", 64), true
		info.Summary.Compatibility.Kind = "compatible"
		snapshot := protocol.DefaultSnapshot()
		snapshot.Node, snapshot.Plugins = protocol.NodeID{1}, []protocol.PluginSummary{info.Summary}
		source := &pluginViewTestNode{snapshot: snapshot, bundle: make(chan link.Response, 1)}
		root := pluginViewFixture(t, source)
		scope := protocol.DefaultPluginContext()
		scope.Package, scope.Surface.Kind = packageReference(info.Summary), "workspace"
		root.pluginViews.navigation["fixture"] = pluginNavigationEntry{owner: root, info: info, scope: scope}
		mounted := root.ensurePluginView(nil, root, info, scope, "navigation", nil)
		if mounted == nil {
			t.Fatal("the fixture's view is not mounted")
		}
		source.bundle <- link.Response{Output: protocol.Output{Kind: "plugin_view", Data: protocol.PluginDesktopBundle{
			Package: scope.Package, Entry: "dev.sailry.platform/desktop/main.js",
			Files: map[string]string{"dev.sailry.platform/desktop/main.js": `import {View} from "gpui-kit";export default class extends View{render(){throw new Error("fixture render failed");}}`},
		}}}
		// The read is the fixture's only worker: its completion is queued.
		root.workers.Wait()
		key, drawn := mounted.key, true
		tester := ui.NewTester(func(view *ui.Context) {
			theme.Apply(view, "", theme.Light)
			root.mu.Lock()
			queued := root.queue
			root.queue = nil
			root.mu.Unlock()
			for _, apply := range queued {
				apply(view)
			}
			ui.Column(view).Fill().Children(func() { drawn = root.renderMountedPlugin(view, root.pluginViews.mounts[key]) })
			kit.ToastHost(view)
		}, 960, 600)
		tester.SetLocale("en")
		for range 3 {
			tester.Frame()
		}
		retry := i18n.In("en", "chat_retry")
		var buttons []ui.AccessInfo
		for _, node := range tester.AccessAll() {
			if node.Name == retry {
				buttons = append(buttons, node)
			}
		}
		if drawn || mounted.failure == nil || root.pluginViews.mounts[key] != mounted || !tester.HasText(i18n.In("en", "plugin_callback_failed")) {
			t.Fatalf("the view did not fail with its toast: drawn %t, %v, %q", drawn, mounted.failure, tester.Texts())
		}
		// The toast's action is the one Retry: no icon button in the page.
		if len(buttons) != 1 || !tester.HasText(retry) {
			t.Fatalf("a failed view shows %d Retry controls %v: %q", len(buttons), buttons, tester.Texts())
		}
		tester.ClickAt(buttons[0].Bounds.X+buttons[0].Bounds.W/2, buttons[0].Bounds.Y+buttons[0].Bounds.H/2)
		for range 2 {
			tester.Frame()
		}
		if again := root.pluginViews.mounts[key]; again == nil || again == mounted || again.failure != nil || !again.loading {
			t.Fatalf("the toast's Retry did not mount the view again: %+v", again)
		}
	})
	t.Run("declared slots draw their figures, ring and header popover", func(t *testing.T) {
		info := protocol.DefaultPluginInfo()
		info.Summary.Name, info.Summary.Digest, info.Summary.Enabled = "slots-fixture", strings.Repeat("d", 64), true
		info.Extension = new(protocol.DefaultPluginExtension())
		if err := json.Unmarshal([]byte(`[
			{"id":"tokens","slot":"statistics","kind":"metric","label":{"label":"Tokens"},"icon":"chart-pie","order":0},
			{"id":"turns","slot":"statistics","kind":"metric","label":{"label":"Turns"},"order":10},
			{"id":"hidden","slot":"statistics","kind":"metric","label":{"label":"Hidden"},"order":20},
			{"id":"quota","slot":"footer","kind":"metric","label":{"label":"Plan usage"},"icon":"gauge","width":320},
			{"id":"idle","slot":"footer","kind":"metric","label":{"label":"Other provider"}},
			{"id":"panel","slot":"header","kind":"popover","label":{"label":"Context"},"icon":"chart-pie","handler":"context","width":380},
			{"id":"ring","slot":"composer","kind":"indicator","label":{"label":"Context"},"align":"end"}]`), &info.Extension.Ui); err != nil {
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
			t.Fatal("the contribution mount was not captured")
		}
		if err := mounted.hooks.Publish(json.RawMessage(`[
			{"id":"tokens","value":"48.2k","details":[{"label":{"label":"This session"},"value":"41.0k"},{"label":{"label":"Subagents"},"value":"7.2k"}]},
			{"id":"turns","value":"17","details":[{"label":{"label":"Responses"},"value":"35"}]},
			{"id":"hidden","value":"0","visible":false},
			{"id":"quota","value":"64%","details":[{"label":{"label":"Five hours"},"value":"64% left","progress":64},{"label":{"label":"Resets"},"value":"18:00"}]},
			{"id":"idle","value":"—","enabled":false},
			{"id":"panel"},
			{"id":"ring","value":{"percent":24,"tone":"muted","hint":"11.6k / 48k"}}]`)); err != nil {
			t.Fatal(err)
		}
		if err := registry.SetActivity(mounted.scope.Package, true, false); err != nil {
			t.Fatal(err)
		}
		for _, slot := range []protocol.PluginUiSlot{"statistics", "footer", "header", "composer"} {
			if len(registry.Entries(slot)) == 0 {
				t.Fatalf("the %s slot registered nothing", slot)
			}
		}
		rings := root.pluginIndicators(registry)
		if len(rings) != 1 || rings[0].Percent != 24 || rings[0].Hint != "11.6k / 48k" || rings[0].Start || rings[0].Panel != nil {
			t.Fatalf("the composer's indicator is not one measured ring at the end: %+v", rings)
		}

		var comp composer.State
		root.projectPluginMetrics(&comp, registry)
		compact, open := false, false
		var footer, statistics [][]conversation.Metric
		tester := ui.NewTester(func(view *ui.Context) {
			theme.Apply(view, "", theme.Light)
			footer = root.pluginMetrics(view, registry, protocol.PluginUiSlotFooter)
			statistics = root.pluginMetrics(view, registry, protocol.PluginUiSlotStatistics)
			ui.Column(view).Fill().Children(func() {
				ui.Row(view.Key("header")).FillWidth().Justify(ui.End).Children(func() {
					root.renderPluginControls(view, registry, protocol.PluginUiSlotHeader, composer.ControlStart)
				})
				ui.Spacer(view)
				ui.Row(view.Key("bar")).FillWidth().Justify(ui.End).Children(func() {
					// Neither slot draws controls of its own any more.
					root.renderPluginControls(view, registry, protocol.PluginUiSlotStatistics, composer.ControlStart)
					root.renderPluginControls(view, registry, protocol.PluginUiSlotFooter, composer.ControlStart)
					comp.Metrics(view, compact, &open)
				})
			})
		}, 960, 600)
		tester.SetLocale("en")
		tester.Frame()
		tester.Frame()

		// The footer's figure shows while its package enables it; a figure
		// published hidden is left out.
		if len(footer) != 1 || len(footer[0]) != 1 || len(statistics) != 1 || len(statistics[0]) != 2 {
			t.Fatalf("the figures are grouped as %+v and %+v", footer, statistics)
		}
		quota, tokens := footer[0][0], statistics[0][0]
		if quota.Label != "Plan usage" || quota.Value != "64%" || quota.Icon == nil || quota.Width != 320 || len(quota.Rows) != 2 ||
			quota.Rows[0] != (conversation.Row{Label: "Five hours", Value: "64% left", Meter: true, Progress: 64}) ||
			quota.Rows[1] != (conversation.Row{Label: "Resets", Value: "18:00"}) {
			t.Errorf("the footer's figure is %+v", quota)
		}
		if tokens.Label != "Tokens" || tokens.Value != "48.2k" || tokens.Icon == nil || tokens.Panel != nil || len(tokens.Rows) != 2 ||
			tokens.Rows[0] != (conversation.Row{Label: "This session", Value: "41.0k"}) ||
			tokens.Rows[1] != (conversation.Row{Label: "Subagents", Value: "7.2k"}) {
			t.Errorf("the statistics slot's first figure is %+v", tokens)
		}
		for _, value := range []string{"64%", "48.2k", "17"} {
			count := 0
			for _, text := range tester.Texts() {
				if text == value {
					count++
				}
			}
			if count != 1 {
				t.Errorf("the bar shows the figure %s %d times: %q", value, count, tester.Texts())
			}
		}
		if tester.HasText("This session") || tester.HasText("Five hours") || tester.HasText("—") {
			t.Errorf("the bar shows more than its figures: %q", tester.Texts())
		}
		hover := func(value string, rows ...string) {
			t.Helper()
			at, ok := tester.Find(value)
			if !ok {
				t.Fatalf("no figure %s: %q", value, tester.Texts())
			}
			tester.Move(at.X+at.W/2, at.Y+at.H/2)
			tester.Frame()
			tester.Advance(time.Second)
			tester.Frame()
			for _, row := range rows {
				if !tester.HasText(row) {
					t.Errorf("the card of %s lacks %q: %q", value, row, tester.Texts())
				}
			}
			tester.Move(0, 0)
			tester.Frame()
			tester.Advance(time.Second)
			tester.Frame()
		}
		hover("48.2k", "Tokens", "This session", "41.0k", "Subagents", "7.2k")
		hover("17", "Turns", "Responses", "35")
		hover("64%", "Plan usage", "Five hours", "64% left", "Resets", "18:00")

		// Below the compact width the statistics are one button whose
		// popover lists them; the provider's figure stays beside it.
		compact = true
		tester.Frame()
		button, ok := tester.AccessByRole(ui.RoleButton, i18n.In("en", "composer_statistics"))
		if !ok || !tester.HasText("64%") || tester.HasText("17") {
			t.Fatalf("the compact bar: button %v, texts %q", ok, tester.Texts())
		}
		tester.ClickAt(button.Bounds.X+button.Bounds.W/2, button.Bounds.Y+button.Bounds.H/2)
		tester.Frame()
		tester.Advance(time.Second)
		tester.Frame()
		for _, row := range []string{"Tokens", "This session", "Subagents", "Turns", "17", "Responses", "35"} {
			if !tester.HasText(row) {
				t.Errorf("the compact popover lacks %q: %q", row, tester.Texts())
			}
		}
		if !open || tester.HasText("Five hours") {
			t.Errorf("the compact popover: open %v, texts %q", open, tester.Texts())
		}
		tester.Key(0, ui.KeyEscape)
		tester.Frame()
		compact = false
		tester.Frame()

		// The header's popover: a header button, opened and closed by the
		// window. The package is asked nothing, so no interaction waits for
		// its answer and the button stays enabled.
		panel := func() ui.AccessInfo {
			t.Helper()
			info, ok := tester.AccessByRole(ui.RoleButton, "Context")
			if !ok {
				t.Fatalf("the header has no Context button: %q", tester.Texts())
			}
			return info
		}
		closed := panel()
		if closed.Disabled || closed.Expanded || closed.Bounds.W != shell.HeaderButton || closed.Bounds.H != shell.HeaderButton {
			t.Fatalf("the header's popover button is %+v", closed)
		}
		press := func() {
			at := panel().Bounds
			tester.ClickAt(at.X+at.W/2, at.Y+at.H/2)
			tester.Frame()
			tester.Advance(time.Second)
			tester.Frame()
		}
		press()
		key, exists := registry.Key(mounted.scope.Package, "panel")
		if !exists {
			t.Fatal("the header's popover is not registered")
		}
		state := root.pluginControls[pluginControlIdentity(key)]
		if opened := panel(); state == nil || !state.open || !opened.Expanded || opened.Disabled {
			t.Fatalf("a press did not open the header's popover: %+v, %+v", state, opened)
		}
		press()
		if again := panel(); state.open || again.Expanded || again.Disabled {
			t.Fatalf("a second press did not close the header's popover: %+v", again)
		}
	})
}

// A failed metadata read or view start is reported in the shape the Node
// records: a failure of its own phase, coded, naming the package only with
// a version, and placed where the view was.
func testPluginDiagnostic(t *testing.T) {
	version, session, worktree := "1.2.0", protocol.NewSessionID(), protocol.NewWorktreeID()
	scope := protocol.PluginContext{Session: &session, Worktree: &worktree}
	started := pluginDiagnostic(protocol.NotificationSystemSystemSourcePluginInitialization, "", "notes", &version, "view_initialize", scope)
	if started.Phase != protocol.NotificationSystemDiagnosticPhaseInitialization || started.Reason != protocol.NotificationSystemDiagnosticReasonFailure ||
		started.Code != protocol.ErrorCodeInternal || started.Plugin == nil || *started.Plugin != (protocol.NotificationSystemDiagnosticPlugin{Name: "notes", Version: version}) ||
		started.Session == nil || *started.Session != session || started.Worktree == nil || *started.Worktree != worktree ||
		started.Operation == nil || *started.Operation != "view_initialize" || started.Version == "" || started.Platform == "" || started.Incident == (protocol.RequestID{}) {
		t.Fatalf("initialization report = %#v", started)
	}
	read := pluginDiagnostic(protocol.NotificationSystemSystemSourcePluginMetadata, protocol.ErrorCodeNotFound, "notes", nil, "metadata_read", protocol.PluginContext{})
	if read.Phase != protocol.NotificationSystemDiagnosticPhaseMetadata || read.Code != protocol.ErrorCodeNotFound || read.Plugin != nil || read.Session != nil || read.Incident == started.Incident {
		t.Fatalf("metadata report = %#v", read)
	}
	// The command is one the protocol decodes strictly.
	encoded, err := json.Marshal(protocol.Command{Kind: "report_diagnostic", Data: started})
	if err != nil {
		t.Fatal(err)
	}
	var decoded protocol.Command
	if err := json.Unmarshal(encoded, &decoded); err != nil {
		t.Fatalf("the Node would refuse the report: %v", err)
	}
}
