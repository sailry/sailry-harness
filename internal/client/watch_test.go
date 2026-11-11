package client

import (
	"context"
	"encoding/hex"
	"encoding/json"
	"reflect"
	"runtime"
	"sync"
	"testing"
	"testing/synctest"
	"time"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
	"lukechampine.com/blake3"
)

type clientSubscription struct {
	updates chan protocol.Update
	closed  chan struct{}
	once    sync.Once
	failure error
}

func testSubscription(initial protocol.Snapshot) *clientSubscription {
	subscription := &clientSubscription{updates: make(chan protocol.Update, 8), closed: make(chan struct{})}
	subscription.updates <- protocol.Update{Kind: "snapshot", Data: initial}
	return subscription
}

func (subscription *clientSubscription) Next(ctx context.Context) (protocol.Update, error) {
	if subscription.failure != nil {
		return protocol.Update{}, subscription.failure
	}
	select {
	case update := <-subscription.updates:
		return update, nil
	case <-ctx.Done():
		return protocol.Update{}, ctx.Err()
	case <-subscription.closed:
		return protocol.Update{}, context.Canceled
	}
}

func (subscription *clientSubscription) Close() error {
	subscription.once.Do(func() { close(subscription.closed) })
	return nil
}

type clientTransport struct {
	open          func(context.Context, protocol.StreamID) (link.Stream, error)
	mu            sync.Mutex
	requests      []protocol.Request
	subscriptions []*clientSubscription
	snapshot      protocol.Snapshot
	dispatch      func(protocol.Request) (link.Admission, error)
}

func (transport *clientTransport) Target() protocol.NodeID { return protocol.NodeID{1} }
func (transport *clientTransport) WatchConnection(ctx context.Context, changed func(link.ConnectionInfo)) error {
	return link.NewLocal(transport.Target(), transport.Target(), nil).WatchConnection(ctx, changed)
}

func (transport *clientTransport) Dispatch(_ context.Context, request protocol.Request) (link.Admission, error) {
	transport.mu.Lock()
	transport.requests = append(transport.requests, request)
	dispatch := transport.dispatch
	snapshot := transport.snapshot
	transport.mu.Unlock()
	if dispatch != nil {
		return dispatch(request)
	}
	completion := make(chan link.Response, 1)
	completion <- link.Response{Output: protocol.Output{Kind: "snapshot", Data: snapshot}}
	return link.Admission{Receipt: protocol.Receipt{ID: request.ID, Durable: request.Command.Durable()}, Completion: completion}, nil
}

func (transport *clientTransport) Subscribe(_ context.Context, topic protocol.Topic) (link.Subscription, error) {
	transport.mu.Lock()
	defer transport.mu.Unlock()
	if topic.Kind != "node" {
		return nil, fault(protocol.ErrorCodeInvalidRequest, "Node topic expected")
	}
	if len(transport.subscriptions) == 0 {
		return nil, fault(protocol.ErrorCodeUnavailable, "no fixture subscription")
	}
	subscription := transport.subscriptions[0]
	transport.subscriptions = transport.subscriptions[1:]
	return subscription, nil
}

func waitForCursor(t *testing.T, controller *Client, changes <-chan struct{}, cursor uint64) {
	t.Helper()
	for controller.Snapshot().Cursor != cursor {
		<-changes
		if err := controller.Err(); err != nil {
			t.Fatal(err)
		}
	}
}

func TestRecoverAfterReset(t *testing.T) {
	t.Parallel()
	t.Run("conversation_watch_history_and_removal", testConversationWatchRecovery)
	t.Run("watch_reset_gap_refresh_and_copy", func(t *testing.T) {
		initial := projectionSnapshot(0)
		session := projectionSession(1, 1)
		session.Config.Options = map[string]map[string]json.RawMessage{"fixture": {"option": json.RawMessage(`{"value":1}`)}}
		initial.Sessions = []protocol.Session{session}
		initial.Worktrees = []protocol.Worktree{{ID: session.Worktree}}
		first := testSubscription(initial)
		second := testSubscription(projectionSnapshot(2))
		transport := &clientTransport{subscriptions: []*clientSubscription{first, second}, snapshot: projectionSnapshot(3)}
		controller := New(transport)
		t.Cleanup(func() {
			if err := controller.Close(); err != nil {
				t.Error(err)
			}
		})
		changes := make(chan struct{}, 16)
		unsubscribe := controller.OnChange(func() {
			select {
			case changes <- struct{}{}:
			default:
			}
		})
		defer unsubscribe()
		if err := controller.Start(context.Background()); err != nil {
			t.Fatal(err)
		}
		if !controller.Connected() {
			t.Fatal("Client did not establish its initial snapshot")
		}
		read := controller.Snapshot()
		read.Sessions[0].Config.Options["fixture"]["option"][0] = 'x'
		if !json.Valid(controller.Snapshot().Sessions[0].Config.Options["fixture"]["option"]) {
			t.Fatal("nested snapshot read changed shared state")
		}
		first.updates <- projectionEvent(1, "defaults_changed", protocol.Defaults{Revision: 1})
		waitForCursor(t, controller, changes, 1)
		first.updates <- protocol.Update{Kind: "reset_required"}
		waitForCursor(t, controller, changes, 3)
		transport.mu.Lock()
		requestCount := len(transport.requests)
		requestKind := ""
		if requestCount > 0 {
			requestKind = transport.requests[0].Command.Kind
		}
		transport.mu.Unlock()
		if requestCount != 1 || requestKind != "snapshot" {
			t.Fatal("reset did not perform exactly one snapshot read")
		}
		// Sync replaces the subscription generation and accepts a restored cursor.
		if err := controller.Refresh(context.Background()); err != nil {
			t.Fatal(err)
		}
		if controller.Snapshot().Cursor != 2 {
			t.Fatal("refresh did not use its new initial snapshot")
		}
		first.updates <- projectionEvent(99, "defaults_changed", protocol.Defaults{Revision: 99})
		second.updates <- projectionEvent(3, "defaults_changed", protocol.Defaults{Revision: 4})
		waitForCursor(t, controller, changes, 3)
		if controller.Snapshot().Defaults.Revision != 4 {
			t.Fatal("retired subscription changed the new projection")
		}
		transport.mu.Lock()
		transport.snapshot = projectionSnapshot(5)
		transport.mu.Unlock()
		second.updates <- projectionEvent(5, "defaults_changed", protocol.Defaults{Revision: 5})
		waitForCursor(t, controller, changes, 5)
		if err := controller.Close(); err != nil {
			t.Fatal(err)
		}
		if controller.Connected() {
			t.Fatal("closed Client remained connected")
		}
	})
	t.Run("refresh_failure_keeps_an_observation_owner", func(t *testing.T) {
		synctest.Test(t, func(t *testing.T) {
			lost := &clientSubscription{closed: make(chan struct{}), failure: fault(protocol.ErrorCodeUnavailable, "link dropped")}
			restored := testSubscription(projectionSnapshot(2))
			transport := &clientTransport{subscriptions: []*clientSubscription{testSubscription(projectionSnapshot(0)), lost, restored}}
			controller := New(transport)
			if err := controller.Start(t.Context()); err != nil {
				t.Fatal(err)
			}
			// The first read of the replacement fails after the old owner joined.
			if err := controller.Refresh(t.Context()); faultCode(err) != protocol.ErrorCodeUnavailable || controller.Connected() || controller.Revision() != 1 {
				t.Fatalf("failed refresh = %v, connected %v, revision %d", err, controller.Connected(), controller.Revision())
			}
			time.Sleep(250 * time.Millisecond)
			synctest.Wait()
			if !controller.Connected() || controller.Err() != nil || controller.Snapshot().Cursor != 2 || controller.Revision() != 2 {
				t.Fatalf("failed refresh was not retried: %v, revision %d", controller.Err(), controller.Revision())
			}
			// A replacement that cannot be opened leaves the current owner in place.
			if err := controller.Refresh(t.Context()); faultCode(err) != protocol.ErrorCodeUnavailable || !controller.Connected() {
				t.Fatalf("unopened refresh = %v, connected %v", err, controller.Connected())
			}
			restored.updates <- projectionEvent(3, "defaults_changed", protocol.Defaults{Revision: 3})
			synctest.Wait()
			if controller.Snapshot().Cursor != 3 || controller.Revision() != 3 {
				t.Fatal("unopened refresh retired the current observation")
			}
			// A permanent failure ends observation, as it does while observing.
			transport.mu.Lock()
			transport.subscriptions = []*clientSubscription{{closed: make(chan struct{}), failure: fault(protocol.ErrorCodePermissionDenied, "peer revoked access")}, testSubscription(projectionSnapshot(9))}
			transport.mu.Unlock()
			if err := controller.Refresh(t.Context()); faultCode(err) != protocol.ErrorCodePermissionDenied {
				t.Fatalf("revoked refresh = %v", err)
			}
			time.Sleep(time.Minute)
			synctest.Wait()
			// Only projection changes advance the revision, never connection state.
			if controller.Connected() || faultCode(controller.Err()) != protocol.ErrorCodePermissionDenied || controller.Snapshot().Cursor != 3 || controller.Revision() != 3 {
				t.Fatal("permanent refresh failure was retried")
			}
			if err := controller.Close(); err != nil {
				t.Fatal(err)
			}
		})
	})
	t.Run("close_interrupts_startup", func(t *testing.T) {
		stalled := &clientSubscription{updates: make(chan protocol.Update), closed: make(chan struct{})}
		transport := &clientTransport{subscriptions: []*clientSubscription{stalled}}
		controller := New(transport)
		started := make(chan error, 1)
		go func() { started <- controller.Start(context.Background()) }()
		for pending := 1; pending > 0; runtime.Gosched() {
			transport.mu.Lock()
			pending = len(transport.subscriptions)
			transport.mu.Unlock()
		}
		closed := make(chan error, 1)
		go func() { closed <- controller.Close() }()
		select {
		case err := <-closed:
			if err != nil {
				t.Fatal(err)
			}
		case <-time.After(10 * time.Second):
			t.Fatal("Close waited for the initial read")
		}
		if err := <-started; err == nil {
			t.Fatal("startup outlived Close")
		}
		<-stalled.closed
	})
	t.Run("captured_request_outcome_without_replay", func(t *testing.T) {
		transport := &clientTransport{}
		controller := New(transport)
		draft := protocol.CommandRenameSessionPayload{Session: protocol.SessionID{1}, ExpectedRevision: 3, Title: "Changed"}
		request := controller.Prepare(protocol.Command{Kind: "rename_session", Data: &draft})
		draft.Title = "Later draft"
		captured, err := payload[protocol.CommandRenameSessionPayload](request.Command.Data)
		if err != nil || captured.Title != "Changed" {
			t.Fatal("prepared request retained mutable draft data")
		}
		body, err := json.Marshal(request)
		if err != nil {
			t.Fatal(err)
		}
		digest := blake3.Sum256(body)
		transport.dispatch = func(actual protocol.Request) (link.Admission, error) {
			completion := make(chan link.Response, 1)
			if actual.Command.Kind == "rename_session" {
				if !reflect.DeepEqual(actual, request) {
					t.Error("Execute changed the prepared request")
				}
				close(completion)
			} else {
				query, err := payload[protocol.CommandInspectRequestPayload](actual.Command.Data)
				if err != nil || query.ID != request.ID || query.Digest != hex.EncodeToString(digest[:]) {
					t.Errorf("wrong reconciliation query: %v, %v", query, err)
				}
				completion <- link.Response{Output: protocol.Output{Kind: "request_outcome", Data: protocol.OutputRequestOutcomePayload{ID: request.ID, Outcome: protocol.RequestOutcome{Kind: "unknown"}}}}
			}
			return link.Admission{Receipt: protocol.Receipt{ID: actual.ID, Durable: true}, Completion: completion}, nil
		}
		if _, err := controller.Execute(context.Background(), request); faultCode(err) != protocol.ErrorCodeOutcomeUnknown {
			t.Fatalf("lost completion: %v", err)
		}
		outcome, err := controller.Outcome(context.Background(), request)
		if err != nil || outcome.Kind != "unknown" {
			t.Fatalf("outcome: %v, %v", outcome, err)
		}
		if len(transport.requests) != 2 || transport.requests[1].Command.Kind != "inspect_request" {
			t.Fatal("reconciliation replayed the mutation")
		}
		request.Target = protocol.NodeID{2}
		if _, err := controller.Execute(context.Background(), request); faultCode(err) != protocol.ErrorCodeWrongTarget {
			t.Fatalf("wrong target: %v", err)
		}
		if len(transport.requests) != 2 {
			t.Fatal("wrong-target request reached the transport")
		}
		if _, err := controller.Outcome(context.Background(), controller.Prepare(protocol.Command{Kind: "snapshot"})); faultCode(err) != protocol.ErrorCodeInvalidRequest {
			t.Fatalf("non-durable outcome: %v", err)
		}
		// An abandoned wait is uncertain only for a request with a stored outcome.
		transport.dispatch = func(actual protocol.Request) (link.Admission, error) {
			return link.Admission{Receipt: protocol.Receipt{ID: actual.ID}, Completion: make(chan link.Response)}, nil
		}
		abandoned, abandon := context.WithCancel(context.Background())
		abandon()
		if _, err := controller.Do(abandoned, protocol.Command{Kind: "snapshot"}); faultCode(err) != protocol.ErrorCodeCancelled {
			t.Fatalf("abandoned read: %v", err)
		}
		request.Target = controller.Target()
		if _, err := controller.Execute(abandoned, request); faultCode(err) != protocol.ErrorCodeOutcomeUnknown {
			t.Fatalf("abandoned mutation: %v", err)
		}
	})
	t.Run("activity_before_invalidations_coalesce", func(t *testing.T) {
		snapshot := projectionSnapshot(0)
		session := projectionSession(1, 1)
		run := protocol.DefaultConversationRun()
		run.Turn, run.Status, run.Sequence = protocol.TurnID{1}, protocol.ConversationStatusRunning, 1
		session.Activity.Run = &run
		snapshot.Sessions = []protocol.Session{session}
		var feed activityFeed
		feed.observe(snapshot)
		waiting := protocol.ActivityWaitingApproval
		snapshot.Cursor = 1
		snapshot.Sessions[0].Activity.Waiting = &waiting
		feed.observe(snapshot)
		snapshot.Cursor = 2
		snapshot.Sessions[0].Activity.Waiting = nil
		feed.observe(snapshot)
		if len(feed.notices) != 1 || feed.notices[0].Kind != "approval" {
			t.Fatal("brief approval attention disappeared before presentation")
		}
		controller := New(&clientTransport{})
		controller.feed = feed
		id := feed.notices[0].ID
		controller.MarkAttentionRead(id)
		if !controller.Notices()[0].Read {
			t.Fatal("controller attention did not become read")
		}
		controller.DismissAttention([]NoticeID{id})
		if len(controller.Notices()) != 0 {
			t.Fatal("dismissed controller attention remained visible")
		}
		if LaneOf(snapshot.Sessions[0]) != LaneRunning {
			t.Fatal("running activity lane was lost")
		}
		snapshot.Sessions[0].Activity.Attention.Unread = true
		completed := run
		completed.Status = protocol.ConversationStatusCompleted
		snapshot.Sessions[0].Activity.Run = &completed
		if LaneOf(snapshot.Sessions[0]) != LaneCompleted {
			t.Fatal("finished-unread lane was lost")
		}
	})
}

func (transport *clientTransport) Open(ctx context.Context, id protocol.StreamID) (link.Stream, error) {
	if transport.open != nil {
		return transport.open(ctx, id)
	}
	return nil, &protocol.Fault{Code: protocol.ErrorCodeNotFound, Message: "fixture stream is unavailable"}
}
