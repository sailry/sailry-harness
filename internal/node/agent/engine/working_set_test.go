package engine

import (
	"context"
	"database/sql"
	"fmt"
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"sync/atomic"
	"testing"
	"time"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/agent/model"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestDefaultWorkingSetCompactsTwoToolHeavyTurns(t *testing.T) {
	t.Parallel()
	const batchSize = 40
	const latest = "Keep the original inventory IDs in the final report."
	const attachmentText = "Fixture inventory includes item ZX-42."
	m := &scriptedModel{scripts: []script{
		workingSetBatch(0, batchSize, 20_000),
		workingSetBatch(40, batchSize, 37_000),
		workingSetBatch(80, batchSize, 54_000),
		response(text("Initial review completed."), workingSetUsage(71_000)),
		workingSetBatch(120, batchSize, 87_000),
		workingSetBatch(160, batchSize, 103_000),
		workingSetBatch(200, batchSize, 800_000),
		response(text(`{"user_intent":["Finish reviewing the inventory"]}`), finish()),
		workingSetBatch(240, batchSize, 20_000),
		response(text("Inventory report completed."), workingSetUsage(37_000)),
	}}
	var ran atomic.Int32
	evidence := strings.Repeat(" evidence", 400)
	probe := objectTool("probe", true, true, func(_ context.Context, request canonical.ToolRequest) ToolResult {
		ran.Add(1)
		return ToolResult{Format: canonical.JSON, Value: request.ID + evidence}
	})
	f := newFixtureWithTimeout(t, m, 20*time.Second, probe)
	setWorkingSetWindow(t, f)
	attachment := protocol.DefaultAttachment()
	attachment.ID = protocol.NewAttachmentID()
	attachment.Spec.Worktree = f.session.Worktree
	attachment.Spec.Name, attachment.Spec.MediaType = "inventory.txt", "text/plain"
	attachment.Spec.Size = uint64(len(attachmentText))
	if err := f.db.Node.Write(f.ctx, func(db *sql.Conn) error {
		return store.InsertAttachment(f.ctx, db, f.node, attachment)
	}); err != nil {
		t.Fatal(err)
	}
	f.r.attachment = func(_ context.Context, _ protocol.TurnID, id protocol.AttachmentID) (protocol.Attachment, bool, error) {
		if id != attachment.ID {
			return protocol.Attachment{}, false, fmt.Errorf("unexpected attachment %s", id)
		}
		return attachment, true, nil
	}
	f.r.attachmentBytes = func(context.Context, protocol.Attachment) ([]byte, error) {
		return []byte(attachmentText), nil
	}
	f.start()
	f.waitRun(f.submit("Review the initial inventory."), protocol.ConversationStatusCompleted)
	if m.count() != 4 || ran.Load() != 120 {
		t.Fatalf("first turn used %d requests and %d tools", m.count(), ran.Load())
	}
	input := protocol.DefaultConversationInput()
	input.Text, input.Attachments = latest, []protocol.AttachmentID{attachment.ID}
	second := f.command("submit_turn", &protocol.CommandSubmitTurnPayload{Session: f.session.ID, ExpectedRevision: f.session.Revision, Message: input}).Data.(protocol.QueuedTurn)
	f.waitRun(second.ID, protocol.ConversationStatusCompleted)
	if m.count() != 10 || ran.Load() != 280 {
		t.Fatalf("two turns used %d requests and %d tools", m.count(), ran.Load())
	}
	m.mu.Lock()
	before, summary, resumed, finished := m.calls[6], m.calls[7], m.calls[8], m.calls[9]
	m.mu.Unlock()
	assertWorkingSetPairs(t, before, 0, 200)
	assertWorkingSetPairs(t, summary, 0, 0)
	assertWorkingSetPairs(t, resumed, 200, 240)
	assertWorkingSetPairs(t, finished, 200, 280)
	if len(summary.Tools) != 0 || len(resumed.Tools) == 0 {
		t.Fatal("tool declarations are scoped incorrectly across the summary")
	}
	transcript := workingSetText(summary)
	if !strings.Contains(transcript, latest) || !strings.Contains(transcript, attachmentPrefix+attachment.ID.String()) || strings.Contains(transcript, attachmentText) || !strings.Contains(transcript, "probe-199") || strings.Contains(transcript, "probe-200") {
		t.Fatal("summary did not receive the covered prefix and attachment reference")
	}
	for _, request := range []fantasy.Call{resumed, finished} {
		prompt := workingSetText(request)
		if strings.Count(prompt, latest) != 1 || strings.Count(prompt, attachmentText) != 1 || !strings.Contains(prompt, "Finish reviewing the inventory") {
			t.Fatal("continuation lost or duplicated the context summary, latest request, or attachment")
		}
	}
	state := f.state()
	var boundary string
	for i, record := range state.Records {
		if record.Kind != "compaction" {
			continue
		}
		if boundary != "" || record.Turn != second.ID {
			t.Fatal("expected one checkpoint during the second turn")
		}
		if i == 0 || contextTokens(state.Records[:i]) < 800_000 {
			t.Fatal("checkpoint was saved before crossing the default working-set threshold")
		}
		boundary = record.Data.(*canonical.Compaction).Boundary
	}
	for _, record := range state.Records {
		if record.ID != boundary || record.Turn != second.ID {
			continue
		}
		result, ok := record.Message().Content[batchSize-1].Data.(*canonical.ToolResponse)
		if !ok || result.ID != "probe-199" || result.Status != canonical.ToolSuccess {
			t.Fatal("checkpoint did not end at a completed batch inside the second turn")
		}
		return
	}
	t.Fatal("checkpoint boundary did not resolve to the original second-turn history")
}

func TestLargeFixedPromptBelowWindowThreshold(t *testing.T) {
	t.Parallel()
	m := &scriptedModel{scripts: []script{
		workingSetBatch(0, 1, 134_000),
		workingSetBatch(1, 1, 150_000),
		workingSetBatch(2, 1, 166_000),
		response(text("Followed the fixed prompt."), workingSetUsage(182_000)),
	}}
	evidence := strings.Repeat(" evidence", 16_000)
	probe := objectTool("probe", true, true, func(context.Context, canonical.ToolRequest) ToolResult {
		return ToolResult{Format: canonical.JSON, Value: evidence}
	})
	f := newFixtureWithTimeout(t, m, 20*time.Second, probe)
	setWorkingSetWindow(t, f)
	fixed := strings.Repeat(" fixture", 130_000) + "\nPreserve the fixed instruction tail."
	if err := os.WriteFile(filepath.Join(f.r.profile, "instructions.md"), []byte(fixed), 0o600); err != nil {
		t.Fatal(err)
	}
	f.start()
	f.waitRun(f.submit("Review the evidence."), protocol.ConversationStatusCompleted)
	if m.count() != 4 {
		t.Fatalf("requests = %d, want four main requests", m.count())
	}
	m.mu.Lock()
	final := m.calls[3]
	m.mu.Unlock()
	assertWorkingSetPairs(t, final, 0, 3)
	if !strings.Contains(workingSetText(final), fixed) {
		t.Fatal("fixed instructions did not reach the final request")
	}
	mustHaveText(t, f.state().Records, "Followed the fixed prompt.")
}

func TestWorkingSetPreservesExplicitPercentAndModelLimit(t *testing.T) {
	t.Parallel()
	m := &scriptedModel{scripts: []script{response(text("Done"), finish())}}
	f := newFixture(t, m)
	setWorkingSetWindow(t, f)
	trigger := func() uint64 {
		t.Helper()
		var statistics protocol.ConversationStatistics
		if err := f.db.Node.Read(f.ctx, func(db *sql.Conn) error {
			var err error
			statistics, err = store.ReadAgentStatistics(f.ctx, db, f.session.ID)
			return err
		}); err != nil {
			t.Fatal(err)
		}
		if statistics.Budget == nil {
			t.Fatal("statistics lost the compaction budget")
		}
		return statistics.Budget.TriggerTokens
	}
	turn := f.submit("Use the captured default.")
	settings := protocol.DefaultConversationCompactionSettings()
	f.command("save_compaction_settings", &settings)
	if got := trigger(); got != 800_000 {
		t.Fatalf("queued turn trigger = %d, want the captured default", got)
	}
	f.start()
	f.waitRun(turn, protocol.ConversationStatusCompleted)
	if got := trigger(); got != 800_000 {
		t.Fatalf("saved 80%% trigger = %d, want 800000", got)
	}
	if err := f.db.Node.Write(f.ctx, func(db *sql.Conn) error {
		_, err := db.ExecContext(f.ctx, "UPDATE providers SET body=json_set(body,'$.provider.models[0].compaction_tokens',250000)")
		return err
	}); err != nil {
		t.Fatal(err)
	}
	if got := trigger(); got != 250_000 {
		t.Fatalf("explicit model trigger = %d, want 250000", got)
	}
}

func setWorkingSetWindow(t *testing.T, f *engineFixture) {
	t.Helper()
	if err := f.db.Node.Write(f.ctx, func(db *sql.Conn) error {
		_, err := db.ExecContext(f.ctx, "UPDATE providers SET body=json_set(body,'$.provider.models[0].context',1000000)")
		return err
	}); err != nil {
		t.Fatal(err)
	}
}

func workingSetBatch(first, size int, total int64) script {
	parts := make([]fantasy.StreamPart, 0, size+1)
	for i := first; i < first+size; i++ {
		parts = append(parts, call("probe-"+strconv.Itoa(i), "probe", `{}`))
	}
	return response(append(parts, workingSetUsage(total))...)
}

func workingSetUsage(total int64) fantasy.StreamPart {
	part := finish()
	part.ProviderMetadata = fantasy.ProviderMetadata{model.UsageMetadataKey: &model.UsageMetadata{Reported: true, Total: &total}}
	return part
}

func workingSetText(request fantasy.Call) string {
	var content strings.Builder
	for _, message := range request.Prompt {
		for _, part := range message.Content {
			if value, ok := part.(fantasy.TextPart); ok {
				content.WriteString(value.Text)
				content.WriteByte('\n')
			}
		}
	}
	return content.String()
}

func assertWorkingSetPairs(t *testing.T, request fantasy.Call, first, end int) {
	t.Helper()
	calls, results := first, first
	for _, message := range request.Prompt {
		for _, part := range message.Content {
			switch value := part.(type) {
			case fantasy.ToolCallPart:
				if value.ToolCallID != "probe-"+strconv.Itoa(calls) {
					t.Fatalf("tool call %s at position %d", value.ToolCallID, calls)
				}
				calls++
			case fantasy.ToolResultPart:
				if results >= calls || value.ToolCallID != "probe-"+strconv.Itoa(results) {
					t.Fatalf("tool result %s at position %d has no matching call", value.ToolCallID, results)
				}
				results++
			}
		}
	}
	if calls != end || results != end {
		t.Fatalf("got %d tool calls and %d results, want %d pairs", calls-first, results-first, end-first)
	}
}
