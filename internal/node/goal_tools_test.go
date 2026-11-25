package node

import (
	"encoding/json"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/moduleapi"
	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestCoreGoalStateCompatibility(t *testing.T) {
	t.Parallel()
	legacy, err := readGoalValue(protocol.PluginStorageConversationEntry{
		Present: true, Value: json.RawMessage(`{"id":"legacy","description":"Finish the checks","state":"in_progress"}`),
	})
	if err != nil || legacy == nil || legacy.Objective != "Finish the checks" || legacy.Status != "paused" ||
		legacy.Reason == nil || *legacy.Reason != "stopped" || legacy.SinceMs != nil {
		t.Fatalf("legacy goal = %+v, %v", legacy, err)
	}
	goal, err := readGoalValue(protocol.PluginStorageConversationEntry{
		Present: true, Value: json.RawMessage(`{"id":"current","objective":"Finish the checks","version":1,"status":"active","extension":{"retained":true}}`),
	})
	if err != nil || goal == nil {
		t.Fatalf("current goal = %+v, %v", goal, err)
	}
	inv := &store.AgentInvocation{Run: protocol.ConversationRun{Continuation: &protocol.ConversationContinuation{
		Package: protocol.PluginReference{Name: "other"}, Kind: new("verify"),
	}}}
	scope := goalToolScope{invocation: inv, plugin: protocol.PluginContext{Package: protocol.PluginReference{Name: "goals", Digest: coreGoalDigests[0], SettingsRevision: 2}}}
	if err := scope.change(goal, "complete", "The first check passed", 10); err != nil || goal.Status != "active" || goal.Claim == nil {
		t.Fatalf("foreign verification marked the goal complete: %+v, %v", goal, err)
	}
	inv.Run.Continuation.Package = protocol.PluginReference{Name: "goals", Digest: coreGoalDigests[1], SettingsRevision: 2}
	if err := scope.change(goal, "complete", "Another release reported success", 15); err != nil || goal.Status != "active" || goal.Claim == nil {
		t.Fatalf("same-name verification marked the goal complete: %+v, %v", goal, err)
	}
	inv.Run.Continuation.Package = scope.plugin.Package
	if err := scope.change(goal, "complete", "Verified independently", 20); err != nil || goal.Status != "completed" || goal.Evidence == nil ||
		*goal.Evidence != "Verified independently" || goal.FinishedMs == nil || *goal.FinishedMs != 20 {
		t.Fatalf("owned verification did not close the goal: %+v, %v", goal, err)
	}
	body, err := json.Marshal(goal)
	if err != nil {
		t.Fatal(err)
	}
	var saved map[string]json.RawMessage
	if err := json.Unmarshal(body, &saved); err != nil || string(saved["extension"]) != `{"retained":true}` || string(saved["status"]) != `"completed"` {
		t.Fatalf("updated goal lost its earlier fields: %s, %v", body, err)
	}
}

func TestCoreGoalExecutorRequiresCapturedIdentity(t *testing.T) {
	t.Parallel()
	const known = "7807993be341e46bdb93e416b6a43afbc300370ec6e946ea2d7a8c09f2ad4925"
	unknown := strings.Repeat("f", 64)
	for _, row := range []struct {
		packageName, digest, origin, tool string
		want                              bool
	}{
		{"goals", known, "directory", "get_goal", true},
		{"goals", known, "bundled", "create_goal", true},
		{"goals", known, "official", "update_goal", true},
		{"goals", known, "official", "end_goal", false},
		{"goals", unknown, "official", "create_goal", false},
		{"goals", unknown, "bundled", "get_goal", false},
		{"other", known, "bundled", "get_goal", false},
	} {
		info := protocol.PluginInfo{Summary: protocol.PluginSummary{Name: row.packageName, Digest: row.digest}, Origin: &protocol.PluginOrigin{Kind: row.origin}}
		if got := new(Node).coreGoalExecutor(new(store.AgentInvocation), info, row.tool) != nil; got != row.want {
			t.Errorf("%s %s tool %q has native executor %v, want %v", row.origin, row.packageName, row.tool, got, row.want)
		}
	}
}

func TestContinuationSettlementUsesPackageDeclarations(t *testing.T) {
	handler := "reconcile"
	ext := &protocol.PluginExtension{Host: &protocol.PluginHostManifest{Turn: &protocol.PluginHostTurn{AfterTurn: &handler}},
		ContinuationMarkers: map[string]protocol.PluginDesktopNavigation{"next": {Label: "Next"}},
		Actions:             []protocol.PluginAction{protocol.PluginActionControlConversation},
	}
	info := protocol.PluginInfo{Summary: protocol.PluginSummary{Name: "independent-workflow"}, Extension: ext}
	if !declaresContinuationSettlement(info) {
		t.Fatal("a continuation-producing callback must settle before its successor")
	}
	ext.Actions = nil
	if declaresContinuationSettlement(info) {
		t.Fatal("an observer without conversation control must not block its successor")
	}
}

func TestSameNameGoalToolUsesItsOwnScript(t *testing.T) {
	t.Parallel()
	node := serviceTestNode(t, moduleapi.Assembly{})
	info := trustPackageFixture(t, node, "goals", "0.2.4", `{"name":"get_goal","description":"Read the package-owned goal","handler":{"name":"read","read_only":true,"parameters":{"type":"object"}}}`)
	info.UpdateSource = &protocol.PluginOrigin{Kind: "official"}
	if declaresContinuationSettlement(info) {
		t.Fatal("a same-name tool without the workflow declarations acquired ordered settlement")
	}
	inv := &store.AgentInvocation{Turn: protocol.DefaultQueuedTurn(), Run: protocol.DefaultConversationRun()}
	tools, err := node.scriptTools(t.Context(), inv, info, nil)
	if err != nil || len(tools) != 1 || tools[0].Binding == nil || tools[0].Binding.Owner != plugins.PackageReference(info) || !tools[0].ReadOnly || tools[0].Approval {
		t.Fatalf("goal package lost its scoped tool: %d tools, %v", len(tools), err)
	}
	tool := tools[0]
	result := tool.Execute(t.Context(), canonical.ToolRequest{ID: "own-goal", Name: tool.Definition.Name, Arguments: json.RawMessage(`{}`)})
	if value, ok := result.Value.(map[string]any); !ok || value["source"] != "package" || result.Failure != nil {
		t.Fatalf("same-name goal tool was replaced: %+v", result)
	}
}
