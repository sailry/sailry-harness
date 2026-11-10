package client

import (
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func testPluginChangeCursors(t *testing.T) {
	snapshot := projectionSnapshot(10)
	tree, other := protocol.WorktreeID{1}, protocol.WorktreeID{2}
	session := projectionSession(1, 1)
	run := protocol.DefaultConversationRun()
	run.Session, run.Turn, run.Worktree = session.ID, protocol.TurnID{1}, tree
	run.Kind, run.Status = "task", "running"
	session.Activity.Run = &run
	snapshot.Worktrees = []protocol.Worktree{{ID: tree}, {ID: other}}
	snapshot.Sessions = []protocol.Session{session}
	plugin := protocol.DefaultPluginSummary()
	plugin.Name = "cursor-fixture"
	snapshot.Plugins = []protocol.PluginSummary{plugin}
	projection := NewProjection(snapshot.Node, 1)
	mustApply(t, projection, 1, protocol.Update{Kind: "snapshot", Data: snapshot}, Applied)
	controller := New(&clientTransport{})
	controller.project = projection
	initial := controller.PluginCursors(plugin.Name, nil, &tree)
	if initial != (PluginCursors{Cursor: 10, Package: 1, Settled: 10, TerminalTools: 1}) {
		t.Fatalf("initial plugin cursors: %+v", initial)
	}
	mustApply(t, projection, 1, projectionEvent(11, "defaults_changed", protocol.DefaultDefaults()), Applied)
	if current := controller.PluginCursors(plugin.Name, nil, &tree); current.Settled != initial.Settled || current.Package != initial.Package {
		t.Fatalf("unrelated Node traffic changed scoped cursors: %+v", current)
	}
	mustApply(t, projection, 1, projectionEvent(12, "plugin_values_changed", protocol.EventPluginValuesChangedPayload{Name: plugin.Name}), Applied)
	if current := controller.PluginCursors(plugin.Name, nil, &tree); current.Package != 2 || current.Settled != 10 {
		t.Fatalf("private values did not invalidate their package: %+v", current)
	}
	mustApply(t, projection, 1, projectionEvent(13, "tools_completed", protocol.EventToolsCompletedPayload{Session: session.ID}), Applied)
	if current := controller.PluginCursors(plugin.Name, nil, &tree); current.Settled != 13 {
		t.Fatalf("tool completion did not settle its actual worktree: %+v", current)
	}
	if current := controller.PluginCursors(plugin.Name, nil, &other); current.Settled != 10 {
		t.Fatalf("tool completion invalidated a different worktree: %+v", current)
	}
	completed := protocol.DefaultActivitySummary()
	completedRun := run
	completedRun.Status = "completed"
	completed.Run = &completedRun
	mustApply(t, projection, 1, projectionEvent(14, "conversation_changed", protocol.EventConversationChangedPayload{Session: session.ID, Activity: &completed}), Applied)
	mustApply(t, projection, 1, projectionEvent(15, "conversation_changed", protocol.EventConversationChangedPayload{Session: session.ID, Activity: &completed}), Applied)
	if current := controller.PluginCursors(plugin.Name, nil, &tree); current.Settled != 14 {
		t.Fatalf("unchanged completed run repeatedly invalidated its worktree: %+v", current)
	}
	mustApply(t, projection, 1, projectionEvent(16, "terminal_settings_changed", protocol.EventTerminalSettingsChangedPayload{Revision: 2}), Applied)
	if current := controller.PluginCursors(plugin.Name, []protocol.PluginAction{protocol.PluginActionReadTerminals}, &tree); current.Package != 3 || current.TerminalTools != 3 || current.Settled != 14 {
		t.Fatalf("declared catalog dependency was not invalidated: %+v", current)
	}
	if current := controller.PluginCursors(plugin.Name, nil, &tree); current.Package != 2 {
		t.Fatalf("undeclared catalog dependency invalidated the package: %+v", current)
	}
	mustApply(t, projection, 1, projectionEvent(17, "worktree_removed", protocol.EventWorktreeRemovedPayload{ID: other}), Applied)
	if current := controller.PluginCursors(plugin.Name, []protocol.PluginAction{protocol.PluginActionReadTerminals}, &other); current.Package != 4 || current.TerminalTools != 3 || current.Settled != 0 {
		t.Fatalf("worktree removal changed terminal tools or retained a stale scope: %+v", current)
	}
}
