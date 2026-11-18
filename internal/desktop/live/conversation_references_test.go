package live

import (
	"testing"

	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestPluginReferenceOpensOwningHostDetails(t *testing.T) {
	localSnapshot := protocol.DefaultSnapshot()
	localSnapshot.Node = protocol.NodeID{1}
	root := pluginViewFixture(t, &pluginViewTestNode{snapshot: localSnapshot})

	info := protocol.DefaultPluginInfo()
	info.Summary.Name, info.Summary.Revision = "host-only", 1
	hostSnapshot := protocol.DefaultSnapshot()
	hostSnapshot.Node = protocol.NodeID{2}
	hostSnapshot.Plugins = []protocol.PluginSummary{info.Summary}
	host := pluginViewFixture(t, &pluginViewTestNode{snapshot: hostSnapshot})
	host.parent, host.App = root, root.App
	host.packages.infos[info.Summary.Name] = info
	host.packages.observing = true
	hostID := hostSnapshot.Node.String()
	root.bindings = map[string]*Runtime{hostID: host}
	root.App.Host = localSnapshot.Node.String()

	loaded := false
	root.App.Settings.PackageActions = &settings.PackageActions{LoadDetails: func(kind settings.DetailKind, id string, done func()) {
		rows := root.App.Settings.Packages
		if root.packageOwner() != host || kind != settings.DetailPlugin || id != info.Summary.Name ||
			len(rows) != 1 || rows[0].ID != id {
			t.Fatalf("details loaded before projecting the owning host: %v, %v", kind, rows)
		}
		loaded = true
		done()
	}}
	host.openReference("", protocol.TurnID{}, protocol.ConversationReference{
		Target: protocol.ConversationReferenceTarget{Kind: "plugin", Data: info.Summary.Name},
	})
	pluginViewRunQueue(root)

	if !loaded || root.App.Page != shell.SettingsPage || root.App.Settings.Section != settings.Plugins ||
		root.App.Settings.ManagedNode != hostID || root.App.Settings.Details == nil || root.App.Host != localSnapshot.Node.String() {
		t.Fatalf("plugin reference did not open the owner without changing the monitor: %v, %v", root.App.Settings.Details, root.App.Host)
	}
}
