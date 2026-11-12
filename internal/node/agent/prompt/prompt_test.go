package prompt

import (
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
