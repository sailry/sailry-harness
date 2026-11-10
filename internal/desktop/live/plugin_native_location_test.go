package live

import (
	"encoding/json"
	"strings"
	"testing"
	"testing/synctest"

	desktopplugins "github.com/sailry/sailry-harness/internal/desktop/plugins"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func testPluginLocationOwnership(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		project := protocol.DefaultProject()
		project.ID, project.Name, project.Path = protocol.ProjectID{1}, "Captured project", "/fixture"
		tree := protocol.Worktree{ID: protocol.WorktreeID{1}, Project: &project.ID, Path: "/fixture", Main: true}
		snapshot := protocol.DefaultSnapshot()
		snapshot.Node, snapshot.Projects, snapshot.Worktrees = protocol.NodeID{1}, []protocol.Project{project}, []protocol.Worktree{tree}
		root := pluginViewFixture(t, &pluginViewTestNode{snapshot: snapshot})
		info := protocol.DefaultPluginInfo()
		info.Summary.Name, info.Summary.Digest, info.Summary.Enabled = "location-fixture", strings.Repeat("a", 64), true
		info.Extension = new(protocol.DefaultPluginExtension())
		info.Extension.Actions = []protocol.PluginAction{protocol.PluginActionReadWorktrees, protocol.PluginActionControlConversation}
		root.packages.infos[info.Summary.Name] = info
		scope := protocol.DefaultPluginContext()
		scope.Package, scope.Surface.Kind = packageReference(info.Summary), "composer"
		native := desktopplugins.NewNativeBinding(root.ctx)
		native.Operations = map[string]desktopplugins.HostOperation{}
		state := &pluginNativeState{
			runtime: root, scope: scope, presentation: "draft/first", native: &native,
			composer: &pluginNativeComposer{source: "draft/first"},
		}
		state.locationOperations()
		root.App.Sidebar.Selected = "draft/second"
		body, err := native.Operations["sailry/ui.readLocation"](root.ctx, nil)
		var initial struct {
			Cursor      string `json:"cursor"`
			Node        string `json:"node"`
			CanRetarget bool   `json:"can_retarget"`
		}
		if err != nil || json.Unmarshal(body, &initial) != nil || initial.Cursor != "1" ||
			initial.Node != root.controller.Target().String() || !initial.CanRetarget {
			t.Fatalf("captured draft location: %s, %v", body, err)
		}
		changes := make(chan json.RawMessage, 1)
		go func() {
			cursor, _ := json.Marshal(initial.Cursor)
			value, err := native.Operations["sailry/ui.nextLocationChange"](root.ctx, []json.RawMessage{cursor})
			if err == nil {
				changes <- value
			}
		}()
		synctest.Wait()
		selected := root.App.Composer("draft/first")
		selected.Project, selected.Worktree = project.ID.String(), tree.Path
		native.Frame(nil)
		synctest.Wait()
		select {
		case body := <-changes:
			var changed struct {
				Cursor   string               `json:"cursor"`
				Worktree *protocol.WorktreeID `json:"worktree"`
			}
			if json.Unmarshal(body, &changed) != nil || changed.Cursor != "2" || changed.Worktree == nil || *changed.Worktree != tree.ID {
				t.Fatalf("location observation lost the captured source's change: %s", body)
			}
		default:
			t.Fatal("location observation followed selection instead of its captured draft")
		}
		if _, err := native.Operations["sailry/ui.readLocation"](root.ctx, nil); faultCode(err) != protocol.ErrorCodePermissionDenied {
			t.Fatalf("former scope read a retargeted draft location: %v", err)
		}
		info.Extension.Actions = nil
		root.packages.infos[info.Summary.Name] = info
		restricted := desktopplugins.NewNativeBinding(root.ctx)
		restricted.Operations = map[string]desktopplugins.HostOperation{}
		denied := &pluginNativeState{runtime: root, scope: scope, native: &restricted}
		denied.locationOperations()
		if _, err := restricted.Operations["sailry/ui.readLocation"](root.ctx, nil); faultCode(err) != protocol.ErrorCodePermissionDenied {
			t.Fatalf("location read did not require its declared worktree action: %v", err)
		}
	})
	t.Run("prepared worktree handoffs", testPluginLocationHandoffs)
	t.Run("confirmed worktree release", testPluginLocationReleases)
	t.Run("captured project picker", testPluginLocationPicker)
	t.Run("session base", testPluginSessionBase)
	t.Run("status badge action", testPluginStatusClick)
}
