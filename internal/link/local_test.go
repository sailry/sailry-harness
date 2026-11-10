package link

import (
	"bytes"
	"context"
	"io"
	"reflect"
	"sync"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

type localHandler struct {
	stream   Stream
	resource protocol.StreamID
	caller   protocol.NodeID
	request  protocol.Request
	topic    protocol.Topic
	result   chan Response
}

func (handler *localHandler) Dispatch(_ context.Context, caller protocol.NodeID, request protocol.Request) (Admission, error) {
	handler.caller, handler.request = caller, request
	return Admission{Receipt: protocol.Receipt{ID: request.ID, Durable: request.Command.Durable()}, Completion: handler.result}, nil
}

func (handler *localHandler) Subscribe(_ context.Context, caller protocol.NodeID, topic protocol.Topic) (Subscription, error) {
	handler.caller, handler.topic = caller, topic
	return &localSubscription{update: protocol.Update{Kind: "snapshot", Data: protocol.DefaultSnapshot()}, closed: make(chan struct{})}, nil
}

type localSubscription struct {
	update protocol.Update
	closed chan struct{}
	once   sync.Once
}

func (subscription *localSubscription) Next(ctx context.Context) (protocol.Update, error) {
	select {
	case <-ctx.Done():
		return protocol.Update{}, ctx.Err()
	case <-subscription.closed:
		return protocol.Update{}, context.Canceled
	default:
		return subscription.update, nil
	}
}

func (subscription *localSubscription) Close() error {
	subscription.once.Do(func() { close(subscription.closed) })
	return nil
}

func TestLocalTransportContract(t *testing.T) {
	t.Parallel()
	node, caller := protocol.NodeID{1}, protocol.NodeID{2}
	t.Run("resource_half_close", func(t *testing.T) {
		client, server := StreamPair()
		defer client.Close()
		defer server.Close()
		resource := protocol.NewStreamID()
		handler := &localHandler{stream: client}
		transport := NewLocal(node, caller, handler)
		opened, err := transport.Open(t.Context(), resource)
		if err != nil {
			t.Fatal(err)
		}
		if handler.caller != caller || handler.resource != resource {
			t.Fatal("stream lost bound caller or resource")
		}
		done := make(chan error, 1)
		go func() {
			_, err := opened.Write([]byte("upload"))
			if err == nil {
				err = opened.CloseWrite()
			}
			if err == nil {
				var reply []byte
				reply, err = io.ReadAll(opened)
				if err == nil && !bytes.Equal(reply, []byte{1}) {
					err = io.ErrUnexpectedEOF
				}
			}
			done <- err
		}()
		data, err := io.ReadAll(server)
		if err != nil || string(data) != "upload" {
			t.Fatalf("upload bytes: %q, %v", data, err)
		}
		if _, err = server.Write([]byte{1}); err != nil {
			t.Fatal(err)
		}
		if err = server.CloseWrite(); err != nil {
			t.Fatal(err)
		}
		if err = <-done; err != nil {
			t.Fatal(err)
		}
	})
	for _, kind := range []string{
		"snapshot", "inspect_request", "create_project", "update_project", "remove_project", "register_worktree", "set_defaults",
		"create_session", "rename_session", "set_session_read", "set_session_archived", "archive_sessions", "remove_session", "remove_sessions",
		"set_session_order", "read_session", "publish_notification", "mark_notification_read", "mark_notifications_read",
		"dismiss_read_notifications", "dismiss_notifications", "read_power_settings", "save_power_settings", "read_telemetry_settings", "save_telemetry_settings",
	} {
		t.Run(kind, func(t *testing.T) {
			handler := &localHandler{result: make(chan Response, 1)}
			transport := NewLocal(node, caller, handler)
			request := protocol.NewRequest(node, protocol.Command{Kind: kind})
			admission, err := transport.Dispatch(context.Background(), request)
			if err != nil {
				t.Fatal(err)
			}
			if transport.Target() != node || handler.caller != caller || !reflect.DeepEqual(handler.request, request) {
				t.Fatal("local route changed the captured target, caller or request")
			}
			if admission.Receipt.ID != request.ID || admission.Receipt.Durable != request.Command.Durable() {
				t.Fatal("local route changed the Node receipt")
			}
			select {
			case <-admission.Completion:
				t.Fatal("receipt manufactured a command outcome")
			default:
			}
			want := Response{Output: protocol.Output{Kind: "notifications_updated"}}
			handler.result <- want
			if got := <-admission.Completion; !reflect.DeepEqual(got, want) {
				t.Fatal("local route changed command completion")
			}
			subscription, err := transport.Subscribe(context.Background(), protocol.Topic{Kind: "node"})
			if err != nil {
				t.Fatal(err)
			}
			if handler.caller != caller || handler.topic.Kind != "node" {
				t.Fatal("subscription lost caller identity or topic")
			}
			if update, err := subscription.Next(context.Background()); err != nil || update.Kind != "snapshot" {
				t.Fatalf("initial subscription update: %v, %v", update, err)
			}
			if err := subscription.Close(); err != nil {
				t.Fatal(err)
			}
		})
	}
}

func (handler *localHandler) Open(_ context.Context, caller protocol.NodeID, id protocol.StreamID) (Stream, error) {
	handler.caller, handler.resource = caller, id
	return handler.stream, nil
}
