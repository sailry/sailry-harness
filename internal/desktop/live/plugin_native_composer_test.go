package live

import (
	"encoding/json"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestDelegatedPluginComposerUsesOpenChild(t *testing.T) {
	parent, child, worktree := protocol.SessionID{1}, protocol.SessionID{2}, protocol.WorktreeID{3}
	info := protocol.DefaultPluginInfo()
	info.Summary.Name, info.Summary.Digest, info.Summary.Enabled = "composer-fixture", strings.Repeat("a", 64), true
	info.Extension = new(protocol.DefaultPluginExtension())
	info.Extension.Actions = []protocol.PluginAction{protocol.PluginActionReadConversation, protocol.PluginActionReadWorktrees, protocol.PluginActionProvideModels}
	snapshot := protocol.DefaultSnapshot()
	snapshot.Node, snapshot.Plugins = protocol.NodeID{1}, []protocol.PluginSummary{info.Summary}
	snapshot.Worktrees = []protocol.Worktree{{ID: worktree, Main: true}}
	session := protocol.DefaultSession()
	session.ID, session.Worktree = child, worktree
	origin := &protocol.Delegation{Session: parent}
	session.Delegation = &origin
	session.Config.Model = "child-model"
	snapshot.Sessions = []protocol.Session{session}
	root := pluginViewFixture(t, &pluginViewTestNode{snapshot: snapshot})
	root.packages.infos[info.Summary.Name] = info
	root.teams = newTeamState()
	root.teams.childParents[child.String()] = parent
	scope := protocol.DefaultPluginContext()
	scope.Package, scope.Surface.Kind, scope.Session, scope.Worktree = packageReference(info.Summary), "composer", &child, &worktree
	if pluginViewScopeExists(root, scope) {
		t.Fatal("a delegated composer mount was allowed before opening its tab")
	}
	root.App.Sidebar.Selected = parent.String()
	root.App.MountPanel()
	root.App.Panel.ShowChild(child.String(), "Child")
	if !pluginViewScopeExists(root, scope) || root.App.Data.Find(child.String()) != nil {
		t.Fatal("the open delegated composer must still be separate from the conversation list")
	}
	native := root.pluginNativeBinding(root.ctx, scope, "")
	read := func(name string, value any) {
		t.Helper()
		body, err := native.Operations[name](root.ctx, nil)
		if err != nil || json.Unmarshal(body, value) != nil {
			t.Fatalf("%s could not read an open delegated composer: %s, %v", name, body, err)
		}
	}
	var composer struct {
		Session  protocol.SessionID `json:"session"`
		Model    string             `json:"model"`
		Readonly bool               `json:"readonly"`
	}
	read("sailry/ui.readComposer", &composer)
	if composer.Session != child || composer.Model != "child-model" || !composer.Readonly {
		t.Fatalf("unexpected child composer projection: %+v", composer)
	}
	var modelContext struct {
		Connected bool `json:"connected"`
	}
	read("sailry/ui.readModelContext", &modelContext)
	if !modelContext.Connected {
		t.Fatal("the delegated composer lost its model connection")
	}
	var location struct {
		Session     protocol.SessionID  `json:"session"`
		Worktree    protocol.WorktreeID `json:"worktree"`
		CanRetarget bool                `json:"can_retarget"`
		CanMove     bool                `json:"can_move"`
	}
	read("sailry/ui.readLocation", &location)
	if location.Session != child || location.Worktree != worktree || location.CanRetarget || location.CanMove {
		t.Fatalf("unexpected child location: %+v", location)
	}
	root.App.Sidebar.Selected = "draft/first"
	root.App.MountPanel()
	if !pluginViewScopeExists(root, scope) {
		t.Fatal("switching conversations invalidated an open child tab")
	}
	root.App.Sidebar.Selected = parent.String()
	root.App.MountPanel()
	root.App.Panel.Close(panel.Tab{Kind: panel.Subagent, ID: child.String()})
	if pluginViewScopeExists(root, scope) {
		t.Fatal("closing the child tab did not invalidate its composer mount")
	}
	for _, name := range []string{"sailry/ui.readComposer", "sailry/ui.readModelContext", "sailry/ui.readLocation"} {
		if _, err := native.Operations[name](root.ctx, nil); faultCode(err) != protocol.ErrorCodeUnavailable {
			t.Fatalf("%s read a closed delegated composer: %v", name, err)
		}
	}
}
