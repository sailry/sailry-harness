package mobile

import (
	"context"
	"encoding/json"
	"maps"
	"slices"
	"strings"
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

// topicStream yields its updates and then waits for its reader to stop.
type topicStream struct {
	updates chan protocol.Update
}

func (stream topicStream) Next(ctx context.Context) (protocol.Update, error) {
	select {
	case update := <-stream.updates:
		return update, nil
	case <-ctx.Done():
		return protocol.Update{}, ctx.Err()
	}
}
func (topicStream) Close() error { return nil }

// topicTransport serves each topic kind the updates a test gives it.
type topicTransport struct {
	noticeTransport
	topics map[string][]protocol.Update
}

func (transport topicTransport) Subscribe(_ context.Context, topic protocol.Topic) (link.Subscription, error) {
	updates, served := transport.topics[topic.Kind]
	if !served {
		return nil, unavailable("test subscription is unavailable")
	}
	stream := topicStream{updates: make(chan protocol.Update, len(updates))}
	for _, update := range updates {
		stream.updates <- update
	}
	return stream, nil
}

// The four topic observers reach Dart as a handle of the kind its API names
// and a JSON string per next, with exactly the members its readers take.
func TestMobileTopicObservers(t *testing.T) {
	t.Parallel()
	node := protocol.NodeID{1}
	terminal := protocol.DefaultTerminalInfo()
	terminal.ID, terminal.Revision, terminal.Status = protocol.NewTerminalID(), 3, protocol.TerminalStatus{Kind: "running"}
	screen := protocol.DefaultTerminalScreen()
	screen.Columns, screen.Rows = 80, make([]protocol.TerminalScreenLine, 24)
	provider := protocol.ConversationLoginAttempt{ID: protocol.NewRequestID(), Provider: protocol.NewProviderID()}
	connector := protocol.PluginConnectorAttempt{ID: protocol.NewRequestID(), Connection: protocol.NewRequestID(), Package: protocol.PluginReference{Name: "mail", Digest: strings.Repeat("a", 64)}, Revision: 1}
	changes := protocol.DefaultPluginConnectorChanges()
	changes.Cursor = 7
	transport := topicTransport{noticeTransport: noticeTransport{node: node}, topics: map[string][]protocol.Update{
		"terminal":        {{Kind: "terminal_snapshot", Data: protocol.TerminalSnapshot{Node: node, Info: terminal, Screen: screen}}},
		"provider_login":  {{Kind: "provider_login", Data: protocol.ConversationLoginUpdate{Attempt: provider, Revision: 1, State: protocol.ConversationLoginState{Kind: "starting"}}}},
		"connector_login": {{Kind: "connector_login", Data: protocol.PluginConnectorLoginUpdate{Attempt: connector, Revision: 1, State: protocol.PluginConnectorLoginState{Kind: "starting"}}}},
		"connections":     {{Kind: "connections_changed", Data: changes}},
	}}
	encoded := func(value any) string {
		data, err := json.Marshal(value)
		if err != nil {
			t.Fatal(err)
		}
		return string(data)
	}
	for _, test := range []struct {
		method    string
		arguments map[string]string
		kind      string
		members   []string
		content   string // a member holding an object, and what that object has
		inside    []string
	}{
		{"watch_terminal", map[string]string{"terminal": terminal.ID.String()}, "terminal_updates",
			[]string{"connected", "error", "recovering", "snapshot"}, "snapshot", []string{"info", "node", "screen", "sequence"}},
		{"watch_login", map[string]string{"attempt": encoded(provider)}, "login_updates",
			[]string{"connected", "error", "update"}, "update", []string{"attempt", "revision", "state"}},
		{"watch_connector_login", map[string]string{"attempt": encoded(connector)}, "login_updates",
			[]string{"connected", "error", "update"}, "update", []string{"attempt", "revision", "state"}},
		{"watch_connections", nil, "connections_updates", []string{"connections", "cursor", "types"}, "", nil},
	} {
		t.Run(test.method, func(t *testing.T) {
			t.Parallel()
			shared := client.New(transport)
			runtime := New(nil)
			t.Cleanup(func() { runtime.Close(1); _ = shared.Close() })
			owner := &connection{client: shared, controller: &controller{}}
			opened, err := owner.invoke(t.Context(), runtime, 0, test.method, json.RawMessage(encoded(test.arguments)))
			if err != nil {
				t.Fatal(err)
			}
			handle := opened.(Handle)
			if handle.Kind != test.kind || handle.Handle != "1" {
				t.Fatalf("handle = %#v", handle)
			}
			runtime.mu.Lock()
			updates := runtime.handles[1].object
			runtime.mu.Unlock()
			value, err := updates.invoke(t.Context(), runtime, 1, "next", nil)
			if err != nil {
				t.Fatal(err)
			}
			members := func(data string) (map[string]json.RawMessage, []string) {
				t.Helper()
				var object map[string]json.RawMessage
				if err := json.Unmarshal([]byte(data), &object); err != nil {
					t.Fatalf("%s: %v", data, err)
				}
				return object, slices.Sorted(maps.Keys(object))
			}
			view, names := members(value.(string))
			if !slices.Equal(names, test.members) {
				t.Fatalf("view %s has members %v, want %v", value, names, test.members)
			}
			if test.content == "" {
				if string(view["cursor"]) != "7" || string(view["types"]) != "[]" || string(view["connections"]) != "[]" {
					t.Fatalf("connections = %s", value)
				}
				return
			}
			if string(view["connected"]) != "true" || string(view["error"]) != "null" {
				t.Fatalf("view = %s", value)
			}
			if _, names := members(string(view[test.content])); !slices.Equal(names, test.inside) {
				t.Fatalf("%s has members %v, want %v", test.content, names, test.inside)
			}
		})
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
