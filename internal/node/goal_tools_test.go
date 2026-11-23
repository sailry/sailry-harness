package node

import (
	"encoding/json"
	"testing"

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
	scope := goalToolScope{invocation: inv, plugin: protocol.PluginContext{Package: protocol.PluginReference{Name: "goals"}}}
	if err := scope.change(goal, "complete", "The first check passed", 10); err != nil || goal.Status != "active" || goal.Claim == nil {
		t.Fatalf("foreign verification marked the goal complete: %+v, %v", goal, err)
	}
	inv.Run.Continuation.Package.Name = "goals"
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

func TestCoreGoalExecutorRequiresOfficialPackage(t *testing.T) {
	t.Parallel()
	for _, row := range []struct {
		packageName, origin, tool string
		want                      bool
	}{
		{"goals", "bundled", "get_goal", true},
		{"goals", "official", "create_goal", true},
		{"goals", "official", "update_goal", true},
		{"goals", "official", "end_goal", false},
		{"goals", "local", "create_goal", false},
		{"other", "bundled", "get_goal", false},
	} {
		info := protocol.PluginInfo{Summary: protocol.PluginSummary{Name: row.packageName}, Origin: &protocol.PluginOrigin{Kind: row.origin}}
		if got := new(Node).coreGoalExecutor(new(store.AgentInvocation), info, row.tool) != nil; got != row.want {
			t.Errorf("%s %s tool %q has native executor %v, want %v", row.origin, row.packageName, row.tool, got, row.want)
		}
	}
}
