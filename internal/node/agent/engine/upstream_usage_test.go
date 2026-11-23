package engine

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"fmt"
	"reflect"
	"strings"
	"sync/atomic"
	"testing"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/agent/model"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
	"golang.org/x/net/http2"
)

func TestUpstreamUsageMissingRecoveryDoesNotReplayTools(t *testing.T) {
	t.Parallel()
	const diagnostic = `error: code=502 reason="UPSTREAM_USAGE_MISSING" message="Upstream response is missing billable token usage; settlement was not completed" metadata=map[]`
	raw, err := json.Marshal(map[string]any{"error": map[string]any{"type": "api_error", "message": diagnostic}})
	if err != nil {
		t.Fatal(err)
	}
	checkModelFailureRecovery(t, &fantasy.ProviderError{StatusCode: 200, ResponseBody: raw}, model.Unavailable, diagnostic, model.MainRetries)
}

func TestHTTP2ProtocolRecoveryDoesNotReplayTools(t *testing.T) {
	t.Parallel()
	reset := http2.StreamError{StreamID: 1, Code: http2.ErrCodeProtocol, Cause: errors.New("received from peer")}
	checkModelFailureRecovery(t, &fantasy.ProviderError{Message: "model stream failed", StatusCode: 200, Cause: reset}, model.Network, "model stream failed: "+reset.Error(), model.MainRetries)
}

func TestHTTP524RecoveryDoesNotReplayTools(t *testing.T) {
	t.Parallel()
	const diagnostic = "origin timed out (HTTP 524)"
	checkModelFailureRecovery(t, &fantasy.ProviderError{StatusCode: 524, Message: diagnostic}, model.Unavailable, diagnostic, 1)
}

func TestAttemptPricesAreFrozenAndCountedOnce(t *testing.T) {
	t.Parallel()
	billed := func(part fantasy.StreamPart, input, output int64) fantasy.StreamPart {
		part.ProviderMetadata = fantasy.ProviderMetadata{model.UsageMetadataKey: &model.UsageMetadata{Reported: true, Input: &input, Output: &output}}
		return part
	}
	for _, outcome := range []string{"success", "failure", "cancelled"} {
		t.Run(outcome, func(t *testing.T) {
			transient := fantasy.StreamPart{Type: fantasy.StreamPartTypeError, Error: &model.Failure{Kind: model.RateLimited, Message: "Retry later"}}
			terminal := fantasy.StreamPart{Type: fantasy.StreamPartTypeError, Error: &model.Failure{Kind: model.InvalidResponse, Message: "Incomplete response"}}
			last, status := response(billed(terminal, 20, 5)), protocol.ConversationStatusFailed
			reported := make(chan struct{})
			switch outcome {
			case "success":
				last, status = response(text("Final answer"), billed(finish(), 20, 5)), protocol.ConversationStatusCompleted
			case "cancelled":
				status = protocol.ConversationStatusCancelled
				last = func(ctx context.Context, _ fantasy.Call, yield func(fantasy.StreamPart) bool) {
					if yield(billed(text("Partial answer"), 20, 5)) {
						close(reported)
						<-ctx.Done()
					}
				}
			}
			m := &scriptedModel{scripts: []script{response(billed(transient, 12, 4)), response(transient), last}}
			f := newFixture(t, m)
			if err := f.db.Node.Write(f.ctx, func(db *sql.Conn) error {
				if _, err := db.ExecContext(f.ctx, `UPDATE providers SET body=json_set(body,'$.provider.billing_rate_bps',5000) WHERE id=?1`, f.session.Config.Provider.String()); err != nil {
					return err
				}
				_, err := db.ExecContext(f.ctx, "INSERT INTO model_catalog(provider,id,body) VALUES('openai','fixture',?1)", []byte(`{"cost":{"input":2,"output":8}}`))
				return err
			}); err != nil {
				t.Fatal(err)
			}
			f.start()
			turn := f.submit("Task")
			for i := range 2 {
				select {
				case wait := <-f.r.clock.(*manualClock).waits:
					if i == 0 {
						if err := f.db.Node.Write(f.ctx, func(db *sql.Conn) error {
							if _, err := db.ExecContext(f.ctx, `UPDATE providers SET body=json_set(body,'$.provider.billing_rate_bps',0) WHERE id=?1`, f.session.Config.Provider.String()); err != nil {
								return err
							}
							_, err := db.ExecContext(f.ctx, "UPDATE model_catalog SET body=?1 WHERE provider='openai' AND id='fixture'", []byte(`{"cost":{"input":4,"output":16}}`))
							return err
						}); err != nil {
							t.Fatal(err)
						}
					}
					close(wait.advance)
				case <-f.ctx.Done():
					t.Fatal("the scripted failure did not enter recovery")
				}
			}
			if outcome == "cancelled" {
				select {
				case <-reported:
					f.command("stop_turn", &protocol.CommandStopTurnPayload{Turn: turn})
				case <-f.ctx.Done():
					t.Fatal("the stopped attempt did not report usage")
				}
			}
			f.waitRun(turn, status)
			var priced []uint64
			for _, record := range f.state().Records {
				message := record.Message()
				if message == nil || message.Usage == nil || message.Usage.Input == nil {
					continue
				}
				pricing := message.Usage.Pricing
				execution := record.Metadata.Execution
				if pricing == nil || pricing.Estimate == nil || execution == nil || execution.Source == nil || execution.Source.ID != f.session.Config.Provider {
					t.Fatalf("billed attempt lost its price or execution source: %+v", message.Usage)
				}
				priced = append(priced, pricing.Estimate.UsdMicros)
			}
			if !reflect.DeepEqual(priced, []uint64{28, 80}) {
				t.Fatalf("stored attempt prices = %v, want [28 80]", priced)
			}
			if err := f.db.Node.Read(f.ctx, func(db *sql.Conn) error {
				statistics, err := store.ReadAgentStatistics(f.ctx, db, f.session.ID)
				if err == nil && (statistics.Responses != 2 || statistics.Cost == nil || statistics.Cost.Responses != 2 || statistics.Cost.UsdMicros != 108) {
					return fmt.Errorf("attempt statistics counted missing or duplicate usage: %+v", statistics)
				}
				return err
			}); err != nil {
				t.Fatal(err)
			}
		})
	}
}

func checkModelFailureRecovery(t *testing.T, failure *fantasy.ProviderError, kind model.FailureKind, diagnostic string, retryLimit int) {
	t.Helper()
	for _, recovers := range []bool{true, false} {
		t.Run(fmt.Sprintf("recovers_%t", recovers), func(t *testing.T) {
			var executed atomic.Int32
			probe := objectTool("probe", true, true, func(context.Context, canonical.ToolRequest) ToolResult {
				executed.Add(1)
				return ToolResult{Value: map[string]bool{"ok": true}}
			})
			m := &scriptedModel{scripts: []script{response(call("committed", "probe", `{}`), finish())}}
			failures := retryLimit + 1
			if recovers {
				failures = 1
			}
			for range failures {
				m.scripts = append(m.scripts, response(text("Discarded attempt"), call("discarded", "probe", `{}`), fantasy.StreamPart{
					Type:  fantasy.StreamPartTypeError,
					Error: failure,
				}))
			}
			if recovers {
				m.scripts = append(m.scripts, response(text("Final answer"), finish()))
			}
			f := newFixture(t, m, probe)
			f.start()
			turn := f.submit("Task")
			waits, status := retryLimit, protocol.ConversationStatusFailed
			if recovers {
				waits, status = 1, protocol.ConversationStatusCompleted
			}
			for range waits {
				select {
				case wait := <-f.r.clock.(*manualClock).waits:
					if executed.Load() != 1 {
						t.Fatalf("failed attempt executed tools or replayed the completed one: %d", executed.Load())
					}
					close(wait.advance)
				case <-f.ctx.Done():
					t.Fatal("the model transport failure did not enter bounded recovery")
				}
			}
			run := f.waitRun(turn, status)
			if executed.Load() != 1 || m.count() != 2+waits {
				t.Fatalf("recovery repeated a completed step: tools=%d requests=%d", executed.Load(), m.count())
			}
			if !recovers && (run.Error == nil || run.Error.Message != diagnostic || run.Error.Details == nil || run.Error.Details.Kind != "model_recovery") {
				t.Fatalf("unresolved model failure was marked successful or lost its cause: %+v", run.Error)
			}
			m.mu.Lock()
			for _, request := range m.calls[2:] {
				if !reflect.DeepEqual(request.Prompt, m.calls[1].Prompt) {
					t.Error("recovery changed the request's completed tool history")
				}
			}
			m.mu.Unlock()
			requests, responses, retries, terminalErrors := 0, 0, 0, 0
			state := f.state()
			for _, record := range state.Records {
				message := record.Message()
				if message == nil {
					continue
				}
				if execution := record.Metadata.Execution; execution != nil && execution.Retry != nil {
					retry := execution.Retry
					if retry.Attempt != uint64(retries) || retry.Limit == nil || *retry.Limit != uint64(retryLimit-1) {
						t.Fatalf("wrong canonical retry number or limit: %+v", retry)
					}
					retries++
					if retry.Status == nil || int(*retry.Status) != failure.StatusCode || retry.Code == nil || *retry.Code != string(kind) || retry.Error == nil || *retry.Error != diagnostic {
						t.Fatalf("retry lost the provider error or actual HTTP status: %+v", retry)
					}
					if message.Usage != nil && !reflect.DeepEqual(*message.Usage, canonical.Usage{}) {
						t.Fatalf("settlement recovery invented usage: %+v", message.Usage)
					}
					if execution.Observation == nil {
						t.Fatal("recovery lost its failed attempt observation")
					}
					attempts := execution.Observation.Attempts
					if len(attempts) != 1 || attempts[0].Outcome != protocol.ConversationObservationOutcomeFailed || attempts[0].Usage != nil {
						t.Fatalf("the failed attempt became a success or known usage: %+v", attempts)
					}
				}
				if message.Error != nil {
					terminalErrors++
					if recovers || message.Error.Code == nil || *message.Error.Code != string(kind) || !message.Error.RetryExhausted || message.Error.Message != diagnostic {
						t.Fatalf("terminal upstream failure was hidden: %+v", message.Error)
					}
				}
				for _, block := range message.Content {
					switch value := block.Data.(type) {
					case *canonical.ToolRequest:
						requests++
						if value.ID != "committed" {
							t.Fatal("an unsuccessful stream persisted a tool request")
						}
					case *canonical.ToolResponse:
						responses++
					case *canonical.TextContent:
						if recovers && strings.Contains(value.Text, "Discarded") {
							t.Fatal("recovery kept discarded provisional text")
						}
					}
				}
			}
			wantErrors := 1
			if recovers {
				wantErrors = 0
				mustHaveText(t, state.Records, "Final answer")
			}
			if requests != 1 || responses != 1 || retries != waits || terminalErrors != wantErrors {
				t.Fatalf("canonical recovery changed effects or terminal status: requests=%d responses=%d retries=%d errors=%d", requests, responses, retries, terminalErrors)
			}
		})
	}
}
