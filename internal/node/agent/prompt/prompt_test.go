package prompt

import (
	"encoding/json"
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
		if !strings.Contains(Policy(protocol.WorkModeCode, permission, false), "Windows retains") {
			t.Fatal("captured permission policy was not retained")
		}
	}
	if !strings.HasPrefix(Instruction(false), "You are Sailry,") || !strings.Contains(Instruction(false), "Responses\n") || strings.Contains(Instruction(true), "Responses\n") {
		t.Fatal("recipient instructions changed")
	}
}

func TestPendingContextTracksRollingBoundary(t *testing.T) {
	turn, now := protocol.NewTurnID(), time.Unix(1, 0)
	user := func(text string) canonical.Record {
		return canonical.NewRecord(turn, "message", canonical.NewMessage(canonical.User, canonical.Text(0, text)), now)
	}
	old, task, next := user("Old task"), user("Current task"), user("Next task")
	environment, skill := "Captured environment", "Retained owner context: skill:fixture\nInstructions"
	owned, parts := []string{skill}, []string{environment, skill}
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
		want    []string
	}{
		{"tail retains state and owner", []canonical.Record{old, task, context, summary}, nil},
		{"next task retains owner", []canonical.Record{old, task, context, summary, next}, []string{environment}},
		{"covered context is reinjected", []canonical.Record{old, task, context, summary, covered, next}, parts},
		{"hidden tail context is reinjected", []canonical.Record{old, task, context, summary, hide}, parts},
		{"revealed tail context remains present", []canonical.Record{old, task, hidden, reveal, summary}, nil},
	} {
		t.Run(row.name, func(t *testing.T) {
			before, err := json.Marshal(row.records)
			if err != nil {
				t.Fatal(err)
			}
			if got := Pending(row.records, owned, parts); !slices.Equal(got, row.want) {
				t.Fatalf("pending context = %q, want %q", got, row.want)
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
	owned := []string{skill, plan}
	persisted := ContextMessage(turn, []string{environment, skill}, owned, now)
	if persisted == nil || persisted.Metadata.Runtime.UserVisible || !persisted.Metadata.Runtime.AgentVisible || !persisted.Metadata.Runtime.TurnContext {
		t.Fatal("turn context visibility was not persisted")
	}
	m := persisted.Message()
	block := m.Content[0].Data.(*canonical.TextContent)
	if block.Text != "<turn-context>\nSession environment\n\n"+skill+"\n</turn-context>" {
		t.Fatalf("context model bytes changed: %q", block.Text)
	}
	for _, row := range []struct {
		name        string
		records     []canonical.Record
		parts, want []string
	}{
		{"first", []canonical.Record{user("Task")}, []string{environment, skill}, []string{environment, skill}},
		{"unchanged", []canonical.Record{user("Task"), *persisted}, []string{environment, skill}, nil},
		{"changed", []canonical.Record{user("Task"), *persisted}, []string{"Moved environment", skill}, []string{"Moved environment"}},
		{"new owner", []canonical.Record{user("Task"), *persisted}, []string{environment, skill, plan}, []string{environment, plan}},
		{"next task", []canonical.Record{user("Task"), *persisted, user("Next task")}, []string{environment, skill}, []string{environment}},
		{"after compaction", []canonical.Record{user("Task"), *persisted, {Kind: "compaction"}, user("Continue")}, []string{environment, skill}, []string{environment, skill}},
	} {
		t.Run(row.name, func(t *testing.T) {
			got := Pending(row.records, owned, row.parts)
			if !slices.Equal(got, row.want) {
				t.Fatalf("pending %q, want %q", got, row.want)
			}
		})
	}
	if Capture([]string{"first\nsecond"}, nil).State == Capture([]string{"first", "second"}, nil).State {
		t.Fatal("state identity lost exact part boundaries")
	}
}
