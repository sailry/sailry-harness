package engine

import (
	"context"
	"encoding/json"
	"fmt"
	"net"
	"net/http"
	"net/url"
	"os"
	"reflect"
	"slices"
	"sync/atomic"
	"testing"
	"time"

	"charm.land/fantasy"
	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/agent/model"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestHTTPRejectionRetriesCurrentStepWithoutReplayingTool(t *testing.T) {
	t.Parallel()
	rejected := response(fantasy.StreamPart{Type: fantasy.StreamPartTypeError, Error: &fantasy.ProviderError{
		StatusCode: http.StatusForbidden, ResponseBody: []byte(`{"error":{"code":"invalid_api_key","message":"Incorrect API key"}}`),
	}})
	m := &scriptedModel{scripts: []script{response(call("committed", "probe", `{}`), finish()), rejected, rejected}}
	var effects atomic.Int32
	probe := objectTool("probe", true, true, func(context.Context, canonical.ToolRequest) ToolResult {
		effects.Add(1)
		return ToolResult{Value: "Tool completed"}
	})
	f := newFixture(t, m, probe)
	f.start()
	turn := f.submit("Perform the operation once")
	select {
	case wait := <-f.r.clock.(*manualClock).waits:
		if wait.duration != 300*time.Millisecond || m.count() != 2 || effects.Load() != 1 {
			t.Fatalf("wrong retry boundary: wait=%s calls=%d tool executions=%d", wait.duration, m.count(), effects.Load())
		}
		close(wait.advance)
	case <-f.ctx.Done():
		t.Fatal("HTTP rejection did not reach its retry wait")
	}
	run := f.waitRun(turn, protocol.ConversationStatusFailed)
	const diagnostic = "Incorrect API key (HTTP 403)"
	if m.count() != 3 || effects.Load() != 1 || run.Error == nil || run.Error.Message != diagnostic || run.Error.Details == nil || run.Error.Details.Kind != "model_request" {
		t.Fatalf("retry lost its boundary or failure: calls=%d tool executions=%d fault=%+v", m.count(), effects.Load(), run.Error)
	}
	raw, err := json.Marshal(run.Error.Details.Data)
	if err != nil {
		t.Fatal(err)
	}
	var payload protocol.FaultDetailsModelRequestPayload
	if err := json.Unmarshal(raw, &payload); err != nil || payload.Reason != protocol.ModelFailureUnauthorized {
		t.Fatalf("retry lost the actionable authorization reason: %+v, %v", payload, err)
	}
	m.mu.Lock()
	sameCall := reflect.DeepEqual(m.calls[1], m.calls[2])
	m.mu.Unlock()
	if !sameCall {
		t.Fatal("HTTP resend changed the request containing the completed tool result")
	}
	var requests []string
	var retries int
	for _, record := range f.state().Records {
		execution := record.Metadata.Execution
		if execution == nil || execution.Observation == nil {
			continue
		}
		if retry := execution.Retry; retry != nil {
			retries++
			if retry.Attempt != 0 || retry.Limit == nil || *retry.Limit != 0 || retry.Error == nil || *retry.Error != diagnostic {
				t.Fatalf("wrong persisted first retry: %+v", retry)
			}
		}
		for _, attempt := range execution.Observation.Attempts {
			if attempt.Status != nil && *attempt.Status == http.StatusForbidden {
				requests = append(requests, execution.Observation.ID)
			}
		}
	}
	if retries != 1 || len(requests) != 2 || requests[0] == "" || requests[0] != requests[1] {
		t.Fatalf("persisted retries=%d request IDs=%v", retries, requests)
	}
}

func TestRetryExhaustionKeepsProviderCause(t *testing.T) {
	t.Parallel()
	for _, summary := range []bool{false, true} {
		name := "main"
		if summary {
			name = "summary"
		}
		t.Run(name, func(t *testing.T) {
			m := &scriptedModel{}
			if summary {
				m.scripts = append(m.scripts, response(fantasy.StreamPart{Type: fantasy.StreamPartTypeError, Error: &model.Failure{Kind: model.ContextOverflow, Message: "context full"}}))
			}
			for range model.MainRetries + 1 {
				m.scripts = append(m.scripts, response(fantasy.StreamPart{Type: fantasy.StreamPartTypeError, Error: &model.Failure{Kind: model.RateLimited, Message: "provider rate limited"}}))
			}
			f := newFixture(t, m)
			f.start()
			turn := f.submit("Task")
			interval := 300 * time.Millisecond
			for range model.MainRetries {
				select {
				case wait := <-f.r.clock.(*manualClock).waits:
					if wait.duration < interval*4/5 || wait.duration > min(interval*6/5, 30*time.Second) {
						t.Fatalf("retry wait = %s", wait.duration)
					}
					close(wait.advance)
					interval = min(interval*2, 30*time.Second)
				case <-f.ctx.Done():
					t.Fatal("the transient request did not reach its retry budget")
				}
			}
			run := f.waitRun(turn, protocol.ConversationStatusFailed)
			if run.Error == nil || run.Error.Message != "provider rate limited" || run.Error.Details == nil || run.Error.Details.Kind != "model_recovery" {
				t.Fatalf("the exhausted turn lost its cause or recovery outcome: %+v", run.Error)
			}
			raw, err := json.Marshal(run.Error.Details.Data)
			if err != nil {
				t.Fatal(err)
			}
			var recovery protocol.FaultDetailsModelRecoveryPayload
			if err := json.Unmarshal(raw, &recovery); err != nil || recovery.Reason != protocol.ConversationRecoveryStopAttemptsExhausted {
				t.Fatalf("invalid exhausted recovery: %+v, %v", recovery, err)
			}
			wantCalls := model.MainRetries + 1
			if summary {
				wantCalls++ // The initial main request requested compaction.
			}
			if m.count() != wantCalls {
				t.Fatalf("exhaustion sent %d requests, want %d", m.count(), wantCalls)
			}
			state := f.state()
			purpose := canonical.PurposeMain
			if summary {
				purpose = canonical.PurposeCompaction
			}
			var retries, observed uint64
			for _, record := range state.Records {
				execution := record.Metadata.Execution
				if execution == nil || execution.Purpose == nil || *execution.Purpose != purpose {
					continue
				}
				if retry := execution.Retry; retry != nil {
					if retry.Attempt != retries || retry.Limit == nil || *retry.Limit != model.MainRetries-1 {
						t.Fatalf("retry %d changed its displayed count or limit: %+v", retries, retry)
					}
					retries++
				}
				if execution.Observation != nil {
					for _, attempt := range execution.Observation.Attempts {
						observed++
						if attempt.Index != observed || attempt.Outcome != protocol.ConversationObservationOutcomeFailed {
							t.Fatalf("attempt %d changed its audit: %+v", observed, attempt)
						}
					}
				}
			}
			if retries != model.MainRetries || observed != model.MainRetries+1 {
				t.Fatalf("exhaustion stored %d retries and %d observations", retries, observed)
			}
			if !summary {
				last := lastMessage(state.Records)
				if last == nil || last.Error == nil || last.Error.Code == nil || *last.Error.Code != string(model.RateLimited) || last.Error.Message != "provider rate limited" || !last.Error.RetryExhausted {
					t.Fatalf("the canonical error lost the provider classification: %+v", last)
				}
				return
			}
			for _, record := range state.Records {
				if maintenance := record.Metadata.Compaction; maintenance != nil && maintenance.Failure != nil {
					failure := maintenance.Failure
					if failure.Reason != protocol.ConversationCompactionReasonRateLimited || failure.Cause == nil || *failure.Cause != "provider rate limited" {
						t.Fatalf("summary exhaustion lost the provider diagnosis: %+v", failure)
					}
					return
				}
			}
			t.Fatal("summary exhaustion has no compaction failure record")
		})
	}
}

func TestRetryWaitHintsKeepFirstRetryQuiet(t *testing.T) {
	t.Parallel()
	for _, summary := range []bool{false, true} {
		for _, row := range []struct {
			seconds int
			stop    bool
		}{{4, false}, {5, false}, {6, false}, {5, true}} {
			t.Run(fmt.Sprintf("summary_%t/wait_%ds/stop_%t", summary, row.seconds, row.stop), func(t *testing.T) {
				t.Parallel()
				busy := fantasy.StreamPart{Type: fantasy.StreamPartTypeError, Error: &fantasy.ProviderError{
					StatusCode: 503, Message: "provider is temporarily busy", ResponseHeaders: map[string]string{"Retry-After": fmt.Sprint(row.seconds)},
				}}
				m := &scriptedModel{}
				purpose := canonical.PurposeMain
				if summary {
					purpose = canonical.PurposeCompaction
					m.scripts = append(m.scripts, response(fantasy.StreamPart{Type: fantasy.StreamPartTypeError, Error: &model.Failure{Kind: model.ContextOverflow, Message: "context full"}}))
				}
				m.scripts = append(m.scripts, response(busy), response(busy))
				if summary {
					m.scripts = append(m.scripts, response(text(`{"user_intent":["Continue task"]}`), finish()))
				}
				m.scripts = append(m.scripts, response(text("Done"), finish()))
				f := newFixture(t, m)
				f.start()
				turn := f.submit("Task")
				records := func() (retries []protocol.ConversationRecoveryRetry, observations []protocol.ConversationObservationAttempt) {
					for _, record := range f.state().Records {
						execution := record.Metadata.Execution
						if execution == nil || execution.Purpose == nil || *execution.Purpose != purpose {
							continue
						}
						if execution.Retry != nil {
							if record.Metadata.Runtime == nil || !record.Metadata.Runtime.UserVisible || record.Metadata.Runtime.AgentVisible {
								t.Fatal("retry record has the wrong audience")
							}
							retries = append(retries, *execution.Retry)
						}
						if execution.Observation != nil {
							observations = append(observations, execution.Observation.Attempts...)
						}
					}
					return retries, observations
				}
				waits := 2
				if row.stop {
					waits = 1
				}
				for index := range waits {
					select {
					case wait := <-f.r.clock.(*manualClock).waits:
						if wait.duration != time.Duration(row.seconds)*time.Second {
							t.Fatalf("retry wait = %s, want %ds", wait.duration, row.seconds)
						}
						retries, observations := records()
						if len(retries) != index+1 || len(observations) != index+1 {
							t.Fatalf("retry wait %d has %d retries and %d observations", index, len(retries), len(observations))
						}
						retry := retries[index]
						if retry.Attempt != uint64(index) || retry.Limit == nil || *retry.Limit != model.MainRetries-1 || retry.DelayMs == nil || *retry.DelayMs != uint64(row.seconds*1000) || retry.Status == nil || *retry.Status != 503 || retry.Code == nil || *retry.Code != string(model.Unavailable) {
							t.Fatalf("retry wait %d changed its visibility, budget or cause: %+v", index, retry)
						}
						if observations[index].Index != uint64(index+1) || observations[index].Outcome != protocol.ConversationObservationOutcomeFailed {
							t.Fatalf("retry wait %d changed its audit: %+v", index, observations[index])
						}
						if !row.stop {
							close(wait.advance)
						}
					case <-f.ctx.Done():
						t.Fatal("the request did not reach its hinted retry wait")
					}
				}
				status, wantCalls, wantObservations := protocol.ConversationStatusCompleted, 3, 3
				if row.stop {
					f.command("stop_turn", &protocol.CommandStopTurnPayload{Turn: turn})
					status, wantCalls, wantObservations = protocol.ConversationStatusCancelled, 1, 1
				}
				if summary {
					wantCalls++
					if !row.stop {
						wantCalls++
					}
				}
				f.waitRun(turn, status)
				retries, observations := records()
				if m.count() != wantCalls || len(retries) != waits || len(observations) != wantObservations {
					t.Fatalf("calls=%d retries=%d observations=%d; want %d, %d, %d", m.count(), len(retries), len(observations), wantCalls, waits, wantObservations)
				}
			})
		}
	}
}

func TestTCPDialTimeoutPersistsRetryLimitAndTerminalAttempt(t *testing.T) {
	t.Parallel()
	dial := &net.OpError{Op: "dial", Net: "tcp", Addr: &net.TCPAddr{Port: 443}, Err: os.ErrDeadlineExceeded}
	cause := &url.Error{Op: "Post", URL: "https://fixture.invalid", Err: dial}
	part := fantasy.StreamPart{Type: fantasy.StreamPartTypeError, Error: &fantasy.ProviderError{Message: "model stream failed", Cause: cause}}
	diagnostic := "model stream failed: " + dial.Error()
	m := &scriptedModel{scripts: []script{response(part), response(part), response(part)}}
	f := newFixture(t, m)
	f.start()
	turn := f.submit("Task")
	for range 2 {
		select {
		case wait := <-f.r.clock.(*manualClock).waits:
			close(wait.advance)
		case <-f.ctx.Done():
			t.Fatal("the dial timeout did not schedule its retry")
		}
	}
	run := f.waitRun(turn, protocol.ConversationStatusFailed)
	if m.count() != 3 || run.Error == nil || run.Error.Message != diagnostic || run.Error.Details == nil || run.Error.Details.Kind != "model_recovery" {
		t.Fatalf("the turn lost its dial timeout: calls=%d, fault=%+v", m.count(), run.Error)
	}
	raw, err := json.Marshal(run.Error.Details.Data)
	if err != nil {
		t.Fatal(err)
	}
	var recovery protocol.FaultDetailsModelRecoveryPayload
	if err := json.Unmarshal(raw, &recovery); err != nil || recovery.Reason != protocol.ConversationRecoveryStopAttemptsExhausted {
		t.Fatalf("wrong recovery reason: %+v, %v", recovery, err)
	}
	state := f.state()
	last := lastMessage(state.Records)
	if last == nil || last.Error == nil || last.Error.Code == nil || *last.Error.Code != string(model.Network) || !last.Error.RetryExhausted || last.Error.RetryWaitExhausted {
		t.Fatalf("wrong canonical error: %+v", last)
	}
	var retries, attempts []uint64
	for _, record := range state.Records {
		execution := record.Metadata.Execution
		if execution == nil || execution.Observation == nil {
			continue
		}
		if retry := execution.Retry; retry != nil {
			if retry.Limit == nil || *retry.Limit != 1 || retry.DelayMs == nil || retry.Error == nil || *retry.Error != diagnostic {
				t.Fatalf("wrong canonical retry: %+v", retry)
			}
			retries = append(retries, retry.Attempt)
		}
		for _, attempt := range execution.Observation.Attempts {
			if attempt.Outcome != protocol.ConversationObservationOutcomeFailed {
				t.Fatalf("wrong canonical attempt: %+v", attempt)
			}
			attempts = append(attempts, attempt.Index)
		}
	}
	if !slices.Equal(retries, []uint64{0, 1}) || !slices.Equal(attempts, []uint64{1, 2, 3}) {
		t.Fatalf("stored retries=%v, observations=%v", retries, attempts)
	}
}
