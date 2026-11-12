package mobile

import (
	"context"
	"encoding/json"
	"testing"

	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type noticeTransport struct {
	node protocol.NodeID
}

func (transport noticeTransport) Target() protocol.NodeID { return transport.node }
func (noticeTransport) Open(context.Context, protocol.StreamID) (link.Stream, error) {
	return nil, unavailable("test stream is unavailable")
}
func (noticeTransport) Dispatch(context.Context, protocol.Request) (link.Admission, error) {
	return link.Admission{}, unavailable("test command is unavailable")
}
func (noticeTransport) Subscribe(context.Context, protocol.Topic) (link.Subscription, error) {
	return nil, unavailable("test subscription is unavailable")
}
func (noticeTransport) WatchConnection(ctx context.Context, _ func(link.ConnectionInfo)) error {
	<-ctx.Done()
	return ctx.Err()
}

// ledgerTransport loses the reply of every mutation and answers
// inspect_request with outcome, or fails the query when outcome is nil.
type ledgerTransport struct {
	noticeTransport
	outcome   *protocol.RequestOutcome
	mutations *int
}

func (transport ledgerTransport) Dispatch(_ context.Context, request protocol.Request) (link.Admission, error) {
	completion := make(chan link.Response, 1)
	switch {
	case request.Command.Kind != "inspect_request":
		*transport.mutations++
		close(completion)
	case transport.outcome == nil:
		return link.Admission{}, unavailable("Node is unreachable")
	default:
		query := request.Command.Data.(protocol.CommandInspectRequestPayload)
		completion <- link.Response{Output: protocol.Output{Kind: "request_outcome", Data: protocol.OutputRequestOutcomePayload{ID: query.ID, Outcome: *transport.outcome}}}
	}
	return link.Admission{Receipt: protocol.Receipt{ID: request.ID, Durable: request.Command.Durable()}, Completion: completion}, nil
}

func TestMobileUncertainRequest(t *testing.T) {
	t.Parallel()
	stored := protocol.Output{Kind: "notifications_updated"}
	conflict := protocol.Fault{Code: protocol.ErrorCodeConflict, Message: "session changed"}
	completed := func(value protocol.Result[protocol.Output, protocol.Fault]) *protocol.RequestOutcome {
		return &protocol.RequestOutcome{Kind: "completed", Data: value}
	}
	for _, test := range []struct {
		name     string
		outcome  *protocol.RequestOutcome
		code     protocol.ErrorCode // "" for success
		released bool
	}{
		{"applied", completed(protocol.Result[protocol.Output, protocol.Fault]{Ok: &stored}), "", false},
		{"failed", completed(protocol.Result[protocol.Output, protocol.Fault]{Err: &conflict}), protocol.ErrorCodeConflict, false},
		{"interrupted by a restart", &protocol.RequestOutcome{Kind: "unknown"}, protocol.ErrorCodeOutcomeUnknown, true},
		{"never admitted", &protocol.RequestOutcome{Kind: "not_admitted"}, protocol.ErrorCodeUnavailable, true},
		{"still running", &protocol.RequestOutcome{Kind: "admitted"}, protocol.ErrorCodeOutcomeUnknown, false},
		{"Node unreachable", nil, protocol.ErrorCodeOutcomeUnknown, false},
	} {
		t.Run(test.name, func(t *testing.T) {
			mutations := 0
			shared := client.New(ledgerTransport{noticeTransport: noticeTransport{node: protocol.NodeID{1}}, outcome: test.outcome, mutations: &mutations})
			t.Cleanup(func() { _ = shared.Close() })
			owner := &connection{client: shared, controller: &controller{}}
			call := func(method, key, value string) string {
				t.Helper()
				data, _ := json.Marshal(map[string]string{key: value})
				encoded, err := owner.invoke(t.Context(), nil, 0, method, data)
				if err != nil {
					t.Fatal(err)
				}
				return encoded.(string)
			}
			request := call("prepare", "command", `{"kind":"rename_session","data":{"session":"`+protocol.NewSessionID().String()+`","expected_revision":1,"title":"Changed"}}`)
			// execute sends once; resolve, the result check, never sends.
			for sent, method := range []string{"execute", "resolve", "resolve"} {
				var answer struct {
					Ok       *json.RawMessage
					Err      *protocol.Fault
					Released bool `json:"released"`
				}
				if err := json.Unmarshal([]byte(call(method, "request", request)), &answer); err != nil {
					t.Fatal(err)
				}
				if test.code == "" {
					if answer.Ok == nil || answer.Err != nil || answer.Released {
						t.Fatalf("%s %d = %#v", method, sent, answer)
					}
				} else if answer.Ok != nil || answer.Err == nil || answer.Err.Code != test.code || answer.Released != test.released {
					t.Fatalf("%s %d = %#v, fault %#v", method, sent, answer, answer.Err)
				}
				if mutations != 1 {
					t.Fatalf("the mutation was sent %d times after %s %d", mutations, method, sent)
				}
			}
		})
	}
	t.Run("a request without a stored outcome is released", func(t *testing.T) {
		mutations := 0
		shared := client.New(ledgerTransport{noticeTransport: noticeTransport{node: protocol.NodeID{1}}, mutations: &mutations})
		t.Cleanup(func() { _ = shared.Close() })
		owner := &connection{client: shared, controller: &controller{}}
		request, _ := json.Marshal(shared.Prepare(protocol.Command{Kind: "snapshot"}))
		data, _ := json.Marshal(map[string]string{"request": string(request)})
		encoded, err := owner.invoke(t.Context(), nil, 0, "resolve", data)
		if err != nil {
			t.Fatal(err)
		}
		var answer struct {
			Err      *protocol.Fault
			Released bool `json:"released"`
		}
		if err := json.Unmarshal([]byte(encoded.(string)), &answer); err != nil || answer.Err == nil || answer.Err.Code != protocol.ErrorCodeUnavailable || !answer.Released || mutations != 0 {
			t.Fatalf("resolved read = %s, %v, sent %d", encoded, err, mutations)
		}
	})
}

func TestMobileClosedConnectionLeavesNoClient(t *testing.T) {
	t.Parallel()
	node := protocol.NodeID{1}
	owner := &controller{clients: map[protocol.NodeID]*client.Client{}}
	first := &connection{client: client.New(noticeTransport{node: node}), controller: owner}
	second := &connection{client: client.New(noticeTransport{node: node}), controller: owner}
	// A second connect to the same peer replaces the first as its Client.
	owner.clients[node] = first.client
	owner.clients[node] = second.client
	if err := first.close(); err != nil {
		t.Fatal(err)
	}
	if owner.clients[node] != second.client {
		t.Fatal("closing a replaced connection removed the current Client")
	}
	if err := second.close(); err != nil {
		t.Fatal(err)
	}
	if _, kept := owner.clients[node]; kept {
		t.Fatal("a closed Client stayed reachable for notification commands")
	}
}

func TestMobileUnobservedConnectionPreservesInbox(t *testing.T) {
	t.Parallel()
	for _, method := range []string{"close", "watch", "watch_terminal"} {
		t.Run(method, func(t *testing.T) {
			node := protocol.NodeID{1}
			notice := client.Notice{
				ID: client.NoticeID{
					Node: node, Cursor: 1,
					Target: client.NoticeTarget{Kind: "notification", Notification: protocol.NewNotificationID()},
				},
				Kind: "notification", NotificationKind: protocol.NotificationKindInfo,
			}
			controller := &controller{}
			controller.inbox.Merge(node, []client.Notice{notice})
			shared := client.New(noticeTransport{node: node})
			t.Cleanup(func() { _ = shared.Close() })
			owner := &connection{client: shared, controller: controller}
			if method == "close" {
				if err := owner.close(); err != nil {
					t.Fatal(err)
				}
			} else {
				argument := json.RawMessage("null")
				if method == "watch_terminal" {
					argument, _ = json.Marshal(protocol.NewTerminalID())
				}
				observer, err := shared.Watch(t.Context(), method, argument)
				if err != nil {
					t.Fatal(err)
				}
				t.Cleanup(func() { _ = observer.Close() })
				updates := &observation{observer: observer, connection: owner, notices: method == "watch"}
				if _, err := updates.invoke(t.Context(), nil, 0, "next", nil); err != nil {
					t.Fatal(err)
				}
			}
			retained := controller.inbox.Notices()
			if len(retained) != 1 || retained[0].ID != notice.ID || controller.inbox.Unread() != 1 {
				t.Fatal("an unobserved connection discarded retained Node notices")
			}
		})
	}
}
