package hostctl

import (
	"bufio"
	"context"
	"encoding/json"
	"errors"
	"io"
	"net"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// pipeControlFixture keeps the real server framing and command decoder. Only
// the socket and Node implementation are replaced, so these tests own no profile.
type pipeControlFixture struct {
	dials    atomic.Int32
	finished chan error
	mu       sync.Mutex
	sockets  []net.Conn
	owners   sync.WaitGroup
	cancel   context.CancelFunc
	ctx      context.Context
	handler  func(context.Context, []byte, io.Writer) error
}

func newPipeControlFixture(t *testing.T, transport link.Transport, rewrite func(*incomingControl)) *pipeControlFixture {
	t.Helper()
	ctx, cancel := context.WithCancel(t.Context())
	fixture := &pipeControlFixture{ctx: ctx, cancel: cancel, finished: make(chan error, 16)}
	fixture.handler = func(ctx context.Context, line []byte, writer io.Writer) error {
		if rewrite != nil {
			writer = controlRewriteWriter{writer: writer, rewrite: rewrite}
		}
		return serveTransport(ctx, line, writer, transport)
	}
	t.Cleanup(func() {
		cancel()
		fixture.mu.Lock()
		for _, socket := range fixture.sockets {
			_ = socket.Close()
		}
		fixture.mu.Unlock()
		fixture.owners.Wait()
	})
	return fixture
}

func (fixture *pipeControlFixture) dial(ctx context.Context) (net.Conn, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	controller, server := net.Pipe()
	fixture.dials.Add(1)
	fixture.mu.Lock()
	fixture.sockets = append(fixture.sockets, controller, server)
	fixture.owners.Add(1)
	fixture.mu.Unlock()
	go func() {
		defer fixture.owners.Done()
		defer server.Close()
		ctx, cancel := context.WithCancel(fixture.ctx)
		defer cancel()
		stop := context.AfterFunc(ctx, func() { _ = server.Close() })
		defer stop()
		reader := bufio.NewReader(server)
		line, err := readLine(reader, protocol.MAX_FRAME_BYTES)
		if err != nil {
			fixture.finished <- err
			return
		}
		disconnected := make(chan struct{})
		go func() {
			defer close(disconnected)
			var extra [1]byte
			_, _ = reader.Read(extra[:])
			cancel()
		}()
		err = fixture.handler(ctx, line, server)
		cancel()
		_ = server.Close()
		<-disconnected
		fixture.finished <- err
	}()
	return controller, nil
}

func (fixture *pipeControlFixture) awaitFinished(t *testing.T, ctx context.Context, count int) {
	t.Helper()
	for range count {
		select {
		case <-fixture.finished:
		case <-ctx.Done():
			t.Fatal("Host control fixture did not release its connection")
		}
	}
}

type controlRewriteWriter struct {
	writer  io.Writer
	rewrite func(*incomingControl)
}

func (writer controlRewriteWriter) Write(line []byte) (int, error) {
	var frame incomingControl
	if err := json.Unmarshal(line, &frame); err != nil {
		return 0, err
	}
	writer.rewrite(&frame)
	if err := writeLine(writer.writer, frame, protocol.MAX_FRAME_BYTES); err != nil {
		return 0, err
	}
	return len(line), nil
}

func TestControlTransportTypedDispatch(t *testing.T) {
	t.Parallel()
	ctx, cancel := context.WithTimeout(t.Context(), 5*time.Second)
	defer cancel()
	input := protocol.PowerSettings{Revision: 7, PreventSleep: false}
	expected := protocol.PowerSettings{Revision: 8, PreventSleep: false}
	seen := make(chan protocol.Request, 1)
	transport := &fixtureTransport{dispatch: func(_ context.Context, request protocol.Request) (link.Admission, error) {
		seen <- request
		return fixtureAdmission(request, protocol.Output{Kind: "power_settings", Data: expected}), nil
	}}
	fixture := newPipeControlFixture(t, transport, nil)
	controller, err := connectTransport(ctx, fixture.dial)
	if err != nil {
		t.Fatal(err)
	}
	request := protocol.NewRequest(controller.Target(), protocol.Command{Kind: "save_power_settings", Data: input})
	admission, err := controller.Dispatch(ctx, request)
	if err != nil {
		t.Fatal(err)
	}
	if admission.Receipt.ID != request.ID || !admission.Receipt.Durable {
		t.Fatalf("admission lost its durable request identity: %#v", admission.Receipt)
	}
	select {
	case received := <-seen:
		settings, ok := received.Command.Data.(*protocol.PowerSettings)
		if received.ID != request.ID || received.Target != request.Target || received.Command.Kind != request.Command.Kind || !ok || *settings != input {
			t.Fatalf("server did not decode the original typed request: %#v", received)
		}
	case <-ctx.Done():
		t.Fatal("Host did not receive the request")
	}
	select {
	case completed, open := <-admission.Completion:
		settings, ok := completed.Output.Data.(*protocol.PowerSettings)
		if !open || completed.Fault != nil || completed.Output.Kind != "power_settings" || !ok || *settings != expected {
			t.Fatalf("completion lost its typed output: %#v", completed)
		}
	case <-ctx.Done():
		t.Fatal("Host did not complete the request")
	}
	fixture.awaitFinished(t, ctx, 2)
	if fixture.dials.Load() != 2 {
		t.Fatal("control transport reconnected while completing a request")
	}
}

func TestControlTransportRejectsUntrustedAdmissionWithoutReplay(t *testing.T) {
	t.Parallel()
	for _, name := range []string{"receipt_id", "receipt_durable", "receipt_missing", "node", "version", "failed_node", "failed_version"} {
		t.Run(name, func(t *testing.T) {
			ctx, cancel := context.WithTimeout(t.Context(), 5*time.Second)
			defer cancel()
			var calls atomic.Int32
			transport := &fixtureTransport{dispatch: func(_ context.Context, request protocol.Request) (link.Admission, error) {
				calls.Add(1)
				return fixtureAdmission(request, protocol.Output{Kind: "power_settings", Data: protocol.DefaultPowerSettings()}), nil
			}}
			fixture := newPipeControlFixture(t, transport, func(frame *incomingControl) {
				if frame.Body.Kind != "admitted" {
					return
				}
				switch name {
				case "receipt_id":
					frame.Body.Data = json.RawMessage(`{"id":"00000000-0000-0000-0000-000000000002","durable":true}`)
				case "receipt_durable":
					var receipt protocol.Receipt
					_ = json.Unmarshal(frame.Body.Data, &receipt)
					receipt.Durable = false
					frame.Body.Data, _ = json.Marshal(receipt)
				case "receipt_missing":
					frame.Body.Data = json.RawMessage(`{}`)
				case "node":
					frame.Node = protocol.NodeID{2}
				case "version":
					frame.Version = 2
				case "failed_node", "failed_version":
					frame.Body.Kind = "failed"
					frame.Body.Data, _ = json.Marshal(protocol.Fault{Code: protocol.ErrorCodeConflict, Message: "Untrusted fixture fault"})
					if name == "failed_node" {
						frame.Node = protocol.NodeID{2}
					} else {
						frame.Version = 2
					}
				}
			})
			controller, err := connectTransport(ctx, fixture.dial)
			if err != nil {
				t.Fatal(err)
			}
			request := protocol.NewRequest(controller.Target(), protocol.Command{Kind: "save_power_settings", Data: protocol.DefaultPowerSettings()})
			admission, err := controller.Dispatch(ctx, request)
			var fault *link.FaultError
			if !errors.As(err, &fault) || fault.Fault.Code != protocol.ErrorCodeOutcomeUnknown || admission.Completion != nil {
				t.Fatalf("untrusted admission was not uncertain: %#v, %v", admission, err)
			}
			fixture.awaitFinished(t, ctx, 2)
			if calls.Load() != 1 || fixture.dials.Load() != 2 {
				t.Fatalf("uncertain admission was replayed: calls=%d dials=%d", calls.Load(), fixture.dials.Load())
			}
		})
	}
}

func TestControlTransportRejectsUntrustedCompletionWithoutReplay(t *testing.T) {
	t.Parallel()
	for _, name := range []string{"node", "version", "result"} {
		t.Run(name, func(t *testing.T) {
			ctx, cancel := context.WithTimeout(t.Context(), 5*time.Second)
			defer cancel()
			var calls atomic.Int32
			transport := &fixtureTransport{dispatch: func(_ context.Context, request protocol.Request) (link.Admission, error) {
				calls.Add(1)
				return fixtureAdmission(request, protocol.Output{Kind: "power_settings", Data: protocol.DefaultPowerSettings()}), nil
			}}
			fixture := newPipeControlFixture(t, transport, func(frame *incomingControl) {
				if frame.Body.Kind != "completed" {
					return
				}
				switch name {
				case "node":
					frame.Node = protocol.NodeID{2}
				case "version":
					frame.Version = 2
				case "result":
					frame.Body.Data = json.RawMessage(`{}`)
				}
			})
			controller, err := connectTransport(ctx, fixture.dial)
			if err != nil {
				t.Fatal(err)
			}
			request := protocol.NewRequest(controller.Target(), protocol.Command{Kind: "save_power_settings", Data: protocol.DefaultPowerSettings()})
			admission, err := controller.Dispatch(ctx, request)
			if err != nil {
				t.Fatal(err)
			}
			select {
			case completed, open := <-admission.Completion:
				if !open || completed.Fault == nil || completed.Fault.Code != protocol.ErrorCodeOutcomeUnknown {
					t.Fatalf("untrusted completion was not uncertain: %#v", completed)
				}
			case <-ctx.Done():
				t.Fatal("uncertain completion did not settle")
			}
			fixture.awaitFinished(t, ctx, 2)
			if calls.Load() != 1 || fixture.dials.Load() != 2 {
				t.Fatalf("uncertain completion was replayed: calls=%d dials=%d", calls.Load(), fixture.dials.Load())
			}
		})
	}
}

type pipeSubscriptionFixture struct {
	updates chan protocol.Update
	closed  chan struct{}
	once    sync.Once
}

func (subscription *pipeSubscriptionFixture) Next(ctx context.Context) (protocol.Update, error) {
	select {
	case update := <-subscription.updates:
		return update, nil
	case <-subscription.closed:
		return protocol.Update{}, io.EOF
	case <-ctx.Done():
		return protocol.Update{}, ctx.Err()
	}
}

func (subscription *pipeSubscriptionFixture) Close() error {
	subscription.once.Do(func() { close(subscription.closed) })
	return nil
}

func TestControlSubscriptionOwnsItsObservationContext(t *testing.T) {
	t.Parallel()
	ctx, cancel := context.WithTimeout(t.Context(), 5*time.Second)
	defer cancel()
	backend := &pipeSubscriptionFixture{updates: make(chan protocol.Update, 1), closed: make(chan struct{})}
	seen := make(chan protocol.Topic, 1)
	transport := &fixtureTransport{subscribe: func(_ context.Context, topic protocol.Topic) (link.Subscription, error) {
		seen <- topic
		return backend, nil
	}}
	fixture := newPipeControlFixture(t, transport, nil)
	controller, err := connectTransport(ctx, fixture.dial)
	if err != nil {
		t.Fatal(err)
	}
	establish, release := context.WithCancel(ctx)
	subscription, err := controller.Subscribe(establish, protocol.Topic{Kind: "node"})
	if err != nil {
		release()
		t.Fatal(err)
	}
	defer subscription.Close()
	release()
	if topic := <-seen; topic.Kind != "node" {
		t.Fatalf("subscription changed its topic: %#v", topic)
	}
	snapshot := protocol.DefaultSnapshot()
	snapshot.Node = controller.Target()
	backend.updates <- protocol.Update{Kind: "snapshot", Data: snapshot}
	update, err := subscription.Next(ctx)
	if err != nil {
		t.Fatalf("canceling establishment closed the observation: %v", err)
	}
	typed, ok := update.Data.(*protocol.Snapshot)
	if update.Kind != "snapshot" || !ok || typed.Node != controller.Target() {
		t.Fatalf("observation lost its typed update: %#v", update)
	}
	observe, stop := context.WithCancel(ctx)
	finished := make(chan error, 1)
	go func() {
		_, err := subscription.Next(observe)
		finished <- err
	}()
	stop()
	select {
	case err := <-finished:
		if !errors.Is(err, context.Canceled) {
			t.Fatalf("Next cancellation returned %v", err)
		}
	case <-ctx.Done():
		t.Fatal("canceling Next did not interrupt its socket read")
	}
	if first, second := subscription.Close(), subscription.Close(); first != nil || second != nil {
		t.Fatalf("Close was not idempotent: %v, %v", first, second)
	}
	select {
	case <-backend.closed:
	case <-ctx.Done():
		t.Fatal("controller close did not release the server subscription")
	}
	fixture.awaitFinished(t, ctx, 2)
}

func TestControlTransportRejectsWrongRequestTargetBeforeDial(t *testing.T) {
	t.Parallel()
	ctx, cancel := context.WithTimeout(t.Context(), 5*time.Second)
	defer cancel()
	fixture := newPipeControlFixture(t, &fixtureTransport{dispatch: func(context.Context, protocol.Request) (link.Admission, error) {
		return link.Admission{}, errors.New("unexpected request reached the fixture")
	}}, nil)
	controller, err := connectTransport(ctx, fixture.dial)
	if err != nil {
		t.Fatal(err)
	}
	request := protocol.NewRequest(protocol.NodeID{2}, protocol.Command{Kind: "read_power_settings"})
	_, err = controller.Dispatch(ctx, request)
	var fault *link.FaultError
	if !errors.As(err, &fault) || fault.Fault.Code != protocol.ErrorCodeWrongTarget || fixture.dials.Load() != 1 {
		t.Fatalf("wrong target reached a Host connection: dials=%d err=%v", fixture.dials.Load(), err)
	}
	fixture.awaitFinished(t, ctx, 1)
}

func TestControlTransportRejectsInvalidHello(t *testing.T) {
	t.Parallel()
	for _, name := range []string{"empty_node", "version"} {
		t.Run(name, func(t *testing.T) {
			ctx, cancel := context.WithTimeout(t.Context(), 5*time.Second)
			defer cancel()
			fixture := newPipeControlFixture(t, &fixtureTransport{}, func(frame *incomingControl) {
				if name == "empty_node" {
					frame.Node = protocol.NodeID{}
				} else {
					frame.Version = 2
				}
			})
			if _, err := connectTransport(ctx, fixture.dial); err == nil {
				t.Fatal("an invalid Host hello established a transport")
			}
			fixture.awaitFinished(t, ctx, 1)
		})
	}
}

func TestControlTransportPreservesTrustedAdmissionFault(t *testing.T) {
	t.Parallel()
	ctx, cancel := context.WithTimeout(t.Context(), 5*time.Second)
	defer cancel()
	var calls atomic.Int32
	transport := &fixtureTransport{dispatch: func(context.Context, protocol.Request) (link.Admission, error) {
		calls.Add(1)
		return link.Admission{}, &protocol.Fault{Code: protocol.ErrorCodeConflict, Message: "Fixture revision conflict"}
	}}
	fixture := newPipeControlFixture(t, transport, nil)
	controller, err := connectTransport(ctx, fixture.dial)
	if err != nil {
		t.Fatal(err)
	}
	request := protocol.NewRequest(controller.Target(), protocol.Command{Kind: "save_power_settings", Data: protocol.DefaultPowerSettings()})
	_, err = controller.Dispatch(ctx, request)
	var fault *link.FaultError
	if !errors.As(err, &fault) || fault.Fault.Code != protocol.ErrorCodeConflict || fault.Fault.Message != "Fixture revision conflict" {
		t.Fatalf("a validated Node fault became uncertain: %v", err)
	}
	fixture.awaitFinished(t, ctx, 2)
	if calls.Load() != 1 || fixture.dials.Load() != 2 {
		t.Fatal("a rejected request was replayed")
	}
}

func TestControlTransportCancellationKeepsAdmittedOutcomeUnknown(t *testing.T) {
	t.Parallel()
	ctx, cancel := context.WithTimeout(t.Context(), 5*time.Second)
	defer cancel()
	var calls atomic.Int32
	transport := &fixtureTransport{dispatch: func(_ context.Context, request protocol.Request) (link.Admission, error) {
		calls.Add(1)
		return link.Admission{Receipt: protocol.Receipt{ID: request.ID, Durable: true}, Completion: make(chan link.Response)}, nil
	}}
	fixture := newPipeControlFixture(t, transport, nil)
	controller, err := connectTransport(ctx, fixture.dial)
	if err != nil {
		t.Fatal(err)
	}
	observe, stop := context.WithCancel(ctx)
	defer stop()
	request := protocol.NewRequest(controller.Target(), protocol.Command{Kind: "save_power_settings", Data: protocol.DefaultPowerSettings()})
	admission, err := controller.Dispatch(observe, request)
	if err != nil {
		t.Fatal(err)
	}
	stop()
	select {
	case result, open := <-admission.Completion:
		if !open || result.Fault == nil || result.Fault.Code != protocol.ErrorCodeOutcomeUnknown {
			t.Fatalf("cancellation claimed an admitted request's business outcome: %#v", result)
		}
	case <-ctx.Done():
		t.Fatal("cancellation did not release completion observation")
	}
	fixture.awaitFinished(t, ctx, 2)
	if calls.Load() != 1 || fixture.dials.Load() != 2 {
		t.Fatal("cancellation replayed the admitted request")
	}
}
