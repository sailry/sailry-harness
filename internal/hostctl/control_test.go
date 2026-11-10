package hostctl

import (
	"bufio"
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"io"
	"strings"
	"sync"
	"testing"

	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type fixtureTransport struct {
	dispatch  func(context.Context, protocol.Request) (link.Admission, error)
	subscribe func(context.Context, protocol.Topic) (link.Subscription, error)
}

func (*fixtureTransport) Target() protocol.NodeID { return protocol.NodeID{1} }
func (*fixtureTransport) Open(context.Context, protocol.StreamID) (link.Stream, error) {
	return nil, errors.New("streams are unavailable")
}
func (transport *fixtureTransport) Dispatch(ctx context.Context, request protocol.Request) (link.Admission, error) {
	return transport.dispatch(ctx, request)
}
func (transport *fixtureTransport) Subscribe(ctx context.Context, topic protocol.Topic) (link.Subscription, error) {
	if transport.subscribe == nil {
		return nil, errors.New("subscriptions are unavailable")
	}
	return transport.subscribe(ctx, topic)
}
func (*fixtureTransport) WatchConnection(ctx context.Context, changed func(link.ConnectionInfo)) error {
	route := link.RouteLocal
	changed(link.ConnectionInfo{Route: &route})
	<-ctx.Done()
	return ctx.Err()
}

func fixtureAdmission(request protocol.Request, output protocol.Output) link.Admission {
	completed := make(chan link.Response, 1)
	completed <- link.Response{Output: output}
	close(completed)
	return link.Admission{Receipt: protocol.Receipt{ID: request.ID, Durable: request.Command.Durable()}, Completion: completed}
}

func TestHostControlContract(t *testing.T) {
	t.Parallel()
	transport := &fixtureTransport{dispatch: func(_ context.Context, request protocol.Request) (link.Admission, error) {
		return fixtureAdmission(request, protocol.Output{Kind: "power_settings", Data: protocol.DefaultPowerSettings()}), nil
	}}
	request := protocol.Request{Version: protocol.VERSION, ID: protocol.RequestID{2}, Target: transport.Target(), Command: protocol.Command{Kind: "read_power_settings"}}
	frame, err := json.Marshal(requestFrame{Command: "node_request", Version: 1, Request: request})
	if err != nil {
		t.Fatal(err)
	}
	for _, row := range []struct {
		name  string
		input string
		kinds []string
	}{
		{"hello", "{\"command\":\"node_hello\",\"version\":1}\n", []string{"hello"}},
		{"wrong_version", "{\"command\":\"node_hello\",\"version\":2}\n", []string{"failed"}},
		{"unknown_field", "{\"command\":\"node_hello\",\"version\":1,\"ignored\":true}\n", []string{"failed"}},
		{"request", string(frame) + "\n", []string{"admitted", "completed"}},
	} {
		t.Run(row.name, func(t *testing.T) {
			var output bytes.Buffer
			if err := serveTransport(t.Context(), []byte(row.input), &output, transport); err != nil {
				t.Fatal(err)
			}
			reader := bufio.NewReader(&output)
			for _, expected := range row.kinds {
				line, err := readLine(reader, protocol.MAX_FRAME_BYTES)
				if err != nil {
					t.Fatal(err)
				}
				var response struct {
					Version uint8           `json:"version"`
					Node    protocol.NodeID `json:"node"`
					Body    struct {
						Kind string          `json:"kind"`
						Data json.RawMessage `json:"data"`
					} `json:"body"`
				}
				if err := json.Unmarshal(line, &response); err != nil {
					t.Fatal(err)
				}
				if response.Version != 1 || response.Node != transport.Target() || response.Body.Kind != expected {
					t.Fatalf("unexpected control response: %s", line)
				}
				if expected == "hello" && len(response.Body.Data) != 0 {
					t.Fatal("unit replies must omit data")
				}
				if expected == "admitted" {
					var receipt protocol.Receipt
					if json.Unmarshal(response.Body.Data, &receipt) != nil || receipt.ID != request.ID {
						t.Fatal("admission did not retain the real receipt")
					}
				}
				if expected == "completed" {
					var completed protocol.Result[protocol.Output, protocol.Fault]
					if json.Unmarshal(response.Body.Data, &completed) != nil || completed.Ok == nil || completed.Ok.Kind != "power_settings" {
						t.Fatal("completion lost the Rust-compatible result envelope")
					}
				}
			}
			if _, err := reader.ReadByte(); !errors.Is(err, io.EOF) {
				t.Fatal("unexpected extra control result")
			}
		})
	}
	for _, line := range []string{"{}", "{}\n", "{\"x\":\"" + strings.Repeat("x", 64) + "\"}\n"} {
		_, err := readLine(bufio.NewReader(strings.NewReader(line)), 32)
		valid := line == "{}\n"
		if (err == nil) != valid {
			t.Fatalf("NDJSON limit/newline contract: line=%q err=%v", line, err)
		}
	}
}

type fixtureSubscription struct {
	initial protocol.Update
	once    sync.Once
}

func (subscription *fixtureSubscription) Next(ctx context.Context) (protocol.Update, error) {
	var initial bool
	subscription.once.Do(func() { initial = true })
	if initial {
		return subscription.initial, nil
	}
	<-ctx.Done()
	return protocol.Update{}, ctx.Err()
}
func (*fixtureSubscription) Close() error { return nil }

func TestHostTaskObservation(t *testing.T) {
	t.Parallel()
	for _, row := range []struct {
		name     string
		status   protocol.ConversationStatus
		approval bool
		question bool
		paused   bool
		expected string
		exit     int
	}{
		{"completed", "completed", false, false, false, "completed", 0},
		{"committed_approval", "completed", true, false, false, "awaiting_approval", 3},
		{"committed_question", "completed", false, true, false, "awaiting_input", 3},
		{"paused", "queued", false, false, true, "paused", 3},
		{"failed", "failed", false, false, false, "failed", 1},
		{"continued", "continued", false, false, false, "continued", 1},
	} {
		t.Run(row.name, func(t *testing.T) {
			session, turn := protocol.SessionID{1}, protocol.TurnID{2}
			page := protocol.DefaultConversationPage()
			page.Session, page.Revision = session, 1
			page.Runs = []protocol.ConversationRun{{Session: session, Turn: turn, Sequence: 1, Revision: 7, Kind: "task", Status: row.status}}
			page.Entries = []protocol.ConversationEntry{
				{ID: "user", Turn: turn, Sequence: 1, Author: "user", Parts: []protocol.ConversationPart{{Kind: "text", Data: "Do not echo this"}}},
				{ID: "assistant", Turn: turn, Sequence: 2, Author: "assistant", Parts: []protocol.ConversationPart{{Kind: "text", Data: "Canonical answer"}}},
			}
			if row.paused {
				page.Queue = protocol.ConversationQueue{Revision: 1, Paused: true, Items: []protocol.ConversationPending{{Turn: turn, Revision: 1, ConfigRevision: 1}}}
			}
			initial := protocol.DefaultConversationSnapshot()
			initial.Node, initial.Sequence, initial.Page = protocol.NodeID{1}, 10, page
			if row.approval {
				page.Approvals = []protocol.ConversationApproval{{ID: protocol.ApprovalID{3}, Session: session, Turn: turn, Entry: "pending-tool", State: "pending"}}
			}
			if row.question {
				page.Questions = []protocol.ConversationQuestion{{ID: protocol.QuestionID{4}, Session: session, Turn: turn, Entry: "pending-tool", State: protocol.ConversationQuestionState{Kind: "pending"}}}
			}
			var mu sync.Mutex
			reads := 0
			transport := &fixtureTransport{
				dispatch: func(_ context.Context, request protocol.Request) (link.Admission, error) {
					if request.Command.Kind != "read_conversation" {
						return link.Admission{}, errors.New("task observation dispatched a mutation")
					}
					mu.Lock()
					reads++
					mu.Unlock()
					history := protocol.ConversationHistory{Sequence: 10, Page: page, Missing: []protocol.TurnID{}}
					return fixtureAdmission(request, protocol.Output{Kind: "conversation", Data: history}), nil
				},
				subscribe: func(_ context.Context, topic protocol.Topic) (link.Subscription, error) {
					if topic.Kind != "conversation" {
						return nil, errors.New("unexpected observation topic")
					}
					return &fixtureSubscription{initial: protocol.Update{Kind: "conversation_snapshot", Data: initial}}, nil
				},
			}
			controller := client.New(transport)
			defer controller.Close()
			result := observeTask(t.Context(), controller, session, turn)
			if result.State != row.expected || result.ExitCode() != row.exit || result.Revision == nil || *result.Revision != 7 || result.Text != "Canonical answer" {
				t.Fatalf("unexpected task outcome: %+v", result)
			}
			if row.approval && len(result.Approvals) != 1 || row.question && len(result.Questions) != 1 {
				t.Fatal("terminal observation missed a review in the durable transaction")
			}
			mu.Lock()
			count := reads
			mu.Unlock()
			if !activeStatus(row.status) && count < 2 {
				t.Fatal("terminal observation did not reread the durable transaction")
			}
		})
	}
}
