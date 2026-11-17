package link

import (
	"bytes"
	"context"
	"encoding/hex"
	"encoding/json"
	"io"
	"sync/atomic"
	"testing"
	"testing/synctest"
	"time"

	"github.com/sailry/sailry-harness/internal/link/iroh"
	"github.com/sailry/sailry-harness/internal/protocol"
	"github.com/sailry/sailry-harness/internal/protocol/wire"
	"lukechampine.com/blake3"
)

type noticeStore struct {
	*fixturePeerStore
	attempt HostRemovalAttempt
}

func (store *noticeStore) PrepareHostRemoval(context.Context, protocol.NodeID, protocol.Request) (HostRemovalAttempt, error) {
	store.mu.Lock()
	defer store.mu.Unlock()
	return cloneHostRemoval(store.attempt)
}

func (store *noticeStore) RecordHostRemoval(_ context.Context, host protocol.NodeID, request protocol.RequestID, state string, failure *protocol.Fault) error {
	store.mu.Lock()
	defer store.mu.Unlock()
	if host != store.attempt.Request.Target || request != store.attempt.Request.ID {
		return invalidLink("notification changed the original request")
	}
	store.attempt.State, store.attempt.Fault = state, failure
	return nil
}

func (store *noticeStore) HostRemovals(context.Context) ([]HostRemovalAttempt, error) {
	store.mu.Lock()
	defer store.mu.Unlock()
	copy, err := cloneHostRemoval(store.attempt)
	return []HostRemovalAttempt{copy}, err
}

func noticeFixture(t *testing.T, worker bool, state string) (*Link, *noticeStore, HostRemovalAttempt) {
	t.Helper()
	self, host := protocol.NodeID{1}, protocol.NodeID{2}
	attempt := HostRemovalAttempt{Request: protocol.NewRequest(host, protocol.Command{Kind: "identity", Data: protocol.IdentityCommand{Kind: "revoke_device", Data: protocol.RevokeDevice{Device: self, ConfirmLast: true}}}), State: state, Active: true}
	store := &noticeStore{fixturePeerStore: &fixturePeerStore{trusted: map[protocol.NodeID]bool{host: true}, directions: map[protocol.NodeID]PeerDirection{host: DirectionHost}, addresses: map[protocol.NodeID]EndpointAddress{}}, attempt: attempt}
	ctx, cancel := context.WithCancel(t.Context())
	service := &Link{nodeID: self, ctx: ctx, cancel: cancel, store: store, softwareVersion: ProductVersion,
		trusted: map[protocol.NodeID]bool{host: true}, directions: map[protocol.NodeID]PeerDirection{host: DirectionHost},
		outgoing: map[protocol.NodeID]*outgoingSlot{}, work: make(chan func(), maxTasks), timeout: context.WithTimeout}
	if worker {
		service.workers.Add(1)
		go service.driveWork()
	}
	t.Cleanup(func() { cancel(); service.workers.Wait() })
	return service, store, attempt
}

func reserveNoticeFixture(t *testing.T, service *Link, attempt HostRemovalAttempt) (*HostRemovalNotice, *atomic.Int32) {
	t.Helper()
	ctx, cancel := context.WithCancel(service.ctx)
	done := make(chan struct{})
	close(done)
	route := RouteP2P
	service.outgoing[attempt.Request.Target] = &outgoingSlot{ctx: ctx, cancel: cancel, connection: &iroh.Connection{},
		telemetry: &connectionTelemetry{value: ConnectionInfo{Route: &route}, changed: make(chan struct{}), cancel: func() {}, done: done}}
	_, notice, err := service.reserveHostRemovalNotice(t.Context(), attempt)
	if err != nil || notice == nil {
		t.Fatalf("reserve notification: %v", err)
	}
	closed := &atomic.Int32{}
	notice.close = func() { closed.Add(1) }
	return notice, closed
}

type noticeStream struct{ *fixtureContextStream }

func (stream *noticeStream) ReadContext(ctx context.Context, data []byte) (int, error) {
	count, err := stream.fixtureContextStream.ReadContext(ctx, data)
	if count != 0 || err != io.EOF {
		return count, err
	}
	select {
	case <-ctx.Done():
		return 0, ctx.Err()
	case <-stream.stopped:
		return 0, io.ErrClosedPipe
	}
}

func noticeScript(t *testing.T, notice *HostRemovalNotice, reply func(protocol.Request) (bool, *protocol.Output)) (<-chan protocol.Request, <-chan *noticeStream) {
	t.Helper()
	requests := make(chan protocol.Request, 2)
	streams := make(chan *noticeStream, 2)
	notice.open = func(context.Context) (contextStream, error) {
		stream := &noticeStream{newFixtureStream(nil)}
		responded := false
		stream.afterWrite = func(written []byte) {
			var envelope wire.Envelope
			if responded || wire.Read(bytes.NewReader(written), &envelope, false) != nil || envelope.Call.Request == nil {
				return
			}
			responded = true
			request := *envelope.Call.Request
			requests <- request
			acknowledge, output := reply(request)
			var frames []wire.Reply
			if acknowledge {
				frames = append(frames, wire.Reply{Kind: wire.ReplyReceipt, Receipt: &protocol.Receipt{ID: request.ID, Durable: request.Command.Durable()}})
			}
			if output != nil {
				frames = append(frames, wire.Reply{Kind: wire.ReplyCompleted, Completed: &wire.Response{Output: output}})
			}
			stream.mu.Lock()
			stream.incoming = bytes.NewReader(fixtureFrames(t, frames...))
			stream.mu.Unlock()
		}
		streams <- stream
		return stream, nil
	}
	return requests, streams
}

func TestHostRemovalNoticeRequiresLocalRevocation(t *testing.T) {
	service, store, original := noticeFixture(t, false, "pending")
	notice, closed := reserveNoticeFixture(t, service, original)
	defer notice.Close()
	if err := notice.Start(); err == nil {
		t.Fatal("notification started before local trust was removed")
	}
	history, _ := store.HostRemovals(t.Context())
	if len(service.work) != 0 || closed.Load() != 0 || history[0].State != "pending" {
		t.Fatal("rejected notification changed its reservation or outcome")
	}
}

func TestHostRemovalOfflineDoesNotReserveOrDial(t *testing.T) {
	service, store, original := noticeFixture(t, false, "pending")
	// No native endpoint is available: an attempted dial would fail this test.
	attempt, notice, err := service.reserveHostRemovalNotice(t.Context(), original)
	if err != nil || notice != nil || attempt.State != "failed" || attempt.Request.ID != original.Request.ID || len(service.work) != 0 {
		t.Fatalf("offline notification = %+v, %v, %v", attempt, notice, err)
	}
	history, _ := store.HostRemovals(t.Context())
	if history[0].State != "failed" || history[0].Fault == nil {
		t.Fatal("offline omission was not retained")
	}
}

func TestHostRemovalNoticeDoesNotDelayLocalRemoval(t *testing.T) {
	for _, receipt := range []bool{false, true} {
		t.Run(map[bool]string{false: "receipt stalled", true: "completion stalled"}[receipt], func(t *testing.T) {
			synctest.Test(t, func(t *testing.T) {
				service, store, original := noticeFixture(t, true, "pending")
				notice, closed := reserveNoticeFixture(t, service, original)
				requests, streams := noticeScript(t, notice, func(protocol.Request) (bool, *protocol.Output) { return receipt, nil })
				started := time.Now()
				if err := service.SetTrust(t.Context(), original.Request.Target, false); err != nil {
					t.Fatal(err)
				}
				if service.Direction(original.Request.Target).Valid() || time.Since(started) != 0 || closed.Load() != 0 || len(requests) != 0 {
					t.Fatal("local removal waited for or destroyed its notification")
				}
				if err := notice.Start(); err != nil || time.Since(started) != 0 {
					t.Fatalf("starting notification blocked: %v", err)
				}
				synctest.Wait()
				request := <-requests
				if request.ID != original.Request.ID || request.Command.Kind != "identity" {
					t.Fatal("notification changed the original revocation")
				}
				stream := <-streams
				time.Sleep(10 * time.Second)
				synctest.Wait()
				history, _ := store.HostRemovals(t.Context())
				if history[0].Request.ID != original.Request.ID || history[0].State != "unknown" || closed.Load() != 1 || !channelClosed(stream.stopped) || len(requests) != 0 {
					t.Fatalf("expired notification retained work or lost uncertainty: %+v, closes=%d", history, closed.Load())
				}
			})
		})
	}
}

func TestHostRemovalNoticeQueueAndPairingCancellation(t *testing.T) {
	for _, reason := range []string{"queued timeout", "re-paired", "link closed", "local save failed"} {
		t.Run(reason, func(t *testing.T) {
			synctest.Test(t, func(t *testing.T) {
				service, _, original := noticeFixture(t, false, "pending")
				notice, closed := reserveNoticeFixture(t, service, original)
				requests, _ := noticeScript(t, notice, func(protocol.Request) (bool, *protocol.Output) { return false, nil })
				if reason == "local save failed" {
					notice.Close()
					if closed.Load() != 0 || !service.Direction(original.Request.Target).AllowsOutbound() || len(service.work) != 0 {
						t.Fatal("abandoned reservation changed trust or started notification")
					}
					return
				}
				if err := service.SetTrust(t.Context(), original.Request.Target, false); err != nil {
					t.Fatal(err)
				}
				if err := notice.Start(); err != nil {
					t.Fatal(err)
				}
				switch reason {
				case "queued timeout":
					time.Sleep(10 * time.Second)
				case "re-paired":
					if err := service.pairPeer(t.Context(), original.Request.Target, DirectionHost); err != nil {
						t.Fatal(err)
					}
				case "link closed":
					service.cancel()
				}
				synctest.Wait()
				if closed.Load() != 1 {
					t.Fatal("queued notification retained its detached connection")
				}
				(<-service.work)()
				if len(requests) != 0 {
					t.Fatal("cancelled queued notification sent an old revocation")
				}
			})
		})
	}
}

func TestHostRemovalUnknownOnlyInspectsOriginalRequest(t *testing.T) {
	for _, outcome := range []string{"admitted", "unknown", "not_admitted"} {
		t.Run(outcome, func(t *testing.T) {
			synctest.Test(t, func(t *testing.T) {
				service, store, original := noticeFixture(t, true, "unknown")
				notice, closed := reserveNoticeFixture(t, service, original)
				requests, _ := noticeScript(t, notice, func(request protocol.Request) (bool, *protocol.Output) {
					return true, &protocol.Output{Kind: "request_outcome", Data: protocol.OutputRequestOutcomePayload{ID: original.Request.ID, Outcome: protocol.RequestOutcome{Kind: outcome}}}
				})
				if err := service.SetTrust(t.Context(), original.Request.Target, false); err != nil {
					t.Fatal(err)
				}
				if err := notice.Start(); err != nil {
					t.Fatal(err)
				}
				synctest.Wait()
				request := <-requests
				var query protocol.CommandInspectRequestPayload
				body, _ := json.Marshal(request.Command.Data)
				originalBody, _ := json.Marshal(original.Request)
				digest := blake3.Sum256(originalBody)
				if json.Unmarshal(body, &query) != nil || request.Command.Kind != "inspect_request" || query.ID != original.Request.ID || query.Digest != hex.EncodeToString(digest[:]) || len(requests) != 0 {
					t.Fatal("unknown removal created a new effect or inspected another request")
				}
				want := "unknown"
				if outcome == "not_admitted" {
					want = "failed"
				}
				history, _ := store.HostRemovals(t.Context())
				if history[0].State != want || history[0].Request.ID != original.Request.ID || closed.Load() != 1 {
					t.Fatalf("inspection discarded original outcome: %+v", history)
				}
			})
		})
	}
}
