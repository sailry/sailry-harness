package engine

import (
	"context"
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
	"github.com/sailry/sailry-harness/internal/protocol"
	"golang.org/x/net/http2"
)

func TestUpstreamUsageMissingRecoveryDoesNotReplayTools(t *testing.T) {
	const diagnostic = `error: code=502 reason="UPSTREAM_USAGE_MISSING" message="Upstream response is missing billable token usage; settlement was not completed" metadata=map[]`
	raw, err := json.Marshal(map[string]any{"error": map[string]any{"type": "api_error", "message": diagnostic}})
	if err != nil {
		t.Fatal(err)
	}
	checkModelFailureRecovery(t, &fantasy.ProviderError{StatusCode: 200, ResponseBody: raw}, model.Unavailable, diagnostic+" (HTTP 200)")
}

func TestHTTP2ProtocolRecoveryDoesNotReplayTools(t *testing.T) {
	reset := http2.StreamError{StreamID: 1, Code: http2.ErrCodeProtocol, Cause: errors.New("received from peer")}
	checkModelFailureRecovery(t, &fantasy.ProviderError{Message: "model stream failed", StatusCode: 200, Cause: reset}, model.Network, "model stream failed: "+reset.Error())
}

func checkModelFailureRecovery(t *testing.T, failure error, kind model.FailureKind, diagnostic string) {
	t.Helper()
	for _, recovers := range []bool{true, false} {
		t.Run(fmt.Sprintf("recovers_%t", recovers), func(t *testing.T) {
			var executed atomic.Int32
			probe := objectTool("probe", true, true, func(context.Context, canonical.ToolRequest) ToolResult {
				executed.Add(1)
				return ToolResult{Value: map[string]bool{"ok": true}}
			})
			m := &scriptedModel{scripts: []script{response(call("committed", "probe", `{}`), finish())}}
			failures := model.MainRetries + 1
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
			waits, status := model.MainRetries, protocol.ConversationStatusFailed
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
					retries++
					if retry := execution.Retry; retry.Status == nil || *retry.Status != 200 || retry.Code == nil || *retry.Code != string(kind) || retry.Error == nil || *retry.Error != diagnostic {
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
