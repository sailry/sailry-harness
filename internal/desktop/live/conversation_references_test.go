package live

import (
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
	marketdesktop "github.com/sailry/sailry-harness/modules/market/desktop"
)

func TestPluginReferenceOpensOwningHostDetails(t *testing.T) {
	localSnapshot := protocol.DefaultSnapshot()
	localSnapshot.Node = protocol.NodeID{1}
	root := pluginViewFixture(t, &pluginViewTestNode{snapshot: localSnapshot})
	root.App = shell.NewLiveWithAssembly(root.App.Data, shell.Assemble(marketdesktop.Register))

	info := protocol.DefaultPluginInfo()
	info.Summary.Name, info.Summary.Revision = "host-only", 1
	hostSnapshot := protocol.DefaultSnapshot()
	hostSnapshot.Node = protocol.NodeID{2}
	hostSnapshot.Plugins = []protocol.PluginSummary{info.Summary}
	session := protocol.SessionID{1}
	hostSnapshot.Sessions = []protocol.Session{{ID: session}}
	host := pluginViewFixture(t, &pluginViewTestNode{snapshot: hostSnapshot})
	host.parent, host.App = root, root.App
	host.packages.infos[info.Summary.Name] = info
	host.packages.observing = true
	hostID := hostSnapshot.Node.String()
	root.bindings = map[string]*Runtime{hostID: host}
	root.App.Host = localSnapshot.Node.String()
	root.App.Data.Unassigned = append(root.App.Data.Unassigned, &viewmodel.Conversation{ID: session.String(), Node: hostID})

	loaded := false
	root.App.Settings.PackageActions = &settings.PackageActions{LoadDetails: func(kind settings.DetailKind, id string, done func(string, string)) {
		rows := root.App.Settings.Packages
		if root.packageOwner() != host || kind != settings.DetailPlugin || id != info.Summary.Name ||
			len(rows) != 1 || rows[0].ID != id {
			t.Fatalf("details loaded before projecting the owning host: %v, %v", kind, rows)
		}
		loaded = true
		done("", "")
	}}
	host.openReference(session.String(), protocol.TurnID{}, protocol.ConversationReference{
		Target: protocol.ConversationReferenceTarget{Kind: "plugin", Data: info.Summary.Name},
	})
	pluginViewRunQueue(root)

	if !loaded || root.App.Page != shell.SettingsPage || root.App.Settings.Section != settings.Plugins ||
		root.App.Settings.ManagedNode != hostID || root.App.Settings.Details == nil || root.App.Host != localSnapshot.Node.String() {
		t.Fatalf("plugin reference did not open the owner without changing the monitor: %v, %v", root.App.Settings.Details, root.App.Host)
	}
}

func TestPluginConversationReferenceNeedsMarket(t *testing.T) {
	snapshot := protocol.DefaultSnapshot()
	snapshot.Node = protocol.NodeID{1}
	session := protocol.SessionID{1}
	snapshot.Sessions = []protocol.Session{{ID: session}}
	r := pluginViewFixture(t, &pluginViewTestNode{snapshot: snapshot})
	r.App.Data.Unassigned = append(r.App.Data.Unassigned, &viewmodel.Conversation{ID: session.String(), Node: snapshot.Node.String()})
	r.openReference(session.String(), protocol.TurnID{}, protocol.ConversationReference{
		Target: protocol.ConversationReferenceTarget{Kind: "plugin", Data: "fixture"},
	})
	tester := ui.NewTester(func(c *ui.Context) {
		r.mu.Lock()
		pending := r.queue
		r.queue = nil
		r.mu.Unlock()
		for _, run := range pending {
			run(c)
		}
	}, 320, 240)
	tester.Frame()
	if r.App.Page == shell.SettingsPage || r.App.Settings.Details != nil || len(tester.OpenedURLs()) != 0 {
		t.Fatal("an unregistered plugin reference opened a destination")
	}
}
