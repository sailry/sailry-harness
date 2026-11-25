//go:build integration

package live

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"slices"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/desktop/preferences"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/testassembly"
	"github.com/sailry/sailry-harness/internal/desktop/testplugin"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node"
	"github.com/sailry/sailry-harness/internal/node/moduleapi"
	"github.com/sailry/sailry-harness/internal/protocol"
	filesnode "github.com/sailry/sailry-harness/modules/files/node"
	"github.com/sailry/sailry-harness/sdk/desktop/native"
)

func TestPluginConversationLinkIntegration(t *testing.T) {
	ctx, cancel := context.WithTimeout(t.Context(), 45*time.Second)
	defer cancel()
	const name, scheme = "link-fixture", "sailry-plugin-link-fixture"
	const zeroName, legacyName = "link-zero-fixture", "link-legacy-fixture"
	const pluginPriority = 10
	const url = scheme + ":/notes/today"
	const entry = "dev.sailry.platform/ui.js"
	const document, documentText = "README.md", "Scoped fixture\nOnly the local worktree owns this file.\n"
	const nativePanel = "fixture-navigation"
	const script = `
import {View, div} from 'gpui-kit';
import {context, publishContributions, nextContributionEvent} from 'sailry/sdk';
import {invokeAppAction} from 'sailry/ui';
const hasFilePermission = %t;
export default class LinkFixture extends View {
  init(_props, cx) {
    const scope = context();
    publishContributions([{id:'open', visible:true, enabled:true}]);
    cx.spawn(async () => {
      let calls = 0;
      while (true) {
        const event = await nextContributionEvent();
        if (Object.keys(event).sort().join(',') !== 'handler,id,kind,sequence,value')
          throw new Error('unexpected link event payload');
        if (event.kind === 'link') {
          await invokeAppAction('fixture.open-panel', {route:event.value});
          if (hasFilePermission) await invokeAppAction('workspace.open-file', {path:'README.md',line:2});
          for (const test of [
            ['fixture.private', {}, 'permission_denied'],
            hasFilePermission
              ? ['workspace.open-file', {path:'README.md',node:scope.execution_node}, 'invalid_request']
              : ['workspace.open-file', {path:'README.md'}, 'permission_denied'],
            ['fixture.open-panel', {route:event.value,node:scope.execution_node}, 'invalid_request']
          ]) {
            let denied = false;
            try { await invokeAppAction(test[0], test[1]); }
            catch (error) { denied = String(error).includes('"code":"'+test[2]+'"'); }
            if (!denied) throw new Error('navigation was not denied: '+test[0]);
          }
        }
        const label = [++calls,event.kind,event.handler,event.value].join('|');
        const value = JSON.stringify({Node:scope.execution_node,Worktree:scope.worktree,Session:scope.session});
        publishContributions([{id:'open', visible:true, enabled:true, label:{label}, value, reply_to:event.sequence}]);
      }
    });
  }
  render() { return div(); }
}`
	const platform = `{"api_version":"v1","desktop":{"ui_entry":"` + entry + `","resources":["` + entry + `"]},
		"ui":[{"id":"open","slot":"composer","kind":"button","label":{"label":%q},"handler":"openNote"%s}]%s}`
	fixture := func(packageName, label, linkFields string, filePermission bool) testplugin.Package {
		actions := ""
		if filePermission {
			actions = `,"actions":["files.read"]`
		}
		return testplugin.Package{Name: packageName, Platform: fmt.Sprintf(platform, label, linkFields, actions), Files: map[string]string{entry: fmt.Sprintf(script, filePermission)}}
	}
	do := func(controller *client.Client, kind string, input any) protocol.Output {
		t.Helper()
		output, err := controller.Do(ctx, protocol.Command{Kind: kind, Data: input})
		if err != nil {
			t.Fatalf("%s: %v", kind, err)
		}
		return output
	}
	start := func() (*node.Node, *client.Client) {
		t.Helper()
		n, err := node.StartWithOptions(ctx, filepath.Join(t.TempDir(), "profile"), node.Options{
			Assembly: moduleapi.Assembly{Modules: []moduleapi.Module{filesnode.Module()}},
			Network:  &link.NetworkScope{Bind: "127.0.0.1:0"},
		})
		if err != nil {
			t.Fatal(err)
		}
		t.Cleanup(func() { _ = n.Stop(context.Background()) })
		controller := client.New(link.NewLocal(n.ID(), n.ID(), n))
		if err := controller.Start(ctx); err != nil {
			t.Fatal(err)
		}
		t.Cleanup(func() { _ = controller.Close() })
		return n, controller
	}
	install := func(controller *client.Client, pkg testplugin.Package) protocol.PluginInfo {
		t.Helper()
		info, err := testplugin.Install(ctx, controller, pkg)
		if err != nil {
			t.Fatal(err)
		}
		return info
	}
	createSession := func(controller *client.Client, project *protocol.ProjectID) protocol.Session {
		t.Helper()
		provider, model := fixtureProvider("Conversation link fixture")
		secret := protocol.NewSecret("fixture-key")
		do(controller, "save_provider", protocol.CommandSaveProviderPayload{Provider: provider, Secret: &secret})
		config := protocol.DefaultSessionConfig()
		config.Provider, config.Model, config.Effort = provider.ID, model.ID, model.DefaultEffort
		config.Mode, config.Permission = protocol.WorkModeCode, protocol.PermissionAsk
		session, ok := detail[protocol.Session](do(controller, "create_session", protocol.CommandCreateSessionPayload{Project: project, Config: &config}).Data)
		if !ok {
			t.Fatal("missing Node session")
		}
		return session
	}
	localNode, local := start()
	installed := install(local, fixture(name, "Link fixture", fmt.Sprintf(`,"link_scheme":%q,"priority":%d`, scheme, pluginPriority), true))
	zero := install(local, fixture(zeroName, "Zero fixture", fmt.Sprintf(`,"link_scheme":%q,"priority":0`, protocol.PluginLinkScheme(zeroName)), false))
	install(local, fixture(legacyName, "Legacy fixture", "", false))
	worktree := t.TempDir()
	if err := os.WriteFile(filepath.Join(worktree, document), []byte(documentText), 0o600); err != nil {
		t.Fatal(err)
	}
	project, ok := detail[protocol.Project](do(local, "register_project", protocol.CommandRegisterProjectPayload{Name: "Plugin link project", Path: worktree}).Data)
	if !ok {
		t.Fatal("missing Node project")
	}
	localSession := createSession(local, &project.ID)
	remoteNode, _ := start()
	offer, err := remoteNode.Link().InviteFor(ctx, link.InvitationScope{Kind: protocol.InvitationMyDevice})
	if err != nil {
		t.Fatal(err)
	}
	defer offer.Close()
	address, err := localNode.Link().Pair(ctx, offer.Ticket())
	if err != nil {
		t.Fatal(err)
	}
	if err := offer.Paired(ctx); err != nil {
		t.Fatal(err)
	}
	remote := client.New(localNode.Link().Remote(address))
	if err := remote.Start(ctx); err != nil {
		t.Fatal(err)
	}
	defer remote.Close()
	remoteSession := createSession(remote, nil)
	assembly := shell.Assemble(testassembly.RegisterFilePanelFixture, func(assembly *shell.Assembly) {
		assembly.RegisterPanelResource(panel.Resource{Kind: nativePanel, Label: "details", Icon: icons.Folder, Order: 90,
			Render: func(c *ui.Context, _ *panel.State, _ panel.Tab) {
				ui.Text(c.Key("navigation-fixture-panel"), "Fixture navigation panel")
			},
		})
	})
	root, err := NewWithAssembly(ctx, local, preferences.Open(filepath.Join(t.TempDir(), "preferences.json")), assembly)
	if err != nil {
		t.Fatal(err)
	}
	defer root.Close()
	root.bindings = map[string]*Runtime{remote.Target().String(): root.newBinding(remote)}
	root.changed()
	root.OpenSession(localSession.ID.String())
	wantScope := shell.AppActionScope{Node: local.Target(), Worktree: localSession.Worktree, Session: localSession.ID}
	var navigation []shell.AppActionContext
	wantPanel := panel.Tab{Kind: nativePanel, ID: "notes/today"}
	if _, err := root.App.RegisterAppAction(shell.AppAction{ID: "fixture.open-panel", PublicNavigation: true,
		Run: func(_ *ui.Context, app *shell.App, source shell.AppActionContext, body json.RawMessage) error {
			var input struct {
				Route string `json:"route"`
			}
			if err := protocol.DecodeObject(body, &input, []string{"route"}, true); err != nil || input.Route != "notes/today" {
				return &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "invalid test navigation input"}
			}
			if !source.Current() || source.AppActionScope != wantScope {
				t.Fatalf("plugin navigation was not bound to the source: %+v", source.AppActionScope)
			}
			navigation = append(navigation, source)
			app.RevealConversationPanel()
			app.Panel.Show(wantPanel)
			return nil
		},
	}); err != nil {
		t.Fatal(err)
	}
	if _, err := root.App.RegisterAppAction(shell.AppAction{ID: "fixture.private",
		Run: func(*ui.Context, *shell.App, shell.AppActionContext, json.RawMessage) error {
			t.Error("the plugin invoked a private action")
			return nil
		},
	}); err != nil {
		t.Fatal(err)
	}

	route := native.LinkRoute{Kind: native.LinkScheme, Name: scheme}
	var calls, defaultCalls []native.LinkTarget
	nativeDenial := &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "native fixture denied the link"}
	nativeHandler := func(_ *ui.Context, target native.LinkTarget, _ native.LinkActions) error {
		calls = append(calls, target)
		return nativeDenial
	}
	defaultHandler := func(_ *ui.Context, target native.LinkTarget, _ native.LinkActions) error {
		defaultCalls = append(defaultCalls, target)
		return nil
	}
	releaseNative, err := root.App.RegisterLinkHandlerWithPriority(route, pluginPriority, nativeHandler)
	if err != nil {
		t.Fatal(err)
	}
	tester := ui.NewTester(root.App.View, 1440, 900)
	tester.SetLocale("en")
	wait := func(label string, ready func() bool) {
		t.Helper()
		deadline := time.Now().Add(5 * time.Second)
		for {
			tester.Advance(16 * time.Millisecond)
			tester.Frame()
			if ready() {
				return
			}
			for _, mounted := range root.pluginViews.mounts {
				if mounted.failure != nil {
					t.Fatalf("%s: %v", label, mounted.failure)
				}
			}
			if ctx.Err() != nil || time.Now().After(deadline) {
				for key, mounted := range root.pluginViews.mounts {
					t.Logf("mount %v: link %+v", key, mounted.link)
				}
				t.Logf("selected %s; local %s; remote %s, paired snapshot sessions %d, projected %t", root.App.Sidebar.Selected,
					localSession.ID, remoteSession.ID, len(remote.Snapshot().Sessions), root.App.Data.FindOn(remote.Target().String(), remoteSession.ID.String()) != nil)
				t.Fatalf("%s did not settle; visible text: %q", label, tester.Texts())
			}
			time.Sleep(10 * time.Millisecond)
		}
	}
	open := func(session protocol.SessionID, url string) {
		t.Helper()
		root.enqueue(func(c *ui.Context) {
			if !root.App.Messages.OpenLink(c, session.String(), url) {
				t.Fatalf("link %q was not consumed", url)
			}
		})
		tester.Frame()
	}
	button := func(label string) bool {
		control, found := tester.AccessByRole(ui.RoleButton, label)
		return found && !control.Disabled
	}
	localMount := func(packageName string) pluginMountKey {
		return pluginMountKey{role: "contribution", name: packageName, scope: pluginScopeKey{
			node: local.Target(), surface: "composer", worktree: localSession.Worktree.String(), session: localSession.ID.String(),
		}}
	}
	localMountKey := localMount(name)
	acknowledgment := func(packageName, label string) (json.RawMessage, bool) {
		mounted := root.pluginViews.mounts[localMount(packageName)]
		if mounted != nil && mounted.registry != nil {
			for _, entry := range mounted.registry.Entries("composer") {
				if entry.Key.Package.Name == packageName && entry.Key.ID == "open" && entry.State.Label != nil &&
					entry.State.Label.Label == label && len(entry.State.Value) != 0 {
					return entry.State.Value, true
				}
			}
		}
		return nil, false
	}
	localTarget := shell.LinkTarget{Route: route, Node: local.Target(), Session: localSession.ID.String(), Worktree: localSession.Worktree, URL: url}
	targetFor := func(packageName string) shell.LinkTarget {
		target := localTarget
		target.Route.Name = protocol.PluginLinkScheme(packageName)
		target.URL = target.Route.Name + ":/notes/today"
		return target
	}
	expectDefault := func(target shell.LinkTarget, count int) {
		t.Helper()
		handled, err := root.App.DispatchLink(nil, target, shell.LinkActions{})
		if !handled || err != nil || len(defaultCalls) != count || defaultCalls[count-1].URL != target.URL {
			t.Fatalf("default did not recover %s: %t, %v, calls %+v", target.Route.Name, handled, err, defaultCalls)
		}
	}

	wait("equal priority conflicts without disabling the plugin button", func() bool {
		mounted := root.pluginViews.mounts[localMountKey]
		return mounted != nil && mounted.link != nil && mounted.link.rejected && button("Link fixture")
	})
	wait("v1 package without a link declaration still mounts its ordinary button", func() bool { return button("Legacy fixture") })
	if handled, err := root.App.DispatchLink(nil, targetFor(legacyName), shell.LinkActions{}); handled || err != nil {
		t.Fatalf("a mounted button without link_scheme registered a route: %t, %v", handled, err)
	}
	wait("paired conversation projection", func() bool { return root.App.Data.FindOn(remote.Target().String(), remoteSession.ID.String()) != nil })
	open(localSession.ID, url)
	if len(calls) != 1 || calls[0].Route != route || calls[0].URL != localTarget.URL || calls[0].Node != localTarget.Node ||
		calls[0].Session != localTarget.Session || calls[0].Worktree != localTarget.Worktree || !button("Link fixture") {
		t.Fatalf("native handler lost the captured target or invoked the plugin: %+v", calls)
	}
	releaseNative()
	if handled, err := root.App.DispatchLink(nil, localTarget, shell.LinkActions{}); handled || err != nil {
		t.Fatalf("the conflicted plugin took over after the native handler closed: %t, %v", handled, err)
	}
	setEnabled := func(enabled bool) {
		t.Helper()
		output := do(local, "set_plugin_enabled", protocol.CommandSetPluginEnabledPayload{Name: name, ExpectedRevision: installed.Summary.Revision, Enabled: enabled})
		var valid bool
		installed, valid = detail[protocol.PluginInfo](output.Data)
		if !valid || installed.Summary.Enabled != enabled {
			t.Fatalf("unexpected plugin enable result: %+v", output)
		}
	}
	setEnabled(false)
	wait("disabled package", func() bool { return root.pluginViews.mounts[localMountKey] == nil })
	setEnabled(true)
	var claim *shell.LinkContribution
	wait("visible, unconflicted plugin route", func() bool {
		mounted := root.pluginViews.mounts[localMountKey]
		if mounted == nil || mounted.link == nil || mounted.link.registration == nil || !button("Link fixture") {
			return false
		}
		claim = mounted.link.registration
		return claim.Active()
	})
	if release, err := root.App.RegisterLinkHandlerWithPriority(route, pluginPriority, nativeHandler); release != nil ||
		faultCode(err) != protocol.ErrorCodeRevisionConflict || !claim.Active() || len(calls) != 1 || len(defaultCalls) != 0 {
		t.Fatalf("dynamic native conflict displaced the mounted plugin or invoked a handler: release %t, error %v", release != nil, err)
	}
	foreign := localTarget
	foreign.Node = remote.Target()
	if handled, err := root.App.DispatchLink(nil, foreign, shell.LinkActions{}); !handled || faultCode(err) != protocol.ErrorCodePermissionDenied {
		t.Fatalf("a different Node accessed the same session and worktree IDs: %t, %v", handled, err)
	}
	open(remoteSession.ID, url)
	open(localSession.ID, "javascript:alert(1)")
	open(localSession.ID, "file://other-node/README.md")
	if err := root.App.RegisterDefaultLinkHandler(route, defaultHandler); err != nil {
		t.Fatal(err)
	}
	releaseLower, err := root.App.RegisterLinkHandler(route, nativeHandler)
	if err != nil {
		t.Fatal(err)
	}
	defer releaseLower()
	for _, url := range []string{scheme + ":/../private", scheme + ":/%2e%2e/private", scheme + "://other/notes", scheme + ":/notes?session=other", scheme + ":42"} {
		target := localTarget
		target.URL = url
		if handled, err := root.App.DispatchLink(nil, target, shell.LinkActions{}); !handled || faultCode(err) != protocol.ErrorCodeInvalidRequest || len(calls) != 1 || len(defaultCalls) != 0 {
			t.Fatalf("invalid target %q entered ranking: %t, %v, %d", url, handled, err, len(calls))
		}
	}
	releaseHigher, err := root.App.RegisterLinkHandlerWithPriority(route, pluginPriority+1, nativeHandler)
	if err != nil {
		t.Fatal(err)
	}
	if handled, err := root.App.DispatchLink(nil, localTarget, shell.LinkActions{}); !handled || !errors.Is(err, nativeDenial) || len(calls) != 2 || !claim.Active() {
		t.Fatalf("higher native priority did not win without revoking the plugin: %t, %v, %d", handled, err, len(calls))
	}
	releaseHigher()
	root.enqueue(func(c *ui.Context) {
		if !root.App.Messages.OpenLink(c, localSession.ID.String(), url) {
			t.Fatal("the conversation link was not consumed")
		}
		// A second click in the same frame reaches the selected package's busy
		// control, before its JS callback can publish the reply.
		if handled, err := root.App.DispatchLink(c, localTarget, shell.LinkActions{}); !handled || faultCode(err) != protocol.ErrorCodeNotConfigured || len(calls) != 2 {
			t.Fatalf("the winning package's rejection tried a lower handler: %t, %v, %d", handled, err, len(calls))
		}
	})
	tester.Frame()
	wantLabel := "1|link|openNote|notes/today"
	var scopedReply json.RawMessage
	wait("the real plugin consumes only the accepted link event in its captured scope", func() bool {
		var ok bool
		scopedReply, ok = acknowledgment(name, wantLabel)
		return ok
	})
	wait("workspace.open-file reads and opens the actual file in the captured worktree", func() bool {
		workspace := root.selectedWorkspace()
		return workspace != nil && workspace.owner.ID == localSession.Worktree && workspace.files[document].Text == documentText &&
			root.App.PanelOpen && root.App.Panel.Active == (panel.Tab{Kind: panel.Files, ID: document})
	})
	if len(navigation) != 1 || len(calls) != 2 || len(defaultCalls) != 0 || len(tester.OpenedURLs()) != 0 {
		t.Fatalf("the link lost its single scoped navigation: %d, %+v, %v", len(navigation), calls, tester.OpenedURLs())
	}
	var scriptScope shell.AppActionScope
	var value string
	if err := json.Unmarshal(scopedReply, &value); err != nil {
		t.Fatal(err)
	}
	if err := json.Unmarshal([]byte(value), &scriptScope); err != nil {
		t.Fatal(err)
	}
	if scriptScope != wantScope {
		t.Fatalf("script context = %+v, want %+v", scriptScope, wantScope)
	}

	root.OpenSession(remoteSession.ID.String())
	wait("switching to the paired source releases the previous route", func() bool {
		return !claim.Active() && root.App.Sidebar.Selected == remoteSession.ID.String() && !button("Legacy fixture")
	})
	if navigation[0].Current() {
		t.Fatal("deferred navigation from the hidden plugin source remained current")
	}
	if handled, err := root.App.DispatchLink(nil, localTarget, shell.LinkActions{}); !handled || !errors.Is(err, nativeDenial) || len(calls) != 3 {
		t.Fatalf("hiding the source did not restore the lower-priority route: %t, %v, %d", handled, err, len(calls))
	}
	root.OpenSession(localSession.ID.String())
	wait("the visible source can claim the route again", func() bool {
		mounted := root.pluginViews.mounts[localMountKey]
		if mounted == nil || mounted.link == nil || mounted.link.registration == nil {
			return false
		}
		claim = mounted.link.registration
		return claim.Active()
	})
	if navigation[0].Current() {
		t.Fatal("reopening a view revived the retired app-action source")
	}
	do(local, "remove_plugin", protocol.CommandRemovePluginPayload{Name: name, ExpectedRevision: installed.Summary.Revision})
	wait("uninstalled source releases its registration", func() bool { return !claim.Active() && root.pluginViews.mounts[localMountKey] == nil })
	if handled, err := root.App.DispatchLink(nil, localTarget, shell.LinkActions{}); !handled || !errors.Is(err, nativeDenial) || len(calls) != 4 {
		t.Fatalf("uninstalling the package did not restore the lower-priority route: %t, %v, %d", handled, err, len(calls))
	}
	releaseLower()
	expectDefault(localTarget, 1)

	zeroTarget := targetFor(zeroName)
	if err := root.App.RegisterDefaultLinkHandler(zeroTarget.Route, defaultHandler); err != nil {
		t.Fatal(err)
	}
	wait("the explicit zero-priority plugin is mounted", func() bool {
		mounted := root.pluginViews.mounts[localMount(zeroName)]
		return mounted != nil && mounted.link != nil && mounted.link.registration != nil && mounted.link.registration.Active()
	})
	if release, err := root.App.RegisterLinkHandler(zeroTarget.Route, nativeHandler); release != nil ||
		faultCode(err) != protocol.ErrorCodeRevisionConflict || len(calls) != 4 || len(defaultCalls) != 1 {
		t.Fatalf("priority-zero live conflict invoked a handler or returned the wrong error: release %t, error %v", release != nil, err)
	}
	open(localSession.ID, zeroTarget.URL)
	wait("declared zero wins over the host default and opens the registered panel", func() bool {
		_, replied := acknowledgment(zeroName, wantLabel)
		return replied && root.App.PanelOpen && root.App.Panel.Active == wantPanel && tester.HasText("Fixture navigation panel")
	})
	if len(navigation) != 2 || len(defaultCalls) != 1 {
		t.Fatalf("declared zero did not reach the public action: navigation %d, default %d", len(navigation), len(defaultCalls))
	}
	do(local, "remove_plugin", protocol.CommandRemovePluginPayload{Name: zeroName, ExpectedRevision: zero.Summary.Revision})
	wait("the zero-priority plugin unloads", func() bool { return root.pluginViews.mounts[localMount(zeroName)] == nil })
	expectDefault(zeroTarget, 2)

	const web = "https://example.test/link"
	open(localSession.ID, web)
	if !slices.Equal(tester.OpenedURLs(), []string{web}) {
		t.Fatalf("host default did not open the ordinary web link: %v", tester.OpenedURLs())
	}
}
