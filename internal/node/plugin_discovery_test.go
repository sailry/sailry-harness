package node

import (
	"database/sql"
	"slices"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestUnassignedReminderToolKeepsDiscoveryLabels(t *testing.T) {
	node := nodeFixture(t)
	session := sessionFixture(t, node)
	invocation := &store.AgentInvocation{Session: session, Turn: protocol.DefaultQueuedTurn(), Run: protocol.DefaultConversationRun()}
	invocation.Turn.Config = session.Config
	invocation.Run.Turn, invocation.Run.Session, invocation.Run.Worktree = protocol.NewTurnID(), session.ID, session.Worktree
	err := node.profile.Store.Node.Read(t.Context(), func(db *sql.Conn) error {
		worktrees, err := store.ReadWorktrees(t.Context(), db)
		if err != nil {
			return err
		}
		for _, worktree := range worktrees {
			if worktree.ID == session.Worktree {
				invocation.Worktree = worktree
				break
			}
		}
		infos, err := store.ReadPluginInfos(t.Context(), db)
		if err != nil {
			return err
		}
		for _, info := range infos {
			if info.Summary.Name == "reminders" {
				invocation.Turn.Plugins = []protocol.PluginReference{plugins.PackageReference(info)}
				break
			}
		}
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
	if invocation.Worktree.ID != session.Worktree || invocation.Worktree.Project != nil || len(invocation.Turn.Plugins) != 1 {
		t.Fatal("fixture did not capture the bundled package in an unassigned conversation")
	}
	tools, err := node.pluginTools(t.Context(), invocation)
	if err != nil {
		t.Fatal(err)
	}
	if len(tools) != 1 || tools[0].Definition.Name != plugins.ToolAlias("reminders", "reminders") || !slices.Contains(tools[0].SearchLabels, "待办提醒") {
		t.Fatalf("captured reminder discovery = %+v", tools)
	}
	// A label cannot make a tool available outside its declared contexts.
	invocation.Turn.Config.Resource = &protocol.ConnectionResource{Kind: "ssh", Data: protocol.NewSshID()}
	if tools, err = node.pluginTools(t.Context(), invocation); err != nil || len(tools) != 0 {
		t.Fatalf("discovery bypassed resource admission: %+v, %v", tools, err)
	}
}
