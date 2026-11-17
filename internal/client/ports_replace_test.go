package client

import (
	"bufio"
	"context"
	"fmt"
	"io"
	"net"
	"strconv"
	"sync"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// The fixture transport supplies isolated byte streams instead of dialing a
// real execution Node. Only the controller's local TCP listener is exercised.
func replacementPortsClient(t *testing.T) *Client {
	t.Helper()
	var mu sync.Mutex
	ports := map[protocol.StreamID]uint16{}
	transport := &clientTransport{}
	for range 8 {
		transport.subscriptions = append(transport.subscriptions, testSubscription(protocol.Snapshot{}))
	}
	transport.dispatch = func(request protocol.Request) (link.Admission, error) {
		input, err := payload[protocol.CommandOpenPortPayload](request.Command.Data)
		if request.Command.Kind != "open_port" || err != nil {
			return link.Admission{}, fmt.Errorf("unexpected command %q", request.Command.Kind)
		}
		if input.Port == 999 {
			return link.Admission{}, fault(protocol.ErrorCodeUnavailable, "fixture remote port is unavailable")
		}
		id := protocol.NewStreamID()
		mu.Lock()
		ports[id] = input.Port
		mu.Unlock()
		completion := make(chan link.Response, 1)
		completion <- link.Response{Output: protocol.Output{Kind: "port_stream", Data: protocol.OutputPortStreamPayload{Stream: id}}}
		return link.Admission{Receipt: protocol.Receipt{ID: request.ID}, Completion: completion}, nil
	}
	transport.open = func(_ context.Context, id protocol.StreamID) (link.Stream, error) {
		mu.Lock()
		port, ok := ports[id]
		delete(ports, id)
		mu.Unlock()
		if !ok {
			return nil, fmt.Errorf("unknown fixture port stream")
		}
		local, remote := link.StreamPair()
		go func() {
			defer remote.Close()
			reader := bufio.NewReader(remote)
			for {
				line, err := reader.ReadString('\n')
				if err != nil {
					return
				}
				if _, err := fmt.Fprintf(remote, "%d:%s", port, line); err != nil {
					return
				}
			}
		}()
		return local, nil
	}
	controller := New(liveTransport{transport})
	t.Cleanup(func() { _ = controller.Close() })
	return controller
}

func replacementConnection(t *testing.T, port uint16) (net.Conn, *bufio.Reader) {
	t.Helper()
	connection, err := net.Dial("tcp4", net.JoinHostPort("127.0.0.1", strconv.Itoa(int(port))))
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = connection.Close() })
	if err := connection.SetDeadline(time.Now().Add(5 * time.Second)); err != nil {
		t.Fatal(err)
	}
	return connection, bufio.NewReader(connection)
}

func replacementReply(t *testing.T, writer io.Writer, reader *bufio.Reader, remote uint16) {
	t.Helper()
	if _, err := io.WriteString(writer, "fixture\n"); err != nil {
		t.Fatal(err)
	}
	got, err := reader.ReadString('\n')
	if err != nil || got != fmt.Sprintf("%d:fixture\n", remote) {
		t.Fatalf("forwarded reply = %q, %v; want remote %d", got, err, remote)
	}
}

func TestReplaceForwardPort(t *testing.T) {
	t.Run("remote_only_keeps_listener_and_existing_stream", func(t *testing.T) {
		controller := replacementPortsClient(t)
		forward, err := controller.ForwardPort(t.Context(), 8080, 0)
		if err != nil {
			t.Fatal(err)
		}
		port := forward.LocalPort()
		existing, before := replacementConnection(t, port)
		replacementReply(t, existing, before, 8080)
		service, stopService := context.WithCancel(t.Context())
		forward.serviceCancel = stopService
		updated, err := controller.ReplaceForwardPort(t.Context(), forward, 8081, port)
		if err != nil || updated != forward || updated.LocalPort() != port || updated.RemotePort() != 8081 || service.Err() == nil {
			t.Fatalf("same-listener replacement = %p, %v; original %p", updated, err, forward)
		}
		replacementReply(t, existing, before, 8080)
		connection, reader := replacementConnection(t, port)
		replacementReply(t, connection, reader, 8081)
	})
	t.Run("failed_remote_keeps_mapping", func(t *testing.T) {
		controller := replacementPortsClient(t)
		forward, err := controller.ForwardPort(t.Context(), 8080, 0)
		if err != nil {
			t.Fatal(err)
		}
		service, stopService := context.WithCancel(t.Context())
		defer stopService()
		forward.serviceCancel = stopService
		if updated, err := controller.ReplaceForwardPort(t.Context(), forward, 999, forward.LocalPort()); err == nil || updated != nil {
			t.Fatalf("unavailable replacement = %v, %v", updated, err)
		}
		if forward.RemotePort() != 8080 || forward.State().Kind != "listening" || service.Err() != nil {
			t.Fatal("failed remote preparation changed the original mapping or source watch")
		}
		connection, reader := replacementConnection(t, forward.LocalPort())
		replacementReply(t, connection, reader, 8080)
	})
	t.Run("failed_local_bind_keeps_mapping", func(t *testing.T) {
		controller := replacementPortsClient(t)
		forward, err := controller.ForwardPort(t.Context(), 8080, 0)
		if err != nil {
			t.Fatal(err)
		}
		occupied, err := net.Listen("tcp4", "127.0.0.1:0")
		if err != nil {
			t.Fatal(err)
		}
		defer occupied.Close()
		local := uint16(occupied.Addr().(*net.TCPAddr).Port)
		if updated, err := controller.ReplaceForwardPort(t.Context(), forward, 8080, local); err == nil || updated != nil {
			t.Fatalf("occupied replacement = %v, %v", updated, err)
		}
		if forward.State().Kind != "listening" || forward.ctx.Err() != nil {
			t.Fatal("failed local preparation retired the original forward")
		}
		connection, reader := replacementConnection(t, forward.LocalPort())
		replacementReply(t, connection, reader, 8080)
	})
	t.Run("local_move_prepares_listener_before_retiring_old", func(t *testing.T) {
		controller := replacementPortsClient(t)
		forward, err := controller.ForwardPort(t.Context(), 8080, 0)
		if err != nil {
			t.Fatal(err)
		}
		updated, err := controller.ReplaceForwardPort(t.Context(), forward, 8081, 0)
		if err != nil || updated == forward || updated.LocalPort() == forward.LocalPort() || forward.State().Kind != "closed" {
			t.Fatalf("moved replacement = %v, %v; previous %+v", updated, err, forward.State())
		}
		connection, reader := replacementConnection(t, updated.LocalPort())
		replacementReply(t, connection, reader, 8081)
	})
	t.Run("foreign_client_cannot_replace_mapping", func(t *testing.T) {
		controller := replacementPortsClient(t)
		foreign := replacementPortsClient(t)
		forward, err := controller.ForwardPort(t.Context(), 8080, 0)
		if err != nil {
			t.Fatal(err)
		}
		if updated, err := foreign.ReplaceForwardPort(t.Context(), forward, 8081, forward.LocalPort()); err == nil || updated != nil {
			t.Fatalf("foreign replacement = %v, %v", updated, err)
		}
		if forward.RemotePort() != 8080 || forward.State().Kind != "listening" {
			t.Fatal("foreign Client changed the original mapping")
		}
	})
}
