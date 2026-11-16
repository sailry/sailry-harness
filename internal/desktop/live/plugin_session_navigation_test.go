package live

import (
	"strings"
	"testing"

	desktopplugins "github.com/sailry/sailry-harness/internal/desktop/plugins"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestPluginSessionNavigationRetainsExecutionNode(t *testing.T) {
	local, remote := protocol.DefaultSnapshot(), protocol.DefaultSnapshot()
	local.Node, remote.Node = protocol.NodeID{1}, protocol.NodeID{2}
	localSession, remoteSession := protocol.DefaultSession(), protocol.DefaultSession()
	localSession.ID, remoteSession.ID = protocol.NewSessionID(), protocol.NewSessionID()
	local.Sessions, remote.Sessions = []protocol.Session{localSession}, []protocol.Session{remoteSession}
	root := pluginViewFixture(t, &pluginViewTestNode{snapshot: local})
	bound := root.newBinding(ownershipClient(t, remote))
	t.Cleanup(func() { _ = bound.Close() })
	root.bindings = map[string]*Runtime{remote.Node.String(): bound}
	info := protocol.DefaultPluginInfo()
	info.Summary.Name, info.Summary.Digest, info.Summary.Enabled = "navigation-fixture", strings.Repeat("a", 64), true
	root.packages.infos[info.Summary.Name] = info
	scope := protocol.DefaultPluginContext()
	scope.Package = packageReference(info.Summary)
	native := desktopplugins.NewNativeBinding(t.Context())
	state := pluginNativeState{runtime: root, scope: scope, native: &native}

	state.openSession(remoteSession.ID, &remote.Node)
	if root.App.Sidebar.Selected != remoteSession.ID.String() {
		t.Fatal("scheduled session did not open on its captured execution Node")
	}
	unknown := protocol.NodeID{3}
	state.openSession(localSession.ID, &unknown)
	state.openSession(localSession.ID, &remote.Node)
	if root.App.Sidebar.Selected != remoteSession.ID.String() {
		t.Fatal("unavailable or mismatched host selected another host's session")
	}
	state.openSession(localSession.ID)
	if root.App.Sidebar.Selected != localSession.ID.String() {
		t.Fatal("omitted execution Node did not retain the view's existing host")
	}
}
