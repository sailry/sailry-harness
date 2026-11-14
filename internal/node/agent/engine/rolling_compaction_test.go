package engine

import (
	"encoding/json"
	"reflect"
	"strings"
	"testing"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/agent/model"
	"github.com/sailry/sailry-harness/internal/node/store"
)

func compactionOverflow(t *testing.T, f *engineFixture, x *execution) store.AgentState {
	t.Helper()
	code := "context_overflow"
	message := canonical.NewMessage(canonical.Assistant)
	message.Error = &canonical.ResponseError{Code: &code, Message: "context full"}
	record := canonical.NewRecord(x.inv.Run.Turn, "message", message, f.r.clock.Now())
	record.Metadata.Runtime = &canonical.Runtime{UserVisible: true, AgentVisible: false}
	if err := f.r.save(f.ctx, f.session.ID, record); err != nil {
		t.Fatal(err)
	}
	return f.state()
}

func compactionPrefix(t *testing.T, f *engineFixture, x *execution) store.AgentState {
	t.Helper()
	older := canonical.NewRecord(x.inv.Run.Turn, "message", canonical.NewMessage(canonical.Assistant, canonical.Text(0, strings.Repeat("Older result to summarize. ", 1800))), f.r.clock.Now())
	recent := canonical.NewRecord(x.inv.Run.Turn, "message", canonical.NewMessage(canonical.User, canonical.Text(0, "Recent task")), f.r.clock.Now())
	if err := f.r.save(f.ctx, f.session.ID, older, recent); err != nil {
		t.Fatal(err)
	}
	return f.state()
}

func TestRollingCompactionPreservesTailAcrossUpdatesAndFailure(t *testing.T) {
	modelScript := &scriptedModel{scripts: []script{
		response(text(`{"user_intent":["First rolling state"]}`), finish()),
		response(text(`{"user_intent":["Second rolling state"]}`), finish()),
		response(text(`{"user_intent":["unfinished`), finish()),
	}}
	f, x, binding, _ := compactionFixture(t, modelScript)
	compactionPrefix(t, f, x)
	message := func(role canonical.Role, blocks ...canonical.Block) canonical.Record {
		return canonical.NewRecord(x.inv.Run.Turn, "message", canonical.NewMessage(role, blocks...), f.r.clock.Now())
	}
	pair := func(id, evidence string) []canonical.Record {
		call := canonical.ToolRequest{ID: id, Name: "read_file", Arguments: json.RawMessage(`{"path":"fixture.go"}`)}
		return []canonical.Record{
			message(canonical.Assistant, canonical.Block{Kind: "tool_request", Data: &call}),
			message(canonical.User, responseBlock(0, call, ToolResult{Value: evidence})),
		}
	}
	if err := f.r.save(f.ctx, f.session.ID, pair("first-read", "First retained evidence")...); err != nil {
		t.Fatal(err)
	}
	check := func(before store.AgentState, tail []canonical.Record, manual bool) canonical.Record {
		t.Helper()
		original, _ := json.Marshal(before.Records)
		if err := f.r.compact(x, binding, before, manual); err != nil {
			t.Fatal(err)
		}
		after := f.state()
		if len(after.Records) != len(before.Records)+1 || after.Records[len(before.Records)].Kind != "compaction" {
			t.Fatal("rolling compaction copied tail records or appended a synthetic continuation")
		}
		summary := after.Records[len(before.Records)]
		projected := model.VisibleRecords(after.Records)
		if len(projected) != len(tail)+1 || projected[0].ID != summary.ID || !reflect.DeepEqual(projected[1:], tail) || !reflect.DeepEqual(model.Answered(projected), projected) {
			t.Fatal("rolling projection lost summary order, original tail or a complete tool exchange")
		}
		unchanged, _ := json.Marshal(after.Records[:len(before.Records)])
		if string(original) != string(unchanged) {
			t.Fatal("rolling compaction rewrote canonical history")
		}
		return summary
	}
	firstState := f.state()
	firstTail := firstState.Records[len(firstState.Records)-3:]
	first := check(firstState, firstTail, false)
	beforeSkip := f.state()
	if err := f.r.compact(x, binding, beforeSkip, false); err != nil || modelScript.count() != 1 || len(f.state().Records) != len(beforeSkip.Records) {
		t.Fatal("unchanged retained tail caused another summary request")
	}
	secondTail := []canonical.Record{message(canonical.User, canonical.Text(0, "Newest task"))}
	secondTail = append(secondTail, pair("second-read", "Newest retained evidence")...)
	growth := message(canonical.Assistant, canonical.Text(0, strings.Repeat("New completed evidence to summarize. ", 1800)))
	if err := f.r.save(f.ctx, f.session.ID, append([]canonical.Record{growth}, secondTail...)...); err != nil {
		t.Fatal(err)
	}
	secondState := f.state()
	secondTail = secondState.Records[len(secondState.Records)-3:]
	second := check(secondState, secondTail, true)
	if first.ID == second.ID || first.Data.(*canonical.Compaction).Boundary == second.Data.(*canonical.Compaction).Boundary {
		t.Fatal("the next rolling summary did not advance coverage")
	}
	for _, record := range model.VisibleRecords(f.state().Records) {
		if record.ID == first.ID {
			t.Fatal("the old summary remained beside the updated summary")
		}
	}
	system := modelScript.calls[1].Prompt[0].Content[0].(fantasy.TextPart).Text
	if strings.Count(system, "First rolling state") != 1 || !strings.Contains(system, "First retained evidence") || strings.Contains(system, "Newest retained evidence") {
		t.Fatal("the next summary did not absorb the earlier handoff and newly covered evidence exactly once")
	}
	more := message(canonical.Assistant, canonical.Text(0, strings.Repeat("Further completed evidence to summarize. ", 1800)))
	last := message(canonical.User, canonical.Text(0, "Keep this pending task"))
	if err := f.r.save(f.ctx, f.session.ID, more, last); err != nil {
		t.Fatal(err)
	}
	beforeFailure := f.state()
	visibleBefore := model.VisibleRecords(beforeFailure.Records)
	if err := f.r.compact(x, binding, beforeFailure, false); err == nil {
		t.Fatal("malformed rolling summary was accepted")
	}
	afterFailure := f.state()
	if len(afterFailure.Records) != len(beforeFailure.Records)+2 || !reflect.DeepEqual(model.VisibleRecords(afterFailure.Records), visibleBefore) || modelScript.count() != 3 {
		t.Fatal("failed rolling summary changed the boundary or lost its usage and failure records")
	}
	compactionFailed(t, f, afterFailure.Records[len(afterFailure.Records)-1])
}

func TestRollingCompactionAdvancesWithinOneLongToolTurn(t *testing.T) {
	m := &scriptedModel{scripts: []script{response(text(`{"user_intent":["Continue the original task"]}`), finish())}}
	f, x, binding, _ := compactionFixture(t, m)
	binding.Provider.Models[0].Context = 16000
	pair := func(id, evidence string) []canonical.Record {
		call := canonical.ToolRequest{ID: id, Name: "read_file", Arguments: json.RawMessage(`{"path":"fixture.go"}`)}
		return []canonical.Record{
			canonical.NewRecord(x.inv.Run.Turn, "message", canonical.NewMessage(canonical.Assistant, canonical.Block{Kind: "tool_request", Data: &call}), f.r.clock.Now()),
			canonical.NewRecord(x.inv.Run.Turn, "message", canonical.NewMessage(canonical.User, responseBlock(0, call, ToolResult{Value: evidence})), f.r.clock.Now()),
		}
	}
	old := pair("old-read", strings.Repeat("archived0123456789XYZ ", 6000))
	tail := pair("current-read", "Current exact evidence")
	if err := f.r.save(f.ctx, f.session.ID, append(old, tail...)...); err != nil {
		t.Fatal(err)
	}
	before := f.state()
	tail = before.Records[len(before.Records)-2:]
	if err := f.r.compact(x, binding, before, false); err != nil {
		t.Fatal(err)
	}
	after := f.state()
	if len(after.Records) != len(before.Records)+1 || m.count() != 1 {
		t.Fatal("one long tool turn did not compact its completed prefix")
	}
	summary := after.Records[len(before.Records)]
	projected := model.VisibleRecords(after.Records)
	if summary.Data.(*canonical.Compaction).Boundary != old[1].ID || len(projected) != 3 || projected[0].ID != summary.ID || !reflect.DeepEqual(projected[1:], tail) || !reflect.DeepEqual(model.Answered(projected), projected) {
		t.Fatal("long-turn compaction split or copied the newest tool exchange")
	}
}
