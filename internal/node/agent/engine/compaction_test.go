package engine

import (
	"context"
	"database/sql"
	"encoding/json"
	"reflect"
	"strings"
	"testing"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	handoff "github.com/sailry/sailry-harness/internal/node/agent/compact"
	"github.com/sailry/sailry-harness/internal/node/agent/model"
	"github.com/sailry/sailry-harness/internal/node/agent/tokens"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestCompactionHandoff(t *testing.T) {
	t.Run("structured candidates", func(t *testing.T) {
		for _, row := range []struct {
			name, document, want string
		}{
			{"coercion", `{"user_intent":{"goal":"Fix parser"},"pending_tasks":[9007199254740993],"files":["a.go",{}]}`, "goal: Fix parser"},
			{"control character", "{\"user_intent\":[\"Fix\nparser\"]}", "Fix\nparser"},
			{"later document", "<analysis>example ```json\n{\"user_intent\":[\"Wrong\"]}\n```</analysis>\n{\"user_intent\":[\"Real goal\"]}", "Real goal"},
			{"quoted terminator", "<analysis>review</analysis>\n```json\n{\"user_intent\":[\"Fix </analysis> marker\"]}\n```", "Fix </analysis> marker"},
			{"analysis example only", "<analysis>literal </analysis>\n```json\n{\"user_intent\":[\"Wrong\"]}\n```\n</analysis>\nCould not summarize", ""},
			{"truncated document", "```json\n{\"user_intent\":[\"Keep\"],\"pending_tasks\":[\"Unfinished", ""},
			{"empty document", `{"files":[{}],"user_intent":[" "],"current_work":""}`, ""},
			{"prose example", `A quoted example {"user_intent":["Wrong"]} is not a handoff`, ""},
		} {
			t.Run(row.name, func(t *testing.T) {
				summary, err := handoff.Parse(row.document, 4000)
				if row.want == "" {
					if err == nil {
						t.Fatal("unusable document replaced the handoff")
					}
					return
				}
				if err != nil || len(summary.UserIntent) != 1 || summary.UserIntent[0] != row.want {
					t.Fatalf("summary=%+v, error=%v", summary, err)
				}
				if row.name == "coercion" && (summary.PendingTasks[0] != "9007199254740993" || len(summary.Files) != 1) {
					t.Fatal("field coercion lost a numeric identifier or retained a blank file")
				}
			})
		}
	})
	t.Run("latest transcript and audiences", func(t *testing.T) {
		turn := protocol.NewTurnID()
		record := func(role canonical.Role, blocks ...canonical.Block) canonical.Record {
			return canonical.NewRecord(turn, "message", canonical.NewMessage(role, blocks...), manualNow())
		}
		result := func(id, value string) canonical.Block {
			raw, _ := json.Marshal(value)
			return canonical.Block{Kind: "tool_response", Data: &canonical.ToolResponse{ID: id, Status: canonical.ToolSuccess, Revision: strings.Repeat("a", 64), Result: canonical.Payload{Value: raw}}}
		}
		latest := "Latest start " + strings.Repeat("x", 2200) + " LATEST_MIDDLE " + strings.Repeat("y", 2200) + " Latest end"
		old := "Old start " + strings.Repeat("文🙂", 700) + " OLD_MIDDLE " + strings.Repeat("字🙂", 700) + " Old end"
		history := []canonical.Record{
			record(canonical.User, canonical.Text(0, "Keep the public API")),
			record(canonical.User, result("old", old)),
			record(canonical.User, canonical.Text(0, "Current request")),
			record(canonical.User, result("latest", latest)),
			record(canonical.Assistant, canonical.Block{Kind: "thinking", Data: &canonical.Thinking{Text: "Private reasoning"}}),
			record(canonical.User, canonical.Block{Kind: "image", Data: &canonical.AssetContent{Asset: canonical.AssetRef{Mime: "image/png", Annotations: json.RawMessage(`{"audience":["user"]}`)}}}),
			record(canonical.User, canonical.Block{Kind: "tool_response", Data: &canonical.ToolResponse{ID: "audience", Status: canonical.ToolSuccess, Format: canonical.MCP, Revision: strings.Repeat("b", 64), Result: canonical.Payload{Value: json.RawMessage(`{"content":[{"type":"text","text":"Private UI table","annotations":{"audience":["user"]}},{"type":"text","text":"Model evidence","annotations":{"audience":["assistant"]}}]}`)}}}),
		}
		before, _ := json.Marshal(history)
		prepared, err := handoff.Prepare(history, 64000, 8000)
		if err != nil || !strings.Contains(prepared, latest) || !strings.Contains(prepared, "Old start") || !strings.Contains(prepared, "Old end") || strings.Contains(prepared, "OLD_MIDDLE") {
			t.Fatalf("latest transcript was altered or older evidence was not previewed: %v", err)
		}
		for _, private := range []string{"Private reasoning", "Private UI table", "[image:"} {
			if strings.Contains(prepared, private) {
				t.Fatalf("summary exposed %q", private)
			}
		}
		if !strings.Contains(prepared, "Model evidence") || !strings.Contains(prepared, `"call":"latest"`) {
			t.Fatal("visible evidence or original output reference was lost")
		}
		base, _ := handoff.Prepare(nil, 64000, 8000)
		tight := uint64(8000+256+100) + tokens.Text(base)
		// A latest turn over the budget is shortened and keeps the user's message.
		if shortened, err := handoff.Prepare(history, tight, 8000); err != nil || !strings.Contains(shortened, "[user]: Current request") || strings.Contains(shortened, "Keep the public API") || strings.Contains(shortened, "LATEST_MIDDLE") {
			t.Fatalf("oversized latest turn was not shortened to its request: %v", err)
		}
		if _, err = handoff.Prepare(history, 8000, 8000); err == nil {
			t.Fatal("summary prompt and output reserve were not counted")
		}
		older := record(canonical.User, canonical.Text(0, strings.Repeat("Old constraint ", 1000)))
		newest := record(canonical.User, canonical.Text(0, "Newest exact request"))
		fitted, err := handoff.Prepare([]canonical.Record{older, newest}, tight, 8000)
		if err != nil || !strings.Contains(fitted, "Newest exact request") || strings.Contains(fitted, "Old constraint") {
			t.Fatal("older turns were not omitted as the final fitting fallback")
		}
		after, _ := json.Marshal(history)
		if string(before) != string(after) {
			t.Fatal("summary projection mutated original history")
		}
	})
	t.Run("binding bound usage and coverage", func(t *testing.T) {
		work := strings.Repeat("Preserve state and complete the task. ", 1000)
		document, _ := json.Marshal(map[string]any{"user_intent": []string{"Fix parser"}, "user_messages": []string{"Keep the v1 contract"}, "pending_tasks": []string{"Add regression tests"}, "current_work": work, "next_step": work, "technical_concepts": []string{strings.Repeat("Disposable reference ", 6000)}})
		input, reportedTotal := int64(123), int64(999)
		complete := finish()
		complete.ProviderMetadata = fantasy.ProviderMetadata{model.UsageMetadataKey: &model.UsageMetadata{Reported: true, Input: &input, Total: &reportedTotal}}
		m := &scriptedModel{scripts: []script{response(text(string(document)), complete)}}
		f, x, binding, state := compactionFixture(t, m)
		binding.Config.Effort = protocol.Effort{Kind: "high"}
		binding.Provider.Models[0].Reasoning = true
		binding.Provider.Models[0].Efforts = []protocol.Effort{{Kind: "high"}, {Kind: "low"}, {Kind: "none"}}
		binding.Provider.Models[0].Output = 24000
		x.inv.Message.Text = "Keep exact task identifiers"
		var captured model.Binding
		f.r.model = func(_ context.Context, value model.Binding) (fantasy.LanguageModel, error) {
			captured = value
			return m, nil
		}
		// An account credential is resolved again for the summary request.
		renewed := `{"kind":"copilot","access":"renewed","endpoint":"https://api.githubcopilot.com","renewal_pending":false}`
		binding.Provider.Authentication, binding.Provider.Endpoint = protocol.AuthenticationCopilot, "https://api.githubcopilot.com"
		f.r.authorizeProvider = func(_ context.Context, inv *store.AgentInvocation) error {
			secret := protocol.NewSecret(renewed)
			inv.Secret = &secret
			return nil
		}
		before, _ := json.Marshal(state.Records)
		if err := f.r.compact(x, binding, state, true); err != nil {
			t.Fatal(err)
		}
		if captured.Secret == nil || captured.Secret.Expose() != renewed {
			t.Fatal("summary request kept the credential captured at turn start")
		}
		if captured.Provider.ID != binding.Provider.ID || captured.Provider.Endpoint != binding.Provider.Endpoint || captured.Config.Model != binding.Config.Model || captured.Config.Effort.Kind != "none" || binding.Config.Effort.Kind != "high" {
			t.Fatal("summary did not retain the captured provider/model with independent lowest effort")
		}
		request := m.calls[0]
		if len(request.Tools) != 0 || request.MaxOutputTokens == nil || *request.MaxOutputTokens != 8000 || len(request.Prompt) != 2 || !strings.Contains(request.Prompt[1].Content[0].(fantasy.TextPart).Text, x.inv.Message.Text) {
			t.Fatal("summary request lost its no-tools output bound or manual focus")
		}
		after := f.state()
		original, _ := json.Marshal(after.Records[:len(state.Records)])
		if string(before) != string(original) || len(after.Records) != len(state.Records)+2 {
			t.Fatal("manual compaction changed original records or carried a duplicate input")
		}
		saved := after.Records[len(state.Records)]
		compacted := saved.Data.(*canonical.Compaction)
		rendered := compacted.Summary.Content[0].Data.(*canonical.TextContent).Text
		if tokens.Text(rendered) > 4000 || !strings.Contains(rendered, "Fix parser") || !strings.Contains(rendered, "Keep the v1 contract") || !strings.Contains(rendered, "Add regression tests") || strings.Contains(rendered, "Disposable reference") {
			t.Fatal("handoff exceeded its source bound or lost essential state")
		}
		if compacted.Boundary != state.Records[len(state.Records)-1].ID || !compacted.Start.Equal(state.Records[0].Created) || !compacted.End.Equal(state.Records[len(state.Records)-1].Created) {
			t.Fatal("summary coverage timestamps describe the model request instead of the replaced history")
		}
		usage := compacted.Summary.Usage
		normalized, _ := handoff.Parse(string(document), 4000)
		if usage.Input == nil || *usage.Input != 123 || usage.Output == nil || *usage.Output != tokens.Text(normalized.Document()) || usage.Total == nil || *usage.Total != 123+*usage.Output || saved.Metadata.Execution.Source.ID != binding.Provider.ID {
			t.Fatal("summary lost reported optional usage, normalized output estimate or captured scope")
		}
	})
	t.Run("cap follows the window", func(t *testing.T) {
		var concepts []string
		for range 600 {
			concepts = append(concepts, strings.Repeat("Reference kept by a large window ", 6))
		}
		document, _ := json.Marshal(map[string]any{"user_intent": []string{"Fix parser"}, "technical_concepts": concepts})
		for _, row := range []struct {
			context        uint32
			output         int64
			limit          uint64
			target, cutoff string
		}{{200000, 8000, 4000, "under 3,000 tokens", "beyond 4,000 tokens"}, {1000000, 24000, 12000, "under 9,000 tokens", "beyond 12,000 tokens"}} {
			m := &scriptedModel{scripts: []script{response(text(string(document)), finish())}}
			f, x, binding, state := compactionFixture(t, m)
			binding.Provider.Models[0].Context, binding.Provider.Models[0].Output = row.context, 64000
			if err := f.r.compact(x, binding, state, true); err != nil {
				t.Fatal(err)
			}
			request := m.calls[0]
			system := request.Prompt[0].Content[0].(fantasy.TextPart).Text
			if request.MaxOutputTokens == nil || *request.MaxOutputTokens != row.output || !strings.Contains(system, row.target) || !strings.Contains(system, row.cutoff) {
				t.Fatalf("%d window: output limit %v or prompt limits are wrong", row.context, request.MaxOutputTokens)
			}
			budget := store.AgentCompactionBudget(binding.Provider.Models[0], 80)
			if int64(budget.SummaryOutput) != row.output || budget.SummaryInput != uint64(row.context)-uint64(row.output) || budget.OutputReserve != 64000 {
				t.Fatalf("%d window: reported budget = %+v", row.context, budget)
			}
			after := f.state()
			rendered := after.Records[len(state.Records)].Data.(*canonical.Compaction).Summary.Content[0].Data.(*canonical.TextContent).Text
			if size := tokens.Text(rendered); size > row.limit || row.limit > 4000 && size <= 4000 {
				t.Fatalf("%d window: handoff is %d tokens under a cap of %d", row.context, size, row.limit)
			}
		}
	})
	t.Run("failed summaries retain usage", func(t *testing.T) {
		for _, row := range []struct {
			name, document, stop string
			reason               protocol.ConversationCompactionReason
		}{
			{"malformed", `{"user_intent":["unfinished`, "stop", protocol.ConversationCompactionReasonUnexpectedOutput},
			{"empty", `{}`, "stop", protocol.ConversationCompactionReasonUnexpectedOutput},
			{"blank", ` `, "stop", protocol.ConversationCompactionReasonEmptySummary},
			{"truncated", `{"user_intent":["Valid but limited"]}`, "MAX_OUTPUT_TOKENS", protocol.ConversationCompactionReasonTruncatedSummary},
		} {
			t.Run(row.name, func(t *testing.T) {
				part := finish()
				part.FinishReason = fantasy.FinishReason(row.stop)
				count := int64(77)
				part.ProviderMetadata = fantasy.ProviderMetadata{model.UsageMetadataKey: &model.UsageMetadata{Reported: true, Total: &count}}
				f, x, binding, before := compactionFixture(t, &scriptedModel{scripts: []script{response(text(row.document), part)}})
				if err := f.r.compact(x, binding, before, true); err == nil {
					t.Fatal("invalid summary replaced context")
				}
				after := f.state()
				if len(after.Records) != len(before.Records)+2 {
					t.Fatal("failed summary usage observation or failure record was lost")
				}
				observation := after.Records[len(before.Records)]
				if observation.Kind != "message" || !compactionUsage(observation) || observation.Metadata.Runtime.AgentVisible || observation.Message().Usage.Total == nil || *observation.Message().Usage.Total != 77 {
					t.Fatal("failed summary usage was not preserved independently")
				}
				failure := compactionFailed(t, f, after.Records[len(before.Records)+1])
				if failure.Source != protocol.ConversationCompactionSourceManual || failure.Stage != protocol.ConversationCompactionStageSummary || failure.Reason != row.reason || failure.Continued || failure.Cause == nil || *failure.Cause == "" {
					t.Fatalf("failure = %+v", failure)
				}
			})
		}
	})
	t.Run("failures are recorded", func(t *testing.T) {
		status := func(code int) script {
			return func(_ context.Context, _ fantasy.Call, yield func(fantasy.StreamPart) bool) {
				yield(fantasy.StreamPart{Type: fantasy.StreamPartTypeError, Error: &fantasy.ProviderError{StatusCode: code, ResponseBody: []byte(`{"error":{"message":"Summary refused"}}`)}})
			}
		}
		for _, row := range []struct {
			name    string
			scripts []script
			context uint32
			stopped bool
			records int
			stage   protocol.ConversationCompactionStage
			reason  protocol.ConversationCompactionReason
			status  uint16
			cause   string
		}{
			{"budget", nil, 100, false, 1, protocol.ConversationCompactionStageBudget, protocol.ConversationCompactionReasonNoFittingPrefix, 0, "summary prompt and output exceed the context limit"},
			{"unauthorized", []script{status(401)}, 0, false, 2, protocol.ConversationCompactionStageRequest, protocol.ConversationCompactionReasonAuthorizationFailed, 401, "Summary refused (HTTP 401)"},
			{"rejected", []script{status(400)}, 0, false, 2, protocol.ConversationCompactionStageRequest, protocol.ConversationCompactionReasonServiceRejected, 400, "Summary refused (HTTP 400)"},
			{"stopped", []script{status(400)}, 0, true, 1, "", "", 0, ""},
		} {
			t.Run(row.name, func(t *testing.T) {
				f, x, binding, before := compactionFixture(t, &scriptedModel{scripts: row.scripts})
				if row.context > 0 {
					binding.Provider.Models[0].Context, binding.Provider.Models[0].Output = row.context, row.context
				}
				if row.stopped {
					stopped, cancel := context.WithCancel(f.ctx)
					x.ctx = stopped
					scripted := row.scripts[0]
					f.r.model = func(context.Context, model.Binding) (fantasy.LanguageModel, error) {
						return &scriptedModel{scripts: []script{func(ctx context.Context, call fantasy.Call, yield func(fantasy.StreamPart) bool) {
							cancel()
							scripted(ctx, call, yield)
						}}}, nil
					}
				}
				if err := f.r.compact(x, binding, before, false); err == nil {
					t.Fatal("failed compaction returned no error")
				}
				added := f.state().Records[len(before.Records):]
				if len(added) != row.records {
					t.Fatalf("added %d records, want %d", len(added), row.records)
				}
				last := added[len(added)-1]
				if row.stopped {
					if last.Metadata.Compaction != nil {
						t.Fatal("a stopped compaction was recorded as a failure")
					}
					return
				}
				failure := compactionFailed(t, f, last)
				if failure.Source != protocol.ConversationCompactionSourceAutomatic || failure.Stage != row.stage || failure.Reason != row.reason || failure.Continued || failure.Cause == nil || *failure.Cause != row.cause || (row.status == 0) != (failure.Status == nil) || failure.Status != nil && *failure.Status != row.status {
					t.Fatalf("failure = %+v", failure)
				}
			})
		}
	})
	t.Run("three automatic failures pause the threshold", func(t *testing.T) {
		refused := func(_ context.Context, _ fantasy.Call, yield func(fantasy.StreamPart) bool) {
			yield(fantasy.StreamPart{Type: fantasy.StreamPartTypeError, Error: &fantasy.ProviderError{StatusCode: 400, ResponseBody: []byte(`{"error":{"message":"Summary refused"}}`)}})
		}
		m := &scriptedModel{scripts: []script{refused, refused, refused, refused, refused, response(text(`{"user_intent":["Continue task"]}`), finish())}}
		f, x, binding, state := compactionFixture(t, m)
		trigger := uint32(1)
		binding.Provider.Models[0].CompactionTokens = &trigger
		other := binding
		other.Config.Model = "other"
		fail := func(manual bool) protocol.ConversationCompactionFailure {
			t.Helper()
			before := f.state()
			if err := f.r.compact(x, binding, before, manual); err == nil {
				t.Fatal("refused summary compacted")
			}
			added := f.state().Records[len(before.Records):]
			last := added[len(added)-1]
			if last.Metadata.Compaction == nil || last.Metadata.Compaction.Failure == nil || last.Metadata.Execution == nil || last.Metadata.Execution.Source.Model != binding.Config.Model {
				t.Fatalf("failure record = %+v", last.Metadata)
			}
			return *last.Metadata.Compaction.Failure
		}
		if !shouldCompact(binding, state, 80) {
			t.Fatal("fixture is not over its threshold")
		}
		for attempt := 1; attempt <= 2; attempt++ {
			if failure := fail(false); failure.Paused {
				t.Fatalf("failure %d paused automatic compaction", attempt)
			}
		}
		// A manual failure neither counts nor pauses.
		if failure := fail(true); failure.Paused || failure.Source != protocol.ConversationCompactionSourceManual {
			t.Fatalf("manual failure = %+v", failure)
		}
		if !shouldCompact(binding, f.state(), 80) {
			t.Fatal("two automatic failures paused the threshold")
		}
		if failure := fail(false); !failure.Paused {
			t.Fatal("the third failure does not say automatic compaction is paused")
		}
		paused := f.state()
		if shouldCompact(binding, paused, 80) {
			t.Fatal("the threshold still compacts after three automatic failures")
		}
		// Another model starts again; the threshold itself is not what stopped it.
		if automaticFailures(binding, paused.Records) != 3 || automaticFailures(other, paused.Records) != 0 {
			t.Fatalf("failures = %d, for another model %d", automaticFailures(binding, paused.Records), automaticFailures(other, paused.Records))
		}
		// Overflow recovery still tries, and its failure does not say the pause again.
		if failure := fail(false); failure.Paused {
			t.Fatal("the pause was told twice")
		}
		if m.count() != 5 {
			t.Fatalf("summary requests = %d", m.count())
		}
		// A compaction that succeeds ends the pause.
		if err := f.r.compact(x, binding, f.state(), true); err != nil {
			t.Fatal(err)
		}
		if automaticFailures(binding, f.state().Records) != 0 {
			t.Fatal("a successful compaction left the threshold paused")
		}
	})
	t.Run("automatic continuation", func(t *testing.T) {
		for _, merged := range []bool{false, true} {
			f, x, binding, state := compactionFixture(t, &scriptedModel{scripts: []script{response(text(`{"user_intent":["Continue task"]}`), finish())}})
			if merged {
				original := state.Records[0]
				summary := canonical.NewRecord(x.inv.Run.Turn, "compaction", &canonical.Compaction{Boundary: original.ID, Summary: canonical.NewMessage(canonical.User, canonical.Text(0, "Earlier handoff must merge")), Start: original.Created, End: original.Created}, f.r.clock.Now())
				summary.Metadata.Runtime = &canonical.Runtime{AgentVisible: true}
				call := canonical.ToolRequest{ID: "read", Name: "beta", Arguments: json.RawMessage(`{}`)}
				request := canonical.NewRecord(x.inv.Run.Turn, "message", canonical.NewMessage(canonical.Assistant, canonical.Block{Kind: "tool_request", Data: &call}), f.r.clock.Now())
				result := canonical.NewRecord(x.inv.Run.Turn, "message", canonical.NewMessage(canonical.User, responseBlock(0, call, ToolResult{Value: "Latest evidence"})), f.r.clock.Now())
				turnContext := canonical.NewRecord(x.inv.Run.Turn, "message", canonical.NewMessage(canonical.User, canonical.Text(0, "Reinjected turn context")), f.r.clock.Now())
				turnContext.Metadata.Runtime = &canonical.Runtime{AgentVisible: true, TurnContext: true}
				if err := f.r.save(f.ctx, f.session.ID, summary, request, result, turnContext); err != nil {
					t.Fatal(err)
				}
				state = f.state()
			}
			if err := f.r.compact(x, binding, state, false); err != nil {
				t.Fatal(err)
			}
			added := f.state().Records[len(state.Records):]
			if merged {
				if len(added) != 2 || !strings.Contains(added[1].Message().Content[0].Data.(*canonical.TextContent).Text, "Continue calling tools") {
					t.Fatal("an earlier summary or reinjected context was carried as user input")
				}
			} else if len(added) != 3 || added[2].Message().Role != canonical.User || added[2].Message().Content[0].Data.(*canonical.TextContent).Text != "Original task" || !strings.Contains(added[1].Message().Content[0].Data.(*canonical.TextContent).Text, "Just continue") {
				t.Fatal("latest automatic input or natural continuation was lost")
			}
		}
	})
	t.Run("latest usage and discovered tools", func(t *testing.T) {
		turn := protocol.NewTurnID()
		record := func(role canonical.Role, blocks ...canonical.Block) canonical.Record {
			return canonical.NewRecord(turn, "message", canonical.NewMessage(role, blocks...), manualNow())
		}
		old := record(canonical.User, canonical.Text(0, "Earlier"))
		stale := uint64(99999)
		old.Message().Usage = &canonical.Usage{Total: &stale}
		latest := record(canonical.User, canonical.Text(0, "Current"))
		currentInput := uint64(3)
		latest.Message().Usage = &canonical.Usage{Input: &currentInput}
		if got, want := contextTokens([]canonical.Record{old, latest}), tokens.Chat("", []canonical.Record{old})+tokens.Chat("", []canonical.Record{latest}); got != want {
			t.Fatalf("partial latest usage reused an older total: %d, want %d", got, want)
		}
		request := func(id, name string) canonical.Record {
			return record(canonical.Assistant, canonical.Block{Kind: "tool_request", Data: &canonical.ToolRequest{ID: id, Name: name, Arguments: json.RawMessage(`{}`)}})
		}
		hidden := request("old", "alpha")
		hidden.Metadata.Runtime = &canonical.Runtime{}
		search := request("search", "tool_search")
		result := record(canonical.User, canonical.Block{Kind: "tool_response", Data: &canonical.ToolResponse{ID: "search", Status: canonical.ToolSuccess, Revision: strings.Repeat("c", 64), Result: canonical.Payload{Value: json.RawMessage(`{"tools":[{"name":"gamma"}],"sailry_context":""}`)}}})
		called := request("call", "beta")
		invalid := request("bad", "delta")
		invalid.Message().Content[0].Data.(*canonical.ToolRequest).Error = &canonical.ToolError{Message: "invalid"}
		state := store.AgentState{Records: []canonical.Record{hidden, search, result, called, invalid}, LoadedTools: []string{"alpha", "beta", "delta", "gamma"}}
		if got := retainedTools(state); !reflect.DeepEqual(got, []string{"beta", "gamma"}) {
			t.Fatalf("retained tools=%v", got)
		}
	})
}

// compactionFailed checks the stored failure record and returns the failure the
// conversation page and the statistics report for it.
func compactionFailed(t *testing.T, f *engineFixture, record canonical.Record) protocol.ConversationCompactionFailure {
	t.Helper()
	if record.Kind != "message" || record.Provenance.Author != "system" || record.Metadata.Compaction == nil || record.Metadata.Compaction.Failure == nil || record.Metadata.Runtime.AgentVisible || !record.Metadata.Runtime.UserVisible || len(record.Message().Content) != 0 {
		t.Fatalf("failure record = %+v", record)
	}
	var history protocol.ConversationHistory
	var statistics protocol.ConversationStatistics
	if err := f.db.Node.Read(f.ctx, func(db *sql.Conn) error {
		var err error
		if history, err = store.ReadConversation(f.ctx, db, f.session.ID, nil, 20); err != nil {
			return err
		}
		statistics, err = store.ReadAgentStatistics(f.ctx, db, f.session.ID)
		return err
	}); err != nil {
		t.Fatal(err)
	}
	var shown *protocol.ConversationCompactionFailure
	for _, entry := range history.Page.Entries {
		for _, part := range entry.Parts {
			var resource struct {
				Type    string                                  `json:"type"`
				Failure *protocol.ConversationCompactionFailure `json:"failure"`
			}
			if raw, ok := part.Data.(json.RawMessage); ok && part.Kind == "resource" && json.Unmarshal(raw, &resource) == nil && resource.Type == "compaction_failure" {
				if entry.ID != record.ID || shown != nil {
					t.Fatalf("failure part on entry %s", entry.ID)
				}
				shown = resource.Failure
			}
		}
	}
	if shown == nil || !reflect.DeepEqual(*shown, *record.Metadata.Compaction.Failure) {
		t.Fatalf("shown failure = %+v", shown)
	}
	last := statistics.LastCompaction
	if last == nil || last.ID != record.ID || last.Turn != record.Turn || last.Outcome.Kind != "failed" {
		t.Fatalf("last compaction = %+v", last)
	}
	return *shown
}

func compactionFixture(t *testing.T, m *scriptedModel) (*engineFixture, *execution, model.Binding, store.AgentState) {
	t.Helper()
	f := newFixture(t, m)
	f.submit("Original task")
	var invocation *store.AgentInvocation
	if err := f.db.Node.Write(f.ctx, func(db *sql.Conn) error {
		var err error
		invocation, _, err = store.ClaimNext(f.ctx, db, f.r.clock.Now())
		return err
	}); err != nil {
		t.Fatal(err)
	}
	if invocation == nil {
		t.Fatal("fixture turn was not claimed")
	}
	x := &execution{inv: invocation, ctx: f.ctx}
	if err := f.r.save(f.ctx, f.session.ID, inputRecord(invocation.Run.Turn, invocation.Message, f.r)); err != nil {
		t.Fatal(err)
	}
	binding, err := model.Capture(*invocation.Provider, invocation.Turn.Config, invocation.Secret, invocation.Session.ID)
	if err != nil {
		t.Fatal(err)
	}
	return f, x, binding, f.state()
}
