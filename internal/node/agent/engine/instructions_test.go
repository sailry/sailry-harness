package engine

import (
	"database/sql"
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/agent/model"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestInstructionRecordCapacity(t *testing.T) {
	// Both files fit raw reads and fit individually when encoded. Together,
	// JSON's escaping fills the same history frame used by AppendCanonical.
	content := strings.Repeat("<", protocol.MAX_FRAME_BYTES/12)
	context := canonical.Context{Personal: &content, Guides: map[string]canonical.Guide{
		"guide": {Path: "AGENTS.md", Content: content},
	}}
	record := canonical.NewRecord(protocol.NewTurnID(), "configuration", &canonical.Configuration{Context: context}, manualNow())
	encoded, err := json.Marshal(record)
	if err != nil || len(encoded) <= protocol.MAX_FRAME_BYTES {
		t.Fatalf("fixture does not cross the encoded history boundary: %v", err)
	}
	err = validateInstructionRecord(record)
	var fault *protocol.Fault
	if !errors.As(err, &fault) || fault.Code != protocol.ErrorCodeInvalidRequest || !strings.Contains(fault.Message, "AGENTS.md") || !strings.Contains(fault.Message, "instructions.md") || !strings.Contains(fault.Message, "encode to") {
		t.Fatalf("combined instruction error = %v", err)
	}
	delete(context.Guides, "guide")
	if err := validateInstructionRecord(canonical.NewRecord(record.Turn, "configuration", &canonical.Configuration{Context: context}, record.Created)); err != nil {
		t.Fatalf("one file within the history capacity was rejected: %v", err)
	}
}

func TestInstructionContextCapacity(t *testing.T) {
	policy, personal := "Captured policy", "Personal preference"
	context := canonical.Context{Policy: &policy, Personal: &personal, Guides: map[string]canonical.Guide{
		"guide": {Path: "AGENTS.md", Content: strings.Repeat("Preserve all instructions.\n", 7000)},
	}}
	binding := model.Binding{Config: protocol.SessionConfig{Model: "fixture"}, Provider: protocol.ConversationProvider{Models: []protocol.ConversationModel{{ID: "fixture", Context: 1000000}}}}
	before, _ := json.Marshal(context)
	if err := validateInstructionContext(binding, context, false); err != nil {
		t.Fatalf("large-window model rejected the complete instructions: %v", err)
	}
	binding.Provider.Models[0].Context = 4096
	err := validateInstructionContext(binding, context, false)
	var fault *protocol.Fault
	if !errors.As(err, &fault) || fault.Code != protocol.ErrorCodeInvalidRequest || !strings.Contains(fault.Message, "estimated") || !strings.Contains(fault.Message, "4096-token") || !strings.Contains(fault.Message, "AGENTS.md") {
		t.Fatalf("small-window instruction failure = %v", err)
	}
	after, _ := json.Marshal(context)
	if string(before) != string(after) {
		t.Fatal("instruction fitting changed the captured rules")
	}
}

func TestLargeInstructionsReachModel(t *testing.T) {
	for _, row := range []struct {
		name  string
		limit uint32
	}{{"large model", 128000}, {"small model", 4096}} {
		t.Run(row.name, func(t *testing.T) {
			m := &scriptedModel{scripts: []script{response(text("Done"), finish())}}
			f := newFixture(t, m)
			var root string
			if err := f.db.Node.Write(f.ctx, func(db *sql.Conn) error {
				if _, err := db.ExecContext(f.ctx, "UPDATE providers SET body=json_set(body,'$.provider.models[0].context',?1)", row.limit); err != nil {
					return err
				}
				var err error
				root, err = store.ReadWorkspaceRoot(f.ctx, db, f.session.Worktree, true)
				return err
			}); err != nil {
				t.Fatal(err)
			}
			guide := strings.Repeat("Keep the declared task scope.\n", 5000) + "Full project instruction tail.\n"
			personal := strings.Repeat("Honor the user's preferences.\n", 5000) + "Full personal instruction tail.\n"
			if err := core.PrivateDirectory(root); err != nil {
				t.Fatal(err)
			}
			for path, value := range map[string]string{filepath.Join(root, "AGENTS.md"): guide, filepath.Join(f.r.profile, "instructions.md"): personal} {
				if err := os.WriteFile(path, []byte(value), 0o600); err != nil {
					t.Fatal(err)
				}
			}
			f.start()
			turn := f.submit("Read the project instructions")
			if row.limit == 4096 {
				run := f.waitRun(turn, protocol.ConversationStatusFailed)
				if run.Error == nil || !strings.Contains(run.Error.Message, "estimated") || m.count() != 0 {
					t.Fatalf("oversized fixed instructions made a model request or lost the capacity explanation: %+v", run.Error)
				}
				if count := f.readQuery("SELECT count(*) FROM agent_events WHERE json_extract(body,'$.kind')='compaction'"); count != 0 {
					t.Fatal("fixed instructions launched a futile compaction")
				}
				return
			}
			f.waitRun(turn, protocol.ConversationStatusCompleted)
			if m.count() != 1 {
				t.Fatalf("model requests = %d", m.count())
			}
			m.mu.Lock()
			var system string
			for _, message := range m.calls[0].Prompt {
				if message.Role == fantasy.MessageRoleSystem {
					for _, part := range message.Content {
						if value, ok := fantasy.AsMessagePart[fantasy.TextPart](part); ok {
							system += value.Text
						}
					}
				}
			}
			m.mu.Unlock()
			if !strings.Contains(system, guide) || !strings.Contains(system, personal) {
				t.Fatal("model did not receive both complete instruction files")
			}
			state := f.state()
			if state.Context.Personal == nil || !strings.HasSuffix(*state.Context.Personal, personal) {
				t.Fatal("reloaded history lost complete personal instructions")
			}
			found := false
			for _, stored := range state.Context.Guides {
				found = found || stored.Path == "AGENTS.md" && strings.HasSuffix(stored.Content, guide)
			}
			if !found {
				t.Fatal("reloaded history lost complete project instructions")
			}
		})
	}
}
