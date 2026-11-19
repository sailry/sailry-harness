package client

import (
	"encoding/json"
	"fmt"
	"strings"
	"sync/atomic"
	"testing"
	"testing/synctest"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestReadCommandTranscript(t *testing.T) {
	t.Parallel()
	session, turn, command := protocol.SessionID{1}, protocol.TurnID{2}, protocol.RequestID{3}
	first, last := strings.Repeat("previous\n", 120)+"partial", " line\n"+strings.Repeat("last\n", 98)+"end\n\n"
	badTurn := false
	transport := &clientTransport{dispatch: func(request protocol.Request) (link.Admission, error) {
		q, err := payload[protocol.CommandReadCommandPagePayload](request.Command.Data)
		if err != nil || request.Command.Kind != "read_command_page" || q.Session != session || q.ID != command || q.WaitMs != 0 || q.Limit != 32*1024 {
			t.Fatalf("unexpected retained output request: %+v, %v", q, err)
		}
		page := protocol.DefaultProcessOutputOutputPage()
		page.Info.Session, page.Info.Turn, page.Info.ID = session, turn, command
		if badTurn {
			page.Info.Turn = protocol.TurnID{9}
		}
		page.Stream, page.Offset, page.TotalBytes = q.Stream, q.Offset, 10000
		switch {
		case q.Stream == protocol.ProcessOutputOutputStreamStderr && q.Offset == 0:
			page.Text = "stderr"
		case q.Stream == protocol.ProcessOutputOutputStreamStdout && q.Offset == 0:
			page.Text, page.NextOffset = first, new(uint64(8192))
		case q.Stream == protocol.ProcessOutputOutputStreamStdout && q.Offset == 8192:
			page.Text = last
		default:
			t.Fatalf("unexpected output cursor: %+v", q)
		}
		completion := make(chan link.Response, 1)
		completion <- link.Response{Output: protocol.Output{Kind: "command_output_page", Data: page}}
		return link.Admission{Receipt: protocol.Receipt{ID: request.ID}, Completion: completion}, nil
	}}
	c := New(transport)
	t.Cleanup(func() { _ = c.Close() })
	for _, tc := range []struct {
		lines int
		want  string
	}{{100, strings.Repeat("last\n", 98) + "end\n\nstderr"}, {0, first + last + "stderr"}} {
		got, err := c.ReadCommandTranscript(t.Context(), session, turn, command, tc.lines)
		if err != nil || got != tc.want {
			t.Fatalf("last %d lines: got %q, want %q, error %v", tc.lines, got, tc.want, err)
		}
	}
	badTurn = true
	if got, err := c.ReadCommandTranscript(t.Context(), session, turn, command, 0); err == nil || got != "" {
		t.Fatalf("an unrelated turn exposed command output: %q, %v", got, err)
	}
}

func TestCommandOutputOwnerRecovery(t *testing.T) {
	t.Parallel()
	synctest.Test(t, func(t *testing.T) {
		session, command := protocol.NewSessionID(), protocol.NewRequestID()
		var reads atomic.Int32
		transport := &clientTransport{dispatch: func(request protocol.Request) (link.Admission, error) {
			if reads.Add(1) == 1 {
				return link.Admission{}, fault(protocol.ErrorCodeUnavailable, "fixture command read failed")
			}
			snapshot := protocol.DefaultProcessSnapshot()
			snapshot.Info.ID, snapshot.Info.Session, snapshot.Info.Status.Kind = command, session, "exited"
			snapshot.Stdout.Text = "ready"
			completion := make(chan link.Response, 1)
			completion <- link.Response{Output: protocol.Output{Kind: "command_output", Data: snapshot}}
			return link.Admission{Receipt: protocol.Receipt{ID: request.ID}, Completion: completion}, nil
		}}
		controller := New(transport)
		defer controller.Close()
		type observation struct {
			release func()
			err     error
		}
		replacement := make(chan observation, 1)
		recovered := make(chan struct{}, 1)
		first, err := controller.ObserveCommandOutput(t.Context(), session, command, func() {
			if state, ok := controller.CommandOutput(session, command); !ok || state.Err == nil {
				t.Error("the failed command read was not published to its observer")
			}
			// Re-observe from the failure callback, before the failed owner
			// returns. A deferred cancellation alone is too late.
			release, err := controller.ObserveCommandOutput(t.Context(), session, command, func() { recovered <- struct{}{} })
			replacement <- observation{release, err}
		})
		if err != nil {
			t.Fatal(err)
		}
		defer first()
		synctest.Wait()
		var second observation
		select {
		case second = <-replacement:
		default:
			t.Fatal("the failed command read did not notify its observer")
		}
		if second.err != nil {
			t.Fatal(second.err)
		}
		defer second.release()
		select {
		case <-recovered:
		default:
			t.Fatal("the new observer joined a command output owner that had ended")
		}
		first()
		state, ok := controller.CommandOutput(session, command)
		if reads.Load() != 2 || !ok || !state.Known || state.Err != nil || state.Snapshot.Stdout.Text != "ready" {
			t.Fatalf("the old observer lost the replacement output: reads %d, state %+v, retained %v", reads.Load(), state, ok)
		}
		second.release()
		if _, ok := controller.CommandOutput(session, command); ok {
			t.Fatal("unobserved replacement command output was retained")
		}
	})
}

func TestHistoricalCommandOutsideActivityWindow(t *testing.T) {
	t.Parallel()
	page := protocol.DefaultConversationPage()
	page.Session = protocol.NewSessionID()
	turn, origin := protocol.NewTurnID(), protocol.NewSessionID()
	page.Runs = []protocol.ConversationRun{{Session: page.Session, Turn: turn, Origin: &origin}}
	for i := 1; i <= 21; i++ {
		id := fmt.Sprintf("call-%d", i)
		command := protocol.RequestID{byte(i)}
		task := "command:" + command.String()
		page.Entries = append(page.Entries,
			protocol.ConversationEntry{ID: id, Turn: turn, Author: "assistant", Parts: []protocol.ConversationPart{{Kind: "tool_call", Data: protocol.ConversationPartToolCallPayload{
				ID: &id, Name: "run_command", Presentation: "summary",
			}}}},
			protocol.ConversationEntry{ID: "result-" + id, Turn: turn, Author: "tool", Parts: []protocol.ConversationPart{{Kind: "tool_result", Data: protocol.ConversationPartToolResultPayload{
				ID: &id, Name: "run_command", Summary: &protocol.ToolDetailSummary{Revision: "launch-" + id, Task: &task},
			}}}},
		)
		if i > 1 {
			page.Background = append(page.Background, protocol.ConversationBackgroundTask{ID: task, Session: page.Session, Turn: turn, State: "consumed"})
		}
	}
	calls := ToolCalls(page)
	if len(calls) != 21 || len(page.Background) != 20 {
		t.Fatalf("history gained rows or changed the activity window: %d calls, %d tasks", len(calls), len(page.Background))
	}
	for i, call := range calls {
		reference := projectToolCall(page, call)["command_detail"]
		if i != 0 {
			if reference != nil {
				t.Errorf("recent row %s was given another detail reader", call.Source.Entry)
			}
			continue
		}
		ref, ok := reference.(*HistoricalCommandReference)
		if !ok || call.Source.Entry != "call-1" || ref.Detail.Entry != "result-call-1" || ref.Detail.Turn != turn ||
			ref.Detail.Session != page.Session || ref.Session != origin || ref.Command != (protocol.RequestID{1}) {
			t.Fatalf("the old result lost its canonical row or captured identity: %+v, %+v", call, ref)
		}
		call.Name = "wait_agent"
		if got := projectToolCall(page, call)["command_detail"]; got != nil {
			t.Fatal("wait_agent acquired a process detail")
		}
		call.Name = "run_command"
		call.Approval = &protocol.ConversationApproval{State: "denied"}
		if got := projectToolCall(page, call)["command_detail"]; got != nil {
			t.Fatal("an unapproved row acquired a process detail")
		}
	}
}

func TestReadHistoricalCommandBinding(t *testing.T) {
	t.Parallel()
	ref := HistoricalCommandReference{Detail: toolDetailReferenceFixture(), Command: protocol.RequestID{3}, Session: protocol.SessionID{4}}
	for _, tc := range []struct {
		name       string
		change     func(map[string]any)
		result     func(*protocol.ProcessSnapshot)
		readError  protocol.ErrorCode
		wantError  protocol.ErrorCode
		wantReads  int
		wantStatus protocol.ConversationBackgroundOutcome
		wantExit   *int32
	}{
		{name: "legacy with both IDs", change: func(data map[string]any) { delete(data, "turn") }, wantReads: 1, wantStatus: "failed", wantExit: new(int32(7))},
		{name: "legacy without session", change: func(data map[string]any) { delete(data, "session") }, wantError: protocol.ErrorCodeOutcomeUnknown},
		{name: "wrong launch session", change: func(data map[string]any) { data["session"] = ref.Detail.Session }, wantError: protocol.ErrorCodeOutcomeUnknown},
		{name: "wrong launch command", change: func(data map[string]any) { data["id"] = protocol.RequestID{9} }, wantError: protocol.ErrorCodeOutcomeUnknown},
		{name: "wrong launch turn", change: func(data map[string]any) { data["turn"] = protocol.TurnID{9} }, wantError: protocol.ErrorCodeOutcomeUnknown},
		{name: "wrong returned session", result: func(out *protocol.ProcessSnapshot) { out.Info.Session = ref.Detail.Session }, wantReads: 1, wantError: protocol.ErrorCodeInvalidRequest},
		{name: "wrong returned command", result: func(out *protocol.ProcessSnapshot) { out.Info.ID = protocol.RequestID{9} }, wantReads: 1, wantError: protocol.ErrorCodeInvalidRequest},
		{name: "wrong returned turn", result: func(out *protocol.ProcessSnapshot) { out.Info.Turn = protocol.TurnID{9} }, wantReads: 1, wantError: protocol.ErrorCodeInvalidRequest},
		{name: "not retained", readError: protocol.ErrorCodeNotFound, wantReads: 1, wantError: protocol.ErrorCodeNotFound},
		{name: "not authorized", readError: protocol.ErrorCodePermissionDenied, wantReads: 1, wantError: protocol.ErrorCodePermissionDenied},
		{name: "uncertain completion", result: func(out *protocol.ProcessSnapshot) {
			out.Info.Status.Data = protocol.ProcessOutcome{Kind: "unknown", Data: "uncertain"}
		}, wantReads: 1, wantStatus: "unknown"},
		{name: "actual success", result: func(out *protocol.ProcessSnapshot) {
			out.Info.Status.Data = protocol.ProcessOutcome{Kind: "exited", Data: int32(0)}
		}, wantReads: 1, wantStatus: "completed", wantExit: new(int32(0))},
	} {
		t.Run(tc.name, func(t *testing.T) {
			data := map[string]any{"id": ref.Command, "session": ref.Session, "turn": ref.Detail.Turn}
			if tc.change != nil {
				tc.change(data)
			}
			raw, err := json.Marshal(map[string]any{"kind": "command_started", "data": data})
			if err != nil {
				t.Fatal(err)
			}
			detail := protocol.DefaultToolDetail()
			detail.Result = new(json.RawMessage(raw))
			body, err := json.Marshal(detail)
			if err != nil {
				t.Fatal(err)
			}
			details, reads := 0, 0
			transport := toolDetailTransportFixture(t, ref.Detail, func(protocol.CommandReadToolDetailPayload) protocol.ToolDetailPage {
				details++
				return protocol.ToolDetailPage{Reference: ref.Detail, Text: string(body), TotalBytes: uint64(len(body))}
			})
			readDetail := transport.dispatch
			transport.dispatch = func(request protocol.Request) (link.Admission, error) {
				if request.Command.Kind != "read_command" {
					return readDetail(request)
				}
				reads++
				q, err := payload[protocol.CommandReadCommandPayload](request.Command.Data)
				if err != nil || q.ID != ref.Command || q.Session != ref.Session || q.WaitMs != 0 || request.Command.Durable() {
					t.Fatalf("read did not use the original pair once: %+v, %v", q, err)
				}
				snapshot := protocol.DefaultProcessSnapshot()
				snapshot.Info.ID, snapshot.Info.Session, snapshot.Info.Turn = q.ID, q.Session, ref.Detail.Turn
				snapshot.Info.Status = protocol.ProcessStatus{Kind: "finished", Data: &protocol.ProcessOutcome{Kind: "exited", Data: new(int32(7))}}
				snapshot.Stdout.Text, snapshot.Stderr.Text = "retained output", "retained error"
				if tc.result != nil {
					tc.result(&snapshot)
				}
				response := link.Response{Output: protocol.Output{Kind: "command_output", Data: snapshot}}
				if tc.readError != "" {
					response = link.Response{Fault: &protocol.Fault{Code: tc.readError, Message: "fixture command fault"}}
				}
				completion := make(chan link.Response, 1)
				completion <- response
				return link.Admission{Receipt: protocol.Receipt{ID: request.ID}, Completion: completion}, nil
			}
			controller := New(transport)
			defer controller.Close()
			got, err := controller.ReadHistoricalCommand(t.Context(), ref)
			if details != 1 || reads != tc.wantReads {
				t.Fatalf("request counts = %d tool details, %d process reads", details, reads)
			}
			if tc.wantError != "" {
				if faultCode(err) != tc.wantError || got.ExitCode != nil || got.Stdout.Text != "" || got.Outcome != "" {
					t.Fatalf("invalid or unavailable read became a result: %+v, %v", got, err)
				}
				return
			}
			if err != nil || got.Outcome != tc.wantStatus || (got.ExitCode == nil) != (tc.wantExit == nil) || got.ExitCode != nil && *got.ExitCode != *tc.wantExit ||
				got.Stdout.Text != "retained output" || got.Stderr.Text != "retained error" {
				t.Fatalf("read lost retained command evidence: %+v, %v", got, err)
			}
			if _, ok := controller.CommandOutput(ref.Session, ref.Command); ok {
				t.Fatal("one historical read left an activity watch alive")
			}
		})
	}
}
