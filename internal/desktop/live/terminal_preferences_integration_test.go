//go:build integration

package live

import (
	"context"
	"encoding/json"
	"path/filepath"
	"strings"
	"testing"

	myterminal "github.com/egoist/mygo/plugins/terminal"
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/desktop/plugins"
	"github.com/sailry/sailry-harness/internal/desktop/preferences"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
	"github.com/sailry/sailry-harness/sdk/desktop/native"
)

// terminalPreferencesNode serves captured terminal identities without a local
// process or SSH connection. The native views and their font updates are real.
type terminalPreferencesNode struct {
	pluginViewTestNode
}

func (source *terminalPreferencesNode) Dispatch(_ context.Context, _ protocol.NodeID, request protocol.Request) (link.Admission, error) {
	output := protocol.Output{Kind: "terminal_input"}
	if request.Command.Kind == "inspect_terminal" {
		input, _ := detail[protocol.CommandInspectTerminalPayload](request.Command.Data)
		for _, info := range source.snapshot.Terminals {
			if info.ID == input.Terminal {
				output = protocol.Output{Kind: "terminal_snapshot", Data: protocol.TerminalSnapshot{Node: source.snapshot.Node, Info: info}}
				break
			}
		}
	}
	completion := make(chan link.Response, 1)
	completion <- link.Response{Output: output}
	return link.Admission{Receipt: protocol.Receipt{ID: request.ID}, Completion: completion}, nil
}

func (source *terminalPreferencesNode) Subscribe(ctx context.Context, caller protocol.NodeID, topic protocol.Topic) (link.Subscription, error) {
	if topic.Kind != "terminal" {
		return source.pluginViewTestNode.Subscribe(ctx, caller, topic)
	}
	return &pluginViewTestSubscription{closed: make(chan struct{})}, nil
}

func TestTerminalPreferencesReachLocalAndPluginSSHViewsIntegration(t *testing.T) {
	ctx, cancel := context.WithCancel(t.Context())
	worktree, ssh := protocol.WorktreeID{1}, protocol.SshID{2}
	snapshot := protocol.DefaultSnapshot()
	snapshot.Node = protocol.NodeID{1}
	for _, id := range []byte{1, 2, 3, 4} {
		info := terminalTabsTestInfo(id, worktree, "running")
		if id%2 == 0 {
			info.Worktree, info.Ssh = nil, &ssh
		}
		snapshot.Terminals = append(snapshot.Terminals, info)
	}
	source := &terminalPreferencesNode{pluginViewTestNode: pluginViewTestNode{snapshot: snapshot}}
	controller := client.New(link.NewLocal(snapshot.Node, snapshot.Node, source))
	if err := controller.Start(ctx); err != nil {
		cancel()
		t.Fatal(err)
	}
	root := &Runtime{
		ctx: ctx, cancel: cancel, controller: controller, packages: newPackageState(),
		App:         shell.NewLiveWithAssembly(&viewmodel.Data{Workspace: &viewmodel.Workspace{}}, shell.Assemble(native.RegisterTerminal)),
		Preferences: preferences.Open(filepath.Join(t.TempDir(), "preferences.json")),
	}
	root.packages.terminals = map[string]*liveTerminal{}
	root.Preferences.Data.Terminal = preferences.Terminal{FontSize: 18, FontFamily: "monospace", PasteProtection: true}
	reference := protocol.PluginReference{Name: "terminal-font-fixture", Digest: strings.Repeat("a", 64), SettingsRevision: 1}
	info := protocol.PluginInfo{Extension: &protocol.PluginExtension{Actions: []protocol.PluginAction{protocol.PluginActionReadSsh, protocol.PluginActionControlSsh}}}
	info.Summary.Name, info.Summary.Digest, info.Summary.SettingsRevision = reference.Name, reference.Digest, reference.SettingsRevision
	info.Summary.Enabled, info.Summary.Compatibility.Kind = true, "compatible"
	root.packages.infos[reference.Name] = info
	native := plugins.NewNativeBinding(ctx)
	scope := protocol.DefaultPluginContext()
	scope.Package, scope.Surface = reference, protocol.PluginDesktopSurface{Kind: "workspace"}
	state := &pluginNativeState{runtime: root, native: &native, scope: scope, terminals: map[string]*pluginNativeTerminal{}}
	var elements []plugins.NativeElement
	tester := ui.NewTester(func(view *ui.Context) {
		root.mu.Lock()
		queue := root.queue
		root.queue = nil
		root.mu.Unlock()
		for _, apply := range queue {
			apply(view)
		}
		root.terminalsFrame(view)
		ui.Column(view).Fill().Children(func() {
			for _, element := range elements {
				ui.Box(view).Key(element.ID).FillWidth().Grow(1).Children(func() {
					if _, err := state.renderTerminal(view, element, nil, nil); err != nil {
						t.Fatal(err)
					}
				})
			}
		})
	}, 800, 600)
	window := &terminalUI{t: t, ctx: ctx, runtime: root, controller: controller, tester: tester}
	defer func() {
		cancel()
		for _, mounted := range state.terminals {
			mounted.release()
		}
		for id := range root.packages.terminals {
			root.releaseLiveTerminal(id)
		}
		root.workers.Wait()
		_ = controller.Close()
	}()
	window.wait("the captured Node connects", controller.Connected)
	openPair := func(first int) {
		local, remote := snapshot.Terminals[first], snapshot.Terminals[first+1]
		root.openLiveTerminal(root, local)
		root.App.Panel.Show(panel.Tab{Kind: panel.Terminal, ID: local.ID.String()})
		props, err := json.Marshal(map[string]any{"terminal": remote.ID})
		if err != nil {
			t.Fatal(err)
		}
		elements = append(elements, plugins.NativeElement{ID: uint64(first + 1), Props: props})
	}
	fontsMatch := func(size float64, count int) bool {
		var terminals []*myterminal.Terminal
		for _, mounted := range root.packages.terminals {
			terminals = append(terminals, mounted.terminal)
		}
		for _, mounted := range state.terminals {
			terminals = append(terminals, mounted.state.terminal)
		}
		if len(terminals) != count {
			return false
		}
		for _, terminal := range terminals {
			if terminal == nil {
				return false
			}
			family, actual := terminalFontOf(terminal)
			if family != "monospace" || actual != size {
				return false
			}
		}
		return true
	}
	openPair(0)
	window.wait("local and plugin SSH terminals use the initial preference", func() bool { return fontsMatch(18, 2) })
	root.Preferences.Data.Terminal.FontSize = 24
	window.wait("both existing terminals use the changed preference", func() bool { return fontsMatch(24, 2) })
	openPair(2)
	window.wait("new terminals share the changed preference", func() bool { return fontsMatch(24, 4) })
	root.Preferences.Data.Terminal.FontSize = 12
	window.wait("all existing terminal views continue to follow preferences", func() bool { return fontsMatch(12, 4) })
}
