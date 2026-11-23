package live

import (
	"context"
	"fmt"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type serviceTestTransport struct {
	conversationTestTransport
	commands protocol.UpdateCommandsPayload
}

func (transport *serviceTestTransport) Subscribe(ctx context.Context, topic protocol.Topic) (link.Subscription, error) {
	if topic.Kind != "commands" {
		return transport.conversationTestTransport.Subscribe(ctx, topic)
	}
	sub := &conversationTestSubscription{updates: make(chan protocol.Update, 1), closed: make(chan struct{})}
	sub.updates <- protocol.Update{Kind: "commands", Data: transport.commands}
	return sub, nil
}

func (*serviceTestTransport) Open(context.Context, protocol.StreamID) (link.Stream, error) {
	local, peer := link.StreamPair()
	_ = peer.Close()
	return local, nil
}

func TestComposerServiceOpensOnItsHost(t *testing.T) {
	ctx, cancel := context.WithTimeout(t.Context(), 5*time.Second)
	defer cancel()
	session := protocol.DefaultSession()
	session.ID = protocol.SessionID{1}
	service := protocol.ProcessService{Port: 5179, Url: "https://127.0.0.1:5179/app?token=fixture#route"}
	command := protocol.DefaultProcessInfo()
	command.ID, command.Session = protocol.RequestID{1}, session.ID
	command.Status = protocol.ProcessStatus{Kind: "running"}
	command.Services = []protocol.ProcessService{service}

	bind := func(node protocol.NodeID) *Runtime {
		t.Helper()
		snapshot := protocol.DefaultSnapshot()
		snapshot.Node, snapshot.Sessions = node, []protocol.Session{session}
		transport := &serviceTestTransport{
			conversationTestTransport: conversationTestTransport{snapshot: snapshot},
			commands:                  protocol.UpdateCommandsPayload{Node: node, Session: session.ID, Items: []protocol.ProcessInfo{command}},
		}
		transport.handle = func(request protocol.Request) (link.Response, bool) {
			switch request.Command.Kind {
			case "list_commands":
				return link.Response{Output: protocol.Output{Kind: "commands", Data: transport.commands.Items}}, true
			case "open_port":
				if request.Command.Data.(protocol.CommandOpenPortPayload).Port == service.Port {
					return link.Response{Output: protocol.Output{Kind: "port_stream", Data: protocol.OutputPortStreamPayload{Stream: protocol.StreamID{1}}}}, true
				}
			}
			return link.Response{}, false
		}
		controller := client.New(transport)
		t.Cleanup(func() { _ = controller.Close() })
		if err := controller.Start(ctx); err != nil {
			t.Fatal(err)
		}
		ready := make(chan struct{}, 1)
		release, err := controller.ObserveCommands(ctx, session.ID, func() {
			select {
			case ready <- struct{}{}:
			default:
			}
		})
		if err != nil {
			t.Fatal(err)
		}
		t.Cleanup(release)
		select {
		case <-ready:
		case <-ctx.Done():
			t.Fatalf("commands for %s: %v", node, ctx.Err())
		}
		return &Runtime{ctx: ctx, controller: controller}
	}

	localNode, remoteNode := protocol.NodeID{1}, protocol.NodeID{2}
	local, remote := bind(localNode), bind(remoteNode)
	local.initPorts()
	t.Cleanup(local.closePorts)
	remote.parent = local
	local.bindings = map[string]*Runtime{remoteNode.String(): remote}
	var opened string
	local.SetOpenService(func(address string) { opened = address })
	click := func(owner *Runtime) {
		actions := &conversationActions{r: owner, cv: &viewmodel.Conversation{ID: session.ID.String(), Node: owner.controller.Target().String()}}
		actions.OpenService(int(service.Port))
		local.workers.Wait()
	}

	click(local)
	if forward, busy := local.forward(localNode.String(), int(service.Port)); forward != nil || busy {
		t.Fatal("opening a local service started a port forward")
	}
	if opened != service.Url {
		t.Fatalf("opening the local service = %q, want %q", opened, service.Url)
	}

	click(remote)
	forward, busy := local.forward(remoteNode.String(), int(service.Port))
	if forward == nil || busy || forward.State().Kind != "listening" {
		t.Fatal("opening a remote service did not establish a local listener")
	}
	if len(local.queue) != 1 {
		t.Fatalf("remote open queued %d callbacks, want 1", len(local.queue))
	}
	local.queue[0](nil)
	if want := fmt.Sprintf("https://127.0.0.1:%d/app?token=fixture#route", forward.LocalPort()); opened != want {
		t.Fatalf("opening the remote service = %q, want %q", opened, want)
	}
}

func TestLoopbackServiceURL(t *testing.T) {
	t.Parallel()
	for name, test := range map[string]struct {
		announced string
		port      uint16
		want      string
	}{
		"token query and path survive":    {"http://localhost:8888/lab?token=fixture", 49152, "http://127.0.0.1:49152/lab?token=fixture"},
		"https stays https":               {"https://localhost:3000", 49153, "https://127.0.0.1:49153"},
		"another loopback host is local":  {"http://[::1]:5173/app/#/route", 49154, "http://127.0.0.1:49154/app/#/route"},
		"the announced port is replaced":  {"http://127.0.0.1:8080/", 8080, "http://127.0.0.1:8080/"},
		"a manual forward opens its root": {"", 49155, "http://127.0.0.1:49155"},
		"an unparsed URL opens the root":  {"://broken", 49156, "http://127.0.0.1:49156"},
		"another scheme opens the root":   {"ws://localhost:9000/socket", 49157, "http://127.0.0.1:49157"},
	} {
		t.Run(name, func(t *testing.T) {
			t.Parallel()
			if got := loopbackServiceURL(test.announced, test.port); got != test.want {
				t.Fatalf("loopbackServiceURL(%q, %d) = %q, want %q", test.announced, test.port, got, test.want)
			}
		})
	}
}

func TestRestoreStep(t *testing.T) {
	t.Parallel()
	for name, test := range map[string]struct {
		connected bool
		restores  int
		attempt   bool
		next      int
	}{
		"a lost forward waits for its Host":           {false, 0, false, 0},
		"the Host returned":                           {true, 0, true, 0},
		"the last attempt":                            {true, forwardRestores - 1, true, forwardRestores - 1},
		"the attempts are used up":                    {true, forwardRestores, false, forwardRestores},
		"losing the Host again starts the count over": {false, forwardRestores, false, 0},
	} {
		t.Run(name, func(t *testing.T) {
			t.Parallel()
			if attempt, next := restoreStep(test.connected, test.restores); attempt != test.attempt || next != test.next {
				t.Fatalf("restoreStep(%v, %d) = %v, %d", test.connected, test.restores, attempt, next)
			}
		})
	}
}
