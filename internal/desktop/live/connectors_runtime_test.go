package live

import (
	"context"
	"testing"
	"testing/synctest"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type connectorRuntimeTransport struct {
	conversationTestTransport
	stream   *conversationTestSubscription
	info     protocol.PluginInfo
	cursor   uint64
	reads    int
	hold     bool
	pending  chan link.Response
	response link.Response
}

func (source *connectorRuntimeTransport) Subscribe(ctx context.Context, topic protocol.Topic) (link.Subscription, error) {
	stream, err := source.conversationTestTransport.Subscribe(ctx, topic)
	if topic.Kind == "node" && err == nil {
		source.stream = stream.(*conversationTestSubscription)
	}
	return stream, err
}

func (source *connectorRuntimeTransport) Dispatch(ctx context.Context, request protocol.Request) (link.Admission, error) {
	if request.Command.Kind != "read_plugin" {
		return source.conversationTestTransport.Dispatch(ctx, request)
	}
	source.mu.Lock()
	defer source.mu.Unlock()
	source.reads++
	completion := make(chan link.Response, 1)
	response := link.Response{Output: protocol.Output{Kind: "plugin", Data: source.info}}
	if source.hold {
		source.hold = false
		source.pending, source.response = completion, response
	} else {
		completion <- response
	}
	return link.Admission{Receipt: protocol.Receipt{ID: request.ID}, Completion: completion}, nil
}

func (source *connectorRuntimeTransport) change(state protocol.PluginMcpRuntimeState) {
	source.mu.Lock()
	defer source.mu.Unlock()
	source.info.Mcp = []protocol.PluginMcp{{Name: "runtime", Allowed: true,
		Availability: protocol.PluginMcpAvailabilityAvailable, Transport: protocol.PluginMcpTransportStreamableHttp,
		Runtime: &protocol.PluginMcpRuntime{State: state, Tools: 2}}}
	source.cursor++
	// Runtime changes publish the same package summary and stored revision.
	source.stream.updates <- protocol.Update{Kind: "event", Data: protocol.EventEnvelope{
		Node: source.Target(), Cursor: source.cursor, Event: protocol.Event{Kind: "plugin_changed", Data: source.info.Summary},
	}}
}

func connectorRuntimeFixture(t *testing.T) (*Runtime, *connectorRuntimeTransport, func()) {
	t.Helper()
	info := protocol.DefaultPluginInfo()
	info.Summary.Name, info.Summary.Revision, info.Summary.Enabled = "mcp-runtime", 1, true
	info.Mcp = []protocol.PluginMcp{{Name: "runtime", Allowed: true,
		Availability: protocol.PluginMcpAvailabilityAvailable, Transport: protocol.PluginMcpTransportStreamableHttp,
		Runtime: &protocol.PluginMcpRuntime{State: protocol.PluginMcpRuntimeStateConnecting}}}
	snapshot := protocol.DefaultSnapshot()
	snapshot.Node, snapshot.Plugins = protocol.NodeID{1}, []protocol.PluginSummary{info.Summary}
	source := &connectorRuntimeTransport{conversationTestTransport: conversationTestTransport{snapshot: snapshot}, info: info}
	ctx, cancel := context.WithCancel(t.Context())
	controller := client.New(source)
	if err := controller.Start(ctx); err != nil {
		cancel()
		t.Fatal(err)
	}
	r := &Runtime{ctx: ctx, cancel: cancel, controller: controller, authorizations: &authorizationState{},
		App: shell.NewLive(&viewmodel.Data{Workspace: &viewmodel.Workspace{}})}
	r.App.Host = source.Target().String()
	// Only the queued projection and settings frame are exercised; avoid the
	// shared raster workers, which cannot join a synctest bubble.
	tester := ui.NewTesterWith(func(c *ui.Context) {
		theme.Apply(c, "", theme.Light)
		r.mu.Lock()
		queue := r.queue
		r.queue = nil
		r.mu.Unlock()
		for _, apply := range queue {
			apply(c)
		}
		r.nodeSettingsFrame()
	}, 1, 1, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true}})
	settle := func() {
		for range 3 {
			synctest.Wait()
			tester.Frame()
		}
		synctest.Wait()
	}
	t.Cleanup(func() {
		cancel()
		r.workers.Wait()
		_ = controller.Close()
	})
	return r, source, settle
}

func TestConnectorRuntimeChanges(t *testing.T) {
	for _, test := range []struct {
		state protocol.PluginMcpRuntimeState
		want  viewmodel.ServerState
	}{
		{protocol.PluginMcpRuntimeStateConnected, viewmodel.ServerUp},
		{protocol.PluginMcpRuntimeStateFailed, viewmodel.ServerDown},
	} {
		t.Run(string(test.state), func(t *testing.T) {
			synctest.Test(t, func(t *testing.T) {
				r, source, settle := connectorRuntimeFixture(t)
				settle()
				assertConnectorRuntime(t, r, viewmodel.ServerStarting)
				source.change(test.state)
				settle()
				assertConnectorRuntime(t, r, test.want)
				for range 5 {
					settle()
				}
				if source.reads != 2 {
					t.Fatalf("package reads = %d, want one initial read and one runtime update", source.reads)
				}
			})
		})
	}
	t.Run("a runtime change during a read is retained", func(t *testing.T) {
		synctest.Test(t, func(t *testing.T) {
			r, source, settle := connectorRuntimeFixture(t)
			settle()
			source.hold = true
			source.change(protocol.PluginMcpRuntimeStateConnecting)
			settle()
			if source.pending == nil || source.reads != 2 {
				t.Fatal("the second package read was not held")
			}
			source.change(protocol.PluginMcpRuntimeStateConnected)
			settle()
			if source.reads != 2 {
				t.Fatal("a concurrent package read bypassed the pending read")
			}
			source.pending <- source.response
			settle()
			assertConnectorRuntime(t, r, viewmodel.ServerUp)
			settle()
			if source.reads != 3 {
				t.Fatalf("package reads = %d, want one coalesced follow-up read", source.reads)
			}
		})
	})
}

func assertConnectorRuntime(t *testing.T, r *Runtime, want viewmodel.ServerState) {
	t.Helper()
	services := r.App.Settings.Services
	if len(services) != 1 || len(services[0].Servers) != 1 {
		t.Fatalf("connector projection = %+v, want one server", services)
	}
	if got := services[0].Servers[0].State; got != want {
		t.Fatalf("connector state = %v, want %v", got, want)
	}
}
