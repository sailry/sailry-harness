package link

import (
	"bytes"
	"context"
	"encoding/binary"
	"encoding/json"
	"errors"
	"io"
	"runtime"
	"strings"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/link/iroh"
	"github.com/sailry/sailry-harness/internal/protocol"
	"github.com/sailry/sailry-harness/internal/protocol/wire"
)

type fixtureContextStream struct {
	mu          sync.Mutex
	incoming    *bytes.Reader
	outgoing    bytes.Buffer
	stopped     chan struct{}
	stopOnce    sync.Once
	closed      bool
	finished    bool
	maxRead     int
	readCount   int
	afterRead   func(int)
	afterWrite  func([]byte)
	writeError  error
	finishError error
}

func newFixtureStream(incoming []byte) *fixtureContextStream {
	return &fixtureContextStream{incoming: bytes.NewReader(incoming), stopped: make(chan struct{})}
}

func (stream *fixtureContextStream) Read(data []byte) (int, error) {
	return stream.ReadContext(context.Background(), data)
}

func (stream *fixtureContextStream) Write(data []byte) (int, error) {
	return stream.WriteContext(context.Background(), data)
}

func (stream *fixtureContextStream) ReadContext(ctx context.Context, data []byte) (int, error) {
	if err := ctx.Err(); err != nil {
		return 0, err
	}
	stream.mu.Lock()
	if stream.closed {
		stream.mu.Unlock()
		return 0, io.ErrClosedPipe
	}
	if stream.maxRead > 0 && len(data) > stream.maxRead {
		data = data[:stream.maxRead]
	}
	count, err := stream.incoming.Read(data)
	stream.readCount += count
	total := stream.readCount
	after := stream.afterRead
	stream.mu.Unlock()
	if after != nil {
		after(total)
	}
	return count, err
}

func (stream *fixtureContextStream) WriteContext(ctx context.Context, data []byte) (int, error) {
	if err := ctx.Err(); err != nil {
		return 0, err
	}
	stream.mu.Lock()
	if stream.closed {
		stream.mu.Unlock()
		return 0, io.ErrClosedPipe
	}
	if stream.writeError != nil {
		stream.mu.Unlock()
		return 0, stream.writeError
	}
	count, err := stream.outgoing.Write(data)
	after := stream.afterWrite
	var written []byte
	if after != nil {
		written = append([]byte{}, stream.outgoing.Bytes()...)
	}
	stream.mu.Unlock()
	if after != nil {
		after(written)
	}
	return count, err
}

func (stream *fixtureContextStream) CloseWrite() error {
	stream.mu.Lock()
	defer stream.mu.Unlock()
	stream.finished = true
	return stream.finishError
}

func (stream *fixtureContextStream) Close() error {
	stream.mu.Lock()
	stream.closed = true
	stream.mu.Unlock()
	stream.stopOnce.Do(func() { close(stream.stopped) })
	return nil
}

func (stream *fixtureContextStream) WaitStopped(ctx context.Context) (*uint64, error) {
	select {
	case <-ctx.Done():
		return nil, ctx.Err()
	case <-stream.stopped:
		return nil, nil
	}
}

func (stream *fixtureContextStream) written() []byte {
	stream.mu.Lock()
	defer stream.mu.Unlock()
	return append([]byte{}, stream.outgoing.Bytes()...)
}

type fixturePeerStore struct {
	mu              sync.Mutex
	trusted         map[protocol.NodeID]bool
	addresses       map[protocol.NodeID]EndpointAddress
	trustOutcome    error
	rememberOutcome error
}

func (store *fixturePeerStore) Peers(context.Context) ([]protocol.NodeID, error) {
	store.mu.Lock()
	defer store.mu.Unlock()
	peers := make([]protocol.NodeID, 0, len(store.trusted))
	for peer := range store.trusted {
		peers = append(peers, peer)
	}
	return peers, nil
}

func (store *fixturePeerStore) Addresses(context.Context) ([]EndpointAddress, error) {
	store.mu.Lock()
	defer store.mu.Unlock()
	addresses := make([]EndpointAddress, 0, len(store.addresses))
	for _, address := range store.addresses {
		addresses = append(addresses, cloneAddress(address))
	}
	return addresses, nil
}

func (store *fixturePeerStore) SetTrust(_ context.Context, peer protocol.NodeID, trusted bool) error {
	store.mu.Lock()
	defer store.mu.Unlock()
	if trusted {
		store.trusted[peer] = true
	} else {
		delete(store.trusted, peer)
		delete(store.addresses, peer)
	}
	return store.trustOutcome
}

func (store *fixturePeerStore) Remember(_ context.Context, address EndpointAddress) error {
	store.mu.Lock()
	defer store.mu.Unlock()
	if !store.trusted[address.ID] {
		return &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "peer is not paired"}
	}
	store.addresses[address.ID] = cloneAddress(address)
	return store.rememberOutcome
}

func newFixtureLink(t *testing.T) *Link {
	t.Helper()
	ctx, cancel := context.WithCancel(t.Context())
	link := &Link{
		ctx: ctx, cancel: cancel, softwareVersion: ProductVersion,
		store:   &fixturePeerStore{trusted: map[protocol.NodeID]bool{}, addresses: map[protocol.NodeID]EndpointAddress{}},
		trusted: map[protocol.NodeID]bool{}, addresses: map[protocol.NodeID]EndpointAddress{},
		outgoing: map[protocol.NodeID]*outgoingSlot{}, work: make(chan func(), maxTasks),
		now: func() time.Time { return time.Unix(100, 0) },
		timeout: func(ctx context.Context, _ time.Duration) (context.Context, context.CancelFunc) {
			return context.WithCancel(ctx)
		},
	}
	link.workers.Add(1)
	go link.driveWork()
	t.Cleanup(func() { cancel(); link.workers.Wait() })
	return link
}

func fixtureFrames(t *testing.T, replies ...wire.Reply) []byte {
	t.Helper()
	var encoded bytes.Buffer
	for _, reply := range replies {
		if err := wire.WriteReply(reply, func(frame wire.Reply) error { return wire.Write(&encoded, frame, false) }); err != nil {
			t.Fatal(err)
		}
	}
	return encoded.Bytes()
}

func TestRemoteAdmissionAndUncertainty(t *testing.T) {
	t.Parallel()
	target := protocol.NodeID{1}
	request := protocol.NewRequest(target, protocol.Command{Kind: "snapshot"})
	receipt := protocol.Receipt{ID: request.ID}
	completed := wire.Response{Output: &protocol.Output{Kind: "notifications_updated"}}
	t.Run("receipt_then_completion_survives_caller", func(t *testing.T) {
		link := newFixtureLink(t)
		remote := link.Remote(EndpointAddress{ID: target})
		// Only a durable command outlives the caller that dispatched it.
		request := protocol.NewRequest(target, protocol.Command{Kind: "dismiss_read_notifications", Data: &protocol.CommandDismissReadNotificationsPayload{}})
		receipt := protocol.Receipt{ID: request.ID, Durable: true}
		stream := newFixtureStream(fixtureFrames(t,
			wire.Reply{Kind: wire.ReplyReceipt, Receipt: &receipt},
			wire.Reply{Kind: wire.ReplyCompleted, Completed: &completed},
		))
		ctx, cancel := context.WithCancel(t.Context())
		admission, err := remote.dispatchStream(ctx, request, stream, false)
		cancel()
		if err != nil || admission.Receipt != receipt {
			t.Fatalf("admission: %v, %v", admission.Receipt, err)
		}
		result := <-admission.Completion
		if result.Fault != nil || result.Output.Kind != "notifications_updated" {
			t.Fatalf("completion: %#v", result)
		}
		var envelope wire.Envelope
		if err := wire.Read(bytes.NewReader(stream.written()), &envelope, false); err != nil {
			t.Fatal(err)
		}
		if envelope.Call.Request == nil || envelope.Call.Request.ID != request.ID {
			t.Fatal("dispatch changed the captured request identifier")
		}
	})
	t.Run("cancelled_caller_stops_a_command_that_is_not_durable", func(t *testing.T) {
		link := newFixtureLink(t)
		admitted := fixtureFrames(t, wire.Reply{Kind: wire.ReplyReceipt, Receipt: &receipt})
		stream := newFixtureStream(append(admitted, fixtureFrames(t, wire.Reply{Kind: wire.ReplyCompleted, Completed: &completed})...))
		ctx, cancel := context.WithCancel(t.Context())
		defer cancel()
		// The caller gives up between the receipt and the completion.
		stream.afterRead = func(total int) {
			if total == len(admitted) {
				cancel()
			}
		}
		admission, err := link.Remote(EndpointAddress{ID: target}).dispatchStream(ctx, request, stream, false)
		if err != nil {
			t.Fatal(err)
		}
		response := <-admission.Completion
		if response.Fault == nil || response.Fault.Code != protocol.ErrorCodeCancelled {
			t.Fatalf("completion after cancellation: %#v", response)
		}
		// Closing the stream is the stop the serving Node observes.
		if _, err := stream.WaitStopped(t.Context()); err != nil {
			t.Fatal(err)
		}
	})
	t.Run("missing_completion", func(t *testing.T) {
		link := newFixtureLink(t)
		stream := newFixtureStream(fixtureFrames(t, wire.Reply{Kind: wire.ReplyReceipt, Receipt: &receipt}))
		admission, err := link.Remote(EndpointAddress{ID: target}).dispatchStream(t.Context(), request, stream, false)
		if err != nil {
			t.Fatal(err)
		}
		response := <-admission.Completion
		if response.Fault == nil || response.Fault.Code != protocol.ErrorCodeOutcomeUnknown {
			t.Fatalf("lost completion: %#v", response)
		}
	})
	for _, name := range []string{"missing_receipt", "mismatched_receipt", "completed_before_receipt", "write_failure", "finish_failure"} {
		t.Run(name, func(t *testing.T) {
			link := newFixtureLink(t)
			stream := newFixtureStream(nil)
			switch name {
			case "mismatched_receipt":
				other := protocol.Receipt{ID: protocol.NewRequestID()}
				stream.incoming = bytes.NewReader(fixtureFrames(t, wire.Reply{Kind: wire.ReplyReceipt, Receipt: &other}))
			case "completed_before_receipt":
				stream.incoming = bytes.NewReader(fixtureFrames(t, wire.Reply{Kind: wire.ReplyCompleted, Completed: &completed}))
			case "write_failure":
				stream.writeError = io.ErrClosedPipe
			case "finish_failure":
				stream.finishError = io.ErrClosedPipe
			}
			_, err := link.Remote(EndpointAddress{ID: target}).dispatchStream(t.Context(), request, stream, false)
			if fault := faultValue(err); fault.Code != protocol.ErrorCodeOutcomeUnknown {
				t.Fatalf("interrupted dispatch: %v", err)
			}
		})
	}
	t.Run("wrong_target_does_not_connect", func(t *testing.T) {
		link := newFixtureLink(t)
		_, err := link.Remote(EndpointAddress{ID: protocol.NodeID{2}}).Dispatch(t.Context(), request)
		if fault := faultValue(err); fault.Code != protocol.ErrorCodeWrongTarget {
			t.Fatalf("wrong target: %v", err)
		}
	})
}

func TestSubscriptionCancellationRetainsPartialFrame(t *testing.T) {
	t.Parallel()
	link := newFixtureLink(t)
	stream := newFixtureStream(fixtureFrames(t, wire.Reply{Kind: wire.ReplyUpdate, Update: &protocol.Update{Kind: "reset_required"}}))
	ctx, cancel := context.WithCancel(t.Context())
	stream.maxRead = 2
	stream.afterRead = func(total int) {
		if total == 2 {
			cancel()
		}
	}
	subscription := &remoteSubscription{link: link, stream: stream, reader: wire.NewReader(false)}
	if _, err := subscription.Next(ctx); err == nil {
		t.Fatal("cancelled partial prefix unexpectedly completed")
	}
	update, err := subscription.Next(t.Context())
	if err != nil || update.Kind != "reset_required" {
		t.Fatalf("resumed frame: %#v, %v", update, err)
	}
	if err := subscription.Close(); err != nil {
		t.Fatal(err)
	}
	if err := subscription.Close(); err != nil {
		t.Fatal(err)
	}
}

func TestProtocolControlsCompatibility(t *testing.T) {
	t.Parallel()
	version, old, newer := uint16(protocol.VERSION), uint16(0), uint16(2)
	unknownVersion, otherVersion := "invalid", "999.0.0"
	if err := checkCompatibility(&version, &unknownVersion, &version, &otherVersion); err != nil {
		t.Fatalf("product version blocked matching protocol: %v", err)
	}
	for _, candidate := range []struct {
		controller *uint16
		host       *uint16
		updateHost bool
		unverified bool
	}{
		{&version, &old, true, false},
		{&version, &newer, false, false},
		{&version, nil, true, true},
		{nil, &version, false, true},
	} {
		err := checkCompatibility(candidate.controller, &unknownVersion, candidate.host, &otherVersion)
		fault := faultValue(err)
		if fault.Code != protocol.ErrorCodeConflict || fault.Details == nil || fault.Details.Kind != "version_mismatch" {
			t.Fatalf("compatibility fault: %#v", fault)
		}
		details, ok := fault.Details.Data.(protocol.FaultDetailsVersionMismatchPayload)
		if !ok || details.UpdateHost != candidate.updateHost || details.Unverified != candidate.unverified {
			t.Fatalf("compatibility details: %#v", fault.Details)
		}
	}
	expectedPlatform := map[string]string{"darwin": "macos", "linux": "linux", "windows": "windows"}[runtime.GOOS]
	if expectedPlatform == "" {
		expectedPlatform = runtime.GOOS
	}
	if endpoint := newFixtureLink(t).endpointReply(); endpoint.Platform != expectedPlatform {
		t.Fatalf("endpoint platform contract: %q", endpoint.Platform)
	}
}

type fixtureHandler struct {
	dispatch  func(context.Context, protocol.NodeID, protocol.Request) (Admission, error)
	subscribe func(context.Context, protocol.NodeID, protocol.Topic) (Subscription, error)
}

func (handler fixtureHandler) Dispatch(ctx context.Context, caller protocol.NodeID, request protocol.Request) (Admission, error) {
	return handler.dispatch(ctx, caller, request)
}

func (handler fixtureHandler) Subscribe(ctx context.Context, caller protocol.NodeID, topic protocol.Topic) (Subscription, error) {
	return handler.subscribe(ctx, caller, topic)
}

func (fixtureHandler) Open(context.Context, protocol.NodeID, protocol.StreamID) (Stream, error) {
	return nil, errors.New("fixture resource is unavailable")
}

func TestServerUsesCertifiedCallerAndOrdersReceipt(t *testing.T) {
	t.Parallel()
	link := newFixtureLink(t)
	caller, target := protocol.NodeID{2}, protocol.NodeID{1}
	link.trusted[caller] = true
	request := protocol.NewRequest(target, protocol.Command{Kind: "save_terminal_settings", Data: protocol.DefaultTerminalSettings()})
	var incoming bytes.Buffer
	if err := wire.Write(&incoming, link.envelope(wire.Call{Kind: wire.CallCommand, Request: &request}), false); err != nil {
		t.Fatal(err)
	}
	stream := newFixtureStream(incoming.Bytes())
	stream.stopOnce.Do(func() { close(stream.stopped) })
	link.handler = fixtureHandler{dispatch: func(ctx context.Context, certified protocol.NodeID, received protocol.Request) (Admission, error) {
		expectedJSON, expectedErr := json.Marshal(request)
		receivedJSON, receivedErr := json.Marshal(received)
		if certified != caller || expectedErr != nil || receivedErr != nil || !bytes.Equal(receivedJSON, expectedJSON) || ctx.Err() != nil {
			t.Error("authenticated caller or durable admission was changed")
		}
		response := make(chan Response, 1)
		response <- Response{Output: protocol.Output{Kind: "notifications_updated"}}
		return Admission{Receipt: protocol.Receipt{ID: received.ID, Durable: true}, Completion: response}, nil
	}}
	var compression atomic.Bool
	if err := link.serveRequest(t.Context(), caller, stream, make(chan struct{}, maxStreams-8), &compression); err != nil {
		t.Fatal(err)
	}
	receiver := bytes.NewReader(stream.written())
	reader := wire.NewReader(false)
	first, err := reader.Reply(receiver)
	if err != nil || first.Kind != wire.ReplyReceipt || first.Receipt.ID != request.ID || !first.Receipt.Durable {
		t.Fatalf("first response: %#v, %v", first, err)
	}
	second, err := reader.Reply(receiver)
	if err != nil || second.Kind != wire.ReplyCompleted || second.Completed.Output.Kind != "notifications_updated" {
		t.Fatalf("completion response: %#v, %v", second, err)
	}
	t.Run("snapshot_pages_have_separate_deadlines", func(t *testing.T) {
		link := newFixtureLink(t)
		// Each deadline lasts for exactly one complete frame.
		var mu sync.Mutex
		var expire context.CancelFunc
		deadlines, frames := 0, 0
		link.timeout = func(ctx context.Context, _ time.Duration) (context.Context, context.CancelFunc) {
			scoped, cancel := context.WithCancel(ctx)
			mu.Lock()
			expire = cancel
			deadlines++
			mu.Unlock()
			return scoped, cancel
		}
		stream := newFixtureStream(nil)
		stream.afterWrite = func(written []byte) {
			complete := 0
			for len(written) >= 4 {
				size := int(binary.BigEndian.Uint32(written))
				if len(written) < 4+size {
					break
				}
				written = written[4+size:]
				complete++
			}
			mu.Lock()
			defer mu.Unlock()
			if complete > frames {
				frames = complete
				expire()
			}
		}
		snapshot := protocol.DefaultSnapshot()
		snapshot.Node, snapshot.Cursor = target, 7
		batch := strings.Repeat("x", 2*wire.PageBytes+1)
		snapshot.ModelCatalog.Scores.Batch = &batch
		update := protocol.Update{Kind: "snapshot", Data: &snapshot}
		if err := link.writeReply(t.Context(), stream, wire.Reply{Kind: wire.ReplyUpdate, Update: &update}, false); err != nil {
			t.Fatalf("paged snapshot shared one frame deadline: %v", err)
		}
		reply, err := wire.NewReader(false).Reply(bytes.NewReader(stream.written()))
		if err != nil || reply.Kind != wire.ReplyUpdate || reply.Update.Kind != "snapshot" {
			t.Fatalf("paged snapshot: %#v, %v", reply.Kind, err)
		}
		received := reply.Update.Data.(*protocol.Snapshot)
		if received.Cursor != 7 || *received.ModelCatalog.Scores.Batch != batch || frames != 4 || deadlines != frames {
			t.Fatalf("snapshot used %d deadlines for %d frames", deadlines, frames)
		}
	})
}

func TestServerCancelsAbandonedSubscriptionPreparation(t *testing.T) {
	t.Parallel()
	link := newFixtureLink(t)
	caller := protocol.NodeID{2}
	link.trusted[caller] = true
	topic := protocol.Topic{Kind: "node"}
	var incoming bytes.Buffer
	if err := wire.Write(&incoming, link.envelope(wire.Call{Kind: wire.CallSubscribe, Topic: &topic}), false); err != nil {
		t.Fatal(err)
	}
	stream := newFixtureStream(incoming.Bytes())
	entered := make(chan struct{})
	stopped := make(chan struct{})
	link.handler = fixtureHandler{subscribe: func(ctx context.Context, certified protocol.NodeID, _ protocol.Topic) (Subscription, error) {
		if certified != caller {
			t.Error("subscription caller did not use its certified identity")
		}
		close(entered)
		<-ctx.Done()
		close(stopped)
		return nil, ctx.Err()
	}}
	permits := make(chan struct{}, 1)
	result := make(chan error, 1)
	var compression atomic.Bool
	go func() { result <- link.serveRequest(t.Context(), caller, stream, permits, &compression) }()
	<-entered
	stream.stopOnce.Do(func() { close(stream.stopped) })
	<-stopped
	if err := <-result; err != nil {
		t.Fatal(err)
	}
	if len(permits) != 0 {
		t.Fatal("abandoned preparation retained its subscription slot")
	}
}

func TestServerPreservesTransientRejection(t *testing.T) {
	t.Parallel()
	link := newFixtureLink(t)
	caller := protocol.NodeID{2}
	link.trusted[caller] = true
	request := protocol.NewRequest(protocol.NodeID{1}, protocol.Command{Kind: "snapshot"})
	var incoming bytes.Buffer
	if err := wire.Write(&incoming, link.envelope(wire.Call{Kind: wire.CallCommand, Request: &request}), false); err != nil {
		t.Fatal(err)
	}
	stream := newFixtureStream(incoming.Bytes())
	link.handler = fixtureHandler{dispatch: func(context.Context, protocol.NodeID, protocol.Request) (Admission, error) {
		return Admission{}, &protocol.Fault{Code: protocol.ErrorCodeNotFound, Message: "fixture unavailable"}
	}}
	var compression atomic.Bool
	if err := link.serveRequest(t.Context(), caller, stream, make(chan struct{}, maxStreams-8), &compression); err != nil {
		t.Fatal(err)
	}
	var reply wire.Reply
	if err := wire.Read(bytes.NewReader(stream.written()), &reply, false); err != nil {
		t.Fatal(err)
	}
	if reply.Kind != wire.ReplyRejected || reply.Rejected.Code != protocol.ErrorCodeNotFound {
		t.Fatalf("transient rejection: %#v", reply)
	}
	// A command this build does not declare, such as one removed from the
	// contract, never reaches the handler. While the request's identity
	// decodes the peer is answered with a rejection, as for any refusal before
	// admission; a frame whose identity does not decode is refused and the
	// stream closes without a reply.
	t.Run("undeclared_command_is_not_dispatched", func(t *testing.T) {
		link := newFixtureLink(t)
		link.trusted[caller] = true
		envelope, err := json.Marshal(link.envelope(wire.Call{Kind: wire.CallCommand, Request: &request}))
		if err != nil {
			t.Fatal(err)
		}
		declared := `"command":{"kind":"snapshot"}`
		identity := `"id":"` + request.ID.String() + `"`
		serve := func(t *testing.T, peer protocol.NodeID, old, replacement string) (error, []byte) {
			t.Helper()
			changed := bytes.Replace(envelope, []byte(old), []byte(replacement), 1)
			if bytes.Equal(changed, envelope) {
				t.Fatalf("envelope has no %s to replace: %s", old, envelope)
			}
			var incoming bytes.Buffer
			if err := wire.Write(&incoming, json.RawMessage(changed), false); err != nil {
				t.Fatal(err)
			}
			stream := newFixtureStream(incoming.Bytes())
			link.handler = fixtureHandler{dispatch: func(context.Context, protocol.NodeID, protocol.Request) (Admission, error) {
				t.Errorf("%s was dispatched", replacement)
				return Admission{}, nil
			}}
			var compression atomic.Bool
			err := link.serveRequest(t.Context(), peer, stream, make(chan struct{}, maxStreams-8), &compression)
			return err, stream.written()
		}
		rejection := func(t *testing.T, peer protocol.NodeID, old, replacement string) protocol.Fault {
			t.Helper()
			err, written := serve(t, peer, old, replacement)
			if err != nil {
				t.Fatalf("%s: %v", replacement, err)
			}
			reader := bytes.NewReader(written)
			var reply wire.Reply
			if err := wire.Read(reader, &reply, false); err != nil || reply.Kind != wire.ReplyRejected || reply.Rejected == nil || reader.Len() != 0 {
				t.Fatalf("%s: reply %#v, %v, %d bytes left", replacement, reply, err, reader.Len())
			}
			return *reply.Rejected
		}
		for _, command := range []string{
			`"command":{"kind":"set_defaults","data":{}}`,
			`"command":{"kind":"future_command","data":{}}`,
			`"command":{"kind":"future_command"}`,
			`"command":{"kind":"inspect_request","data":{"id":7}}`,
			`"command":{"kind":"snapshot","data":{}}`,
			`"command":7`,
		} {
			if fault := rejection(t, caller, declared, command); fault.Code != protocol.ErrorCodeInvalidRequest || fault.Message != "invalid command" {
				t.Fatalf("%s: %#v", command, fault)
			}
		}
		// The sender reads that rejection as a definite refusal, not as a
		// request that may have been admitted.
		_, written := serve(t, caller, declared, `"command":{"kind":"future_command","data":{}}`)
		sender := newFixtureStream(written)
		var refused *protocol.Fault
		if _, err := link.Remote(EndpointAddress{ID: protocol.NodeID{1}}).dispatchStream(t.Context(), request, sender, false); !errors.As(err, &refused) || refused.Code != protocol.ErrorCodeInvalidRequest {
			t.Fatalf("sender read %v", err)
		}
		// A peer that is no longer paired learns nothing about the command.
		if fault := rejection(t, protocol.NodeID{3}, declared, `"command":{"kind":"future_command","data":{}}`); fault.Code != protocol.ErrorCodePermissionDenied {
			t.Fatalf("unpaired peer: %#v", fault)
		}
		for _, change := range [][2]string{
			{identity, `"id":"not-an-identifier"`},
			{identity, `"id":7`},
			{identity + `,`, ``},
			{`"kind":"command"`, `"kind":"future_call"`},
			{declared, `"command":{"kind":"future_command"},"target":7`},
		} {
			err, written := serve(t, caller, change[0], change[1])
			if fault := faultValue(err); err == nil || fault.Code != protocol.ErrorCodeInvalidRequest || fault.Message != "invalid Link frame" || len(written) != 0 {
				t.Fatalf("%s: %v, %d bytes written", change[1], err, len(written))
			}
		}
	})
	t.Run("admitted_read_context_lives_through_completion", func(t *testing.T) {
		link := newFixtureLink(t)
		link.trusted[caller] = true
		stream := newFixtureStream(incoming.Bytes())
		continueRead := make(chan struct{})
		var release sync.Once
		stream.afterWrite = func(written []byte) {
			var reply wire.Reply
			if wire.Read(bytes.NewReader(written), &reply, false) == nil && reply.Kind == wire.ReplyReceipt {
				release.Do(func() { close(continueRead) })
			}
		}
		link.handler = fixtureHandler{dispatch: func(ctx context.Context, _ protocol.NodeID, received protocol.Request) (Admission, error) {
			completion := make(chan Response, 1)
			go func() {
				<-continueRead
				if ctx.Err() != nil {
					completion <- Response{Fault: &protocol.Fault{Code: protocol.ErrorCodeCancelled, Message: "fixture read context was released before completion"}}
				} else {
					completion <- Response{Output: protocol.Output{Kind: "notifications_updated"}}
				}
				close(completion)
			}()
			return Admission{Receipt: protocol.Receipt{ID: received.ID}, Completion: completion}, nil
		}}
		var compression atomic.Bool
		if err := link.serveRequest(t.Context(), caller, stream, make(chan struct{}, maxStreams-8), &compression); err != nil {
			t.Fatal(err)
		}
		reader := wire.NewReader(false)
		written := bytes.NewReader(stream.written())
		if reply, err := reader.Reply(written); err != nil || reply.Kind != wire.ReplyReceipt {
			t.Fatalf("transient receipt: %#v, %v", reply, err)
		}
		if reply, err := reader.Reply(written); err != nil || reply.Kind != wire.ReplyCompleted || reply.Completed.Fault != nil || reply.Completed.Output.Kind != "notifications_updated" {
			t.Fatalf("transient asynchronous completion: %#v, %v", reply, err)
		}
	})
}

func TestAddressMergeRetainsSupportedVariants(t *testing.T) {
	t.Parallel()
	first := EndpointAddress{ID: protocol.NodeID{1}, Addrs: []json.RawMessage{json.RawMessage(`{"Ip":"127.0.0.1:1"}`)}}
	second := EndpointAddress{ID: first.ID, Addrs: []json.RawMessage{
		json.RawMessage(`{"Relay":"https://relay.example/"}`),
		json.RawMessage(`{ "Ip": "127.0.0.1:1" }`),
		json.RawMessage(`{"Future":"retained"}`),
	}}
	merged := mergeAddress(first, second)
	if len(merged.Addrs) != 3 || !addressIncludes(merged, first) || addressIncludes(first, merged) {
		t.Fatalf("address union changed: %#v", merged)
	}
	selection := protocol.NetworkSelection{Kind: "custom", Data: protocol.NetworkSelectionCustomPayload{Relays: []string{"https://SELECTED.example:443"}}}
	candidate, err := CandidateAddress(merged, selection)
	if err != nil || !addressIncludes(candidate, merged) || len(candidate.Addrs) != 4 {
		t.Fatalf("uncertain network candidate dropped old hints: %#v, %v", candidate, err)
	}
	selected := []string{"https://selected.example/"}
	filtered, err := pairingAddressOnNetwork(candidate, selected)
	if err != nil || len(filtered.Addrs) != 2 || !addressIncludes(filtered, first) {
		t.Fatalf("selected pairing hints: %#v, %v", filtered, err)
	}
	if _, err := pairingAddressOnNetwork(second, selected); err == nil {
		t.Fatal("pairing on another relay network was accepted")
	}
	for _, relays := range [][]string{
		{}, {"http://relay.example"}, {"https://user:secret@relay.example"},
		{"https://relay.example/?token=x"}, {"https://relay.example/#secret"},
		{"https://relay.example:65536"},
	} {
		invalid := protocol.NetworkSelection{Kind: "custom", Data: protocol.NetworkSelectionCustomPayload{Relays: relays}}
		if _, err := NetworkRelayURLs(t.Context(), invalid); err == nil {
			t.Fatalf("invalid relay selection accepted: %#v", relays)
		}
	}
	for _, test := range []struct {
		name     string
		statuses []iroh.RelayStatus
		address  EndpointAddress
		ready    bool
		denied   bool
	}{
		{"ready", []iroh.RelayStatus{{URL: selected[0], Connected: true}}, filtered, true, false},
		{"waiting", []iroh.RelayStatus{{URL: selected[0]}}, filtered, false, false},
		{"old_advertisement", []iroh.RelayStatus{{URL: selected[0], Connected: true}}, candidate, false, false},
		{"authentication_denied", []iroh.RelayStatus{{URL: selected[0], AuthDeniedReason: "denied"}}, filtered, false, true},
	} {
		t.Run(test.name, func(t *testing.T) {
			ready, denied := selectedRelayReady(test.statuses, test.address, selected)
			if ready != test.ready || denied != test.denied {
				t.Fatalf("relay readiness: %v %v", ready, denied)
			}
		})
	}
}

func TestConnectionTelemetrySharesSampler(t *testing.T) {
	t.Parallel()
	ticks, started, stopped := make(chan time.Time), make(chan struct{}), make(chan struct{})
	var samples atomic.Int32
	rtt := uint64(9)
	initial := iroh.ConnectionInfo{Route: "p2p", RTTMS: &rtt}
	state := newConnectionTelemetry(t.Context(), selectedConnectionInfo(initial), func(context.Context) (iroh.ConnectionInfo, error) {
		samples.Add(1)
		return iroh.ConnectionInfo{Route: "relay", RTTMS: &rtt}, nil
	}, nil, func() (<-chan time.Time, func()) {
		close(started)
		return ticks, func() { close(stopped) }
	})
	<-started
	first, firstChanged, _ := state.snapshot()
	second, secondChanged, _ := state.snapshot()
	if firstChanged != secondChanged || !equalConnectionInfo(first, second) || samples.Load() != 0 {
		t.Fatal("subscribers created independent native samplers")
	}
	*first.Route, *first.RTTMS = Route("relay"), 100
	if *second.Route != Route("p2p") || *second.RTTMS != 9 {
		t.Fatal("a subscriber mutated shared connection state")
	}
	ticks <- time.Time{}
	<-firstChanged
	value, _, closed := state.snapshot()
	if closed || value.Route == nil || *value.Route != Route("relay") || value.RTTMS == nil || *value.RTTMS != 9 || samples.Load() != 1 {
		t.Fatalf("shared selected path and RTT: %#v", value)
	}
	state.close()
	<-state.done
	<-stopped
	value, _, closed = state.snapshot()
	if !closed || value.Route != nil || value.RTTMS != nil || samples.Load() != 1 {
		t.Fatal("closed connection retained telemetry or its sampler")
	}
	t.Run("idle_slot_cleanup_releases_cache_lock", func(t *testing.T) {
		link := newFixtureLink(t)
		var stale []*outgoingSlot
		for index := 1; index <= maxConnections; index++ {
			ctx, cancel := context.WithCancel(link.ctx)
			slot := &outgoingSlot{ctx: ctx, cancel: cancel}
			link.outgoing[protocol.NodeID{byte(index)}] = slot
			stale = append(stale, slot)
		}
		joined := make(chan struct{})
		stale[0].telemetry = &connectionTelemetry{
			changed: make(chan struct{}), done: joined,
			cancel: func() {
				if !link.mu.TryLock() {
					t.Error("eviction held the Link lock during telemetry cleanup")
				} else {
					link.mu.Unlock()
				}
				close(joined)
			},
		}
		slot, err := link.slot(t.Context(), protocol.NodeID{byte(maxConnections + 1)})
		if err != nil {
			t.Fatal(err)
		}
		link.releaseSlot(slot)
		if len(link.outgoing) != 1 {
			t.Fatal("idle closed slots retained connection capacity")
		}
		for _, slot := range stale {
			if slot.ctx.Err() == nil || slot.connection != nil || slot.telemetry != nil {
				t.Fatal("evicted slot retained its lifetime or connection owners")
			}
		}
	})
	t.Run("disconnect_drops_cached_connection", func(t *testing.T) {
		link := newFixtureLink(t)
		peer := protocol.NodeID{9}
		ctx, cancel := context.WithCancel(link.ctx)
		defer cancel()
		joined := make(chan struct{})
		slot := &outgoingSlot{ctx: ctx, cancel: cancel, dial: make(chan struct{}, 1)}
		slot.telemetry = &connectionTelemetry{changed: make(chan struct{}), done: joined, cancel: func() { close(joined) }}
		link.outgoing[peer] = slot
		link.Disconnect(protocol.NodeID{8})
		if slot.telemetry == nil {
			t.Fatal("disconnecting another peer dropped this connection")
		}
		link.Disconnect(peer)
		link.Disconnect(peer)
		if !channelClosed(joined) || slot.telemetry != nil || slot.connection != nil {
			t.Fatal("the cached connection outlived the disconnect")
		}
		// The peer stays paired and its slot stays usable: only the next use dials.
		if link.outgoing[peer] != slot || slot.ctx.Err() != nil {
			t.Fatal("disconnect removed the peer's slot")
		}
	})
}
