package prompt

import (
	"encoding/json"
	"reflect"
	"slices"
	"strings"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestSystemPromptByteStable(t *testing.T) {
	t.Parallel()
	policy, personal, environment, title := "Captured policy", "Personal rules", "First environment", "First title"
	state := canonical.Context{Policy: &policy, Personal: &personal, Environment: &environment, Title: &title, Guides: map[string]canonical.Guide{"z": {Content: "Later guide"}, "a": {Content: "Earlier guide"}}}
	first := System("Captured instructions", state)
	environment, title = "Changed environment\n2026-11-10", "Renamed conversation"
	second := System("Captured instructions", state)
	if first != second || strings.Contains(second, environment) || strings.Contains(second, title) {
		t.Fatal("dynamic turn state changed cached system instructions")
	}
	if !strings.HasPrefix(first, "Captured instructions\n\nCaptured policy\n\nPersonal rules\n\nProject instructions\n") || !strings.HasSuffix(first, "Earlier guide\n\nLater guide") {
		t.Fatalf("captured instruction order changed: %q", first)
	}
	for _, permission := range []protocol.Permission{protocol.PermissionAsk, protocol.PermissionProject, protocol.PermissionFull} {
		if Policy(protocol.WorkModePlan, permission, false) != text("plan") {
			t.Fatal("planning inherited write authority from permission")
		}
		want := "Command sandbox: enabled"
		if permission == protocol.PermissionFull {
			want = "without the workspace sandbox"
			if !strings.Contains(Policy(protocol.WorkModeCode, permission, false), "absolute paths outside the session worktree") {
				t.Fatal("Full permission policy does not explain file tools for out-of-worktree targets")
			}
		}
		if !strings.Contains(Policy(protocol.WorkModeCode, permission, false), want) {
			t.Fatalf("%s policy does not describe its command sandbox", permission)
		}
	}
	for _, child := range []bool{false, true} {
		instructions := Instruction(child)
		if !strings.HasPrefix(instructions, text("baseline")+"\n\n") || !strings.Contains(instructions, "use tool_search when offered to load its exact current name") {
			t.Fatalf("shared instructions missing for child=%t", child)
		}
	}
	if !strings.HasPrefix(Instruction(false), "You are Sailry,") || !strings.Contains(Instruction(false), "Responses\n") || strings.Contains(Instruction(true), "Responses\n") {
		t.Fatal("recipient instructions changed")
	}
}

func TestCollaborateWorkflowUsesAvailableDelegation(t *testing.T) {
	t.Parallel()
	available := Workflow(protocol.WorkModeFlagship, false, true)
	unavailable := Workflow(protocol.WorkModeFlagship, false, false)
	child := Workflow(protocol.WorkModeFlagship, true, false)
	if !strings.Contains(available, "spawn_agent") || !strings.Contains(available, "use the current model") ||
		!strings.Contains(available, "Handle small, straightforward or tightly coupled tasks directly") ||
		!strings.Contains(available, "Do not duplicate in the parent") {
		t.Fatalf("missing collaboration instructions: %q", available)
	}
	if !strings.Contains(unavailable, "Delegation is unavailable") || strings.Contains(unavailable, "spawn_agent") {
		t.Fatalf("unavailable delegation was advertised: %q", unavailable)
	}
	if !strings.Contains(child, "report to the parent") || strings.Contains(child, "spawn_agent") {
		t.Fatalf("workflow did not retain the child boundary: %q", child)
	}
}

func TestParentProgressInstructionsAreRecipientScoped(t *testing.T) {
	t.Parallel()
	const report = "When you collect a substantive result, failure or blocker from a delegated child"
	const link = "](sailry://session/<session-id>)"
	const initialPlan = "Use update_plan to record a plan early for substantial multi-step work"
	const revisePlan = "reconcile the latest recorded plan in the retained `plan` section"
	for _, mode := range []protocol.WorkMode{protocol.WorkModeCode, protocol.WorkModeFlagship, protocol.WorkModePlan} {
		t.Run(string(mode), func(t *testing.T) {
			parentPolicy := Policy(mode, protocol.PermissionFull, false)
			parent := System(Instruction(false), canonical.Context{Policy: &parentPolicy})
			for _, phrase := range []string{
				report, "review its evidence and integrate", "what the child actually verified", "static only", "internal handoffs", link,
				initialPlan, "skip the tool for simple tasks", "without claiming a saved plan", revisePlan,
				"do not guess at a saved plan", "build a fresh plan from the current scope and confirmed evidence",
				"Batch update_plan in the same turn alongside real tool calls",
				"read the corresponding results", "leave unrun required checks pending", "not proof of completion or new authorization",
			} {
				if !strings.Contains(parent, phrase) {
					t.Errorf("parent prompt is missing %q", phrase)
				}
			}
			if mode == protocol.WorkModePlan && !strings.Contains(parent, "Do not implement changes or run tools with side effects, except to record conversation progress with update_plan when offered.") {
				t.Error("Plan prompt lost its read-only policy or conversation progress exception")
			}

			childPolicy := Policy(mode, protocol.PermissionFull, true)
			child := System(Instruction(true), canonical.Context{Policy: &childPolicy})
			if strings.Contains(child, report) || strings.Contains(child, link) || strings.Contains(child, initialPlan) || strings.Contains(child, revisePlan) {
				t.Error("child prompt includes parent progress instructions")
			}
			for _, phrase := range []string{"returns to the parent agent, not the user", "assigned goal", "what remains unverified"} {
				if !strings.Contains(child, phrase) {
					t.Errorf("child prompt is missing %q", phrase)
				}
			}
			for _, phrase := range []string{
				"in assistant text before tool calls that start extended work or a long wait", "meaningful milestones", "avoid fixed intervals",
				"Tool calls, receipts and plan updates do not replace a conversational update",
			} {
				if !strings.Contains(parent, phrase) || !strings.Contains(child, phrase) {
					t.Errorf("shared progress instructions are missing %q", phrase)
				}
			}
		})
	}
}

func TestPendingContextTracksRollingBoundary(t *testing.T) {
	turn, now := protocol.NewTurnID(), time.Unix(1, 0)
	user := func(text string) canonical.Record {
		return canonical.NewRecord(turn, "message", canonical.NewMessage(canonical.User, canonical.Text(0, text)), now)
	}
	old, task, next := user("Old task"), user("Current task"), user("Next task")
	environment, skill := "Captured environment", "Retained owner context: skill:fixture\nInstructions"
	owned, parts := []string{skill}, []Part{{Key: "environment", Text: environment}}
	context := *ContextMessage(turn, parts, owned, now)
	summary := canonical.NewRecord(turn, "compaction", &canonical.Compaction{Boundary: old.ID, Summary: canonical.NewMessage(canonical.User, canonical.Text(0, "Handoff"))}, now)
	covered := canonical.NewRecord(turn, "compaction", &canonical.Compaction{Boundary: context.ID, Summary: canonical.NewMessage(canonical.User, canonical.Text(0, "Updated handoff"))}, now)
	hide := canonical.NewRecord(turn, "operation", &canonical.Operation{Kind: "visibility", Data: &canonical.Visibility{Message: context.ID, AgentVisible: false}}, now)
	reveal := canonical.NewRecord(turn, "operation", &canonical.Operation{Kind: "visibility", Data: &canonical.Visibility{Message: context.ID, AgentVisible: true}}, now)
	hidden := context
	runtime := *hidden.Metadata.Runtime
	runtime.AgentVisible = false
	hidden.Metadata.Runtime = &runtime
	for _, row := range []struct {
		name    string
		records []canonical.Record
		want    []Part
		owned   []string
	}{
		{"tail retains state and owner", []canonical.Record{old, task, context, summary}, nil, nil},
		{"next task retains state and owner", []canonical.Record{old, task, context, summary, next}, nil, nil},
		{"covered context is reinjected", []canonical.Record{old, task, context, summary, covered, next}, parts, owned},
		{"hidden tail context is reinjected", []canonical.Record{old, task, context, summary, hide}, parts, owned},
		{"revealed tail context remains present", []canonical.Record{old, task, hidden, reveal, summary}, nil, nil},
	} {
		t.Run(row.name, func(t *testing.T) {
			before, err := json.Marshal(row.records)
			if err != nil {
				t.Fatal(err)
			}
			if got, additions := Pending(row.records, parts, owned); !slices.Equal(got, row.want) || !slices.Equal(additions, row.owned) {
				t.Fatalf("pending context = %v, %q, want %v, %q", got, additions, row.want, row.owned)
			}
			after, err := json.Marshal(row.records)
			if err != nil || string(before) != string(after) {
				t.Fatal("context identity projection changed canonical records")
			}
		})
	}
}

func TestContextMessageOnlyOnChange(t *testing.T) {
	t.Parallel()
	turn := protocol.TurnID{1}
	now := time.Unix(1, 0)
	user := func(text string) canonical.Record {
		r := canonical.NewRecord(turn, "message", canonical.NewMessage(canonical.User, canonical.Text(0, text)), now)
		r.Provenance.Author = "user"
		return r
	}
	environment, skill, plan := "Session environment", "Retained owner context: skill:fixture:SKILL.md\nBody", "Retained owner context: plan\nSteps"
	parts := []Part{{Key: "environment", Text: environment}}
	persisted := ContextMessage(turn, parts, []string{skill}, now)
	if persisted == nil || persisted.Metadata.Runtime.UserVisible || !persisted.Metadata.Runtime.AgentVisible || !persisted.Metadata.Runtime.TurnContext {
		t.Fatal("turn context visibility was not persisted")
	}
	m := persisted.Message()
	block := m.Content[0].Data.(*canonical.TextContent)
	if !strings.Contains(block.Text, "<context-section name=\"environment\">\nSession environment\n</context-section>") || !strings.HasSuffix(block.Text, skill+"\n</turn-context>") {
		t.Fatalf("context model bytes changed: %q", block.Text)
	}
	for _, row := range []struct {
		name         string
		records      []canonical.Record
		parts, want  []Part
		owned, added []string
	}{
		{"first", []canonical.Record{user("Task")}, parts, parts, []string{skill}, []string{skill}},
		{"unchanged", []canonical.Record{user("Task"), *persisted}, parts, nil, []string{skill}, nil},
		{"changed", []canonical.Record{user("Task"), *persisted}, []Part{{Key: "environment", Text: "Moved environment"}}, []Part{{Key: "environment", Text: "Moved environment"}}, []string{skill}, nil},
		{"new owner", []canonical.Record{user("Task"), *persisted}, parts, nil, []string{skill, plan}, []string{plan}},
		{"next task", []canonical.Record{user("Task"), *persisted, user("Next task")}, parts, nil, []string{skill}, nil},
		{"after compaction", []canonical.Record{user("Task"), *persisted, {Kind: "compaction"}, user("Continue")}, parts, parts, []string{skill}, []string{skill}},
	} {
		t.Run(row.name, func(t *testing.T) {
			got, added := Pending(row.records, row.parts, row.owned)
			if !slices.Equal(got, row.want) || !slices.Equal(added, row.added) {
				t.Fatalf("pending %v, %q, want %v, %q", got, added, row.want, row.added)
			}
		})
	}
	if Capture([]Part{{Key: "first", Text: "second\nthird"}}, nil).State == Capture([]Part{{Key: "first\nsecond", Text: "third"}}, nil).State {
		t.Fatal("state identity lost exact part boundaries")
	}
}

func TestPendingContextRefreshesAfterInputButNotNotice(t *testing.T) {
	turn, now := protocol.NewTurnID(), time.Unix(1, 0)
	parts := []Part{{Key: "deferred_tools", Text: "Deferred tools\nread_session", RefreshOnInput: true}}
	original := *ContextMessage(turn, parts, nil, now)
	notice := canonical.NewRecord(turn, "message", canonical.NewMessage(canonical.User, canonical.Text(0, "Background completion")), now)
	notice.Provenance.Author = "user"
	source := canonical.InputNotice
	index := uint64(0)
	notice.Metadata.Input = &canonical.Input{Source: &source, NoticePart: &index}
	records := []canonical.Record{original, notice}
	if got, _ := Pending(records, parts, nil); len(got) > 0 {
		t.Fatalf("notice-only history refreshed the discovery section: %+v", got)
	}

	guidance := canonical.NewRecord(turn, "message", canonical.NewMessage(canonical.User, canonical.Text(0, "Continue with the remaining checks")), now)
	guidance.Provenance.Author = "user"
	records = append(records, guidance)
	if got, _ := Pending(records, parts, nil); !slices.Equal(got, parts) {
		t.Fatalf("new guidance did not refresh the discovery section: %+v", got)
	}
	refreshed := *ContextMessage(turn, parts, nil, now)
	records = append(records, refreshed, notice)
	if got, _ := Pending(records, parts, nil); len(got) > 0 {
		t.Fatalf("unchanged discovery section refreshed again: %+v", got)
	}
}

func TestContextTransitionsSurviveReload(t *testing.T) {
	turn, now := protocol.NewTurnID(), time.Unix(1, 0)
	workflow := Part{Key: "workflow", Text: "Mode A"}
	changed := Part{Key: "workflow", Text: "Mode B"}
	title := Part{Key: "title", Text: "Assigned title"}
	identity := Part{Key: "execution_identity", Text: "Current turn"}
	var records []canonical.Record
	for _, row := range []struct {
		name        string
		parts, want []Part
	}{
		{"baseline", []Part{workflow, title}, []Part{workflow, title}},
		{"change", []Part{changed, title}, []Part{changed}},
		{"restore earlier value", []Part{workflow, title}, []Part{workflow}},
		{"remove", []Part{workflow}, []Part{{Key: "title"}}},
		{"remains removed", []Part{workflow}, nil},
		{"restore removed section", []Part{workflow, title}, []Part{title}},
		{"clear explicitly", []Part{workflow, {Key: "title"}}, []Part{{Key: "title"}}},
		{"identity only", []Part{workflow, identity}, []Part{identity}},
	} {
		t.Run(row.name, func(t *testing.T) {
			before, err := json.Marshal(records)
			if err != nil {
				t.Fatal(err)
			}
			parts, owned := Pending(records, row.parts, nil)
			if !slices.Equal(parts, row.want) || len(owned) != 0 {
				t.Fatalf("pending = %v, %q, want %v", parts, owned, row.want)
			}
			after, err := json.Marshal(records)
			if err != nil || string(before) != string(after) {
				t.Fatal("delta changed earlier canonical records")
			}
			if record := ContextMessage(turn, parts, owned, now); record != nil {
				records = append(records, *record)
				if len(parts) == 1 && parts[0].Text == "" && !strings.Contains(record.Message().Content[0].Data.(*canonical.TextContent).Text, "Cleared; earlier content") {
					t.Fatal("removed section did not revoke its earlier text")
				}
			}
			encoded, err := json.Marshal(records)
			if err != nil {
				t.Fatal(err)
			}
			var restored []canonical.Record
			if err := json.Unmarshal(encoded, &restored); err != nil {
				t.Fatal(err)
			}
			records = restored
		})
	}
}

func TestCompactionRetainsOnlyEmittedSections(t *testing.T) {
	turn, now := protocol.NewTurnID(), time.Unix(1, 0)
	workflow := Part{Key: "workflow", Text: "Stable workflow"}
	identity := Part{Key: "execution_identity", Text: "Old turn"}
	baseline := *ContextMessage(turn, []Part{workflow, identity}, nil, now)
	identity.Text = "Current turn"
	delta := *ContextMessage(turn, []Part{identity}, nil, now)
	summary := canonical.NewRecord(turn, "compaction", &canonical.Compaction{Boundary: baseline.ID, Summary: canonical.NewMessage(canonical.User, canonical.Text(0, "Handoff")), Start: now, End: now}, now)
	parts, _ := Pending([]canonical.Record{baseline, delta, summary}, []Part{workflow, identity}, nil)
	if !slices.Equal(parts, []Part{workflow}) {
		t.Fatalf("retained identity delta suppressed the missing workflow baseline: %v", parts)
	}
	if !reflect.DeepEqual(delta.Metadata.Runtime.Notes.Context.Parts, map[string]string{identity.Key: digest(identity.Text)}) {
		t.Fatal("a delta claims sections it did not emit")
	}
}

func TestLegacyContextAddsBaselineWithoutRewritingHistory(t *testing.T) {
	turn, now := protocol.NewTurnID(), time.Unix(1, 0)
	skill := "Retained owner context: fixture\nSkill body"
	legacy := canonical.NewRecord(turn, "message", canonical.NewMessage(canonical.User, canonical.Text(0, "<turn-context>\nOld workflow\n\n"+skill+"\n</turn-context>")), now)
	legacy.Metadata.Runtime = &canonical.Runtime{AgentVisible: true, TurnContext: true, Notes: &canonical.OperationNotes{Context: &canonical.PromptContext{State: "legacy", Retained: []string{digest(skill)}}}}
	before, err := json.Marshal(legacy)
	if err != nil {
		t.Fatal(err)
	}
	var restored canonical.Record
	if err := json.Unmarshal(before, &restored); err != nil {
		t.Fatal(err)
	}
	current := []Part{{Key: "workflow", Text: "Current workflow"}, {Key: "execution_identity", Text: "Current session and turn"}}
	parts, owned := Pending([]canonical.Record{restored}, current, []string{skill})
	if !slices.Equal(parts, current) || len(owned) != 0 {
		t.Fatalf("legacy baseline = %v, %q", parts, owned)
	}
	baseline := ContextMessage(turn, parts, owned, now)
	parts, owned = Pending([]canonical.Record{restored, *baseline}, current, []string{skill})
	if len(parts) != 0 || len(owned) != 0 {
		t.Fatalf("legacy baseline repeated: %v, %q", parts, owned)
	}
	after, err := json.Marshal(restored)
	if err != nil || string(before) != string(after) {
		t.Fatal("legacy context changed during upgrade")
	}
}
