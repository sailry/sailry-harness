package live

import (
	"slices"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/preferences"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// TestSettingsTarget drives the Node-owned settings pages over two Nodes
// that answer with different values: every page reads and saves on the Node
// of the selected host, turns to another host's Node when the selection
// changes, and keeps the desktop's telemetry consent on the local Node.
func TestSettingsTarget(t *testing.T) {
	nodes := []protocol.NodeID{{1}, {2}}
	transports := make([]*conversationTestTransport, len(nodes))
	controllers := make([]*client.Client, len(nodes))
	for index, node := range nodes {
		snapshot := protocol.DefaultSnapshot()
		snapshot.Node = node
		// Each Node answers its own values: the local one revision 1, 60 %, 4
		// children, sleep allowed, sandbox on; the remote one revision 2,
		// 70 %, 5 children, sleep prevented, sandbox off.
		revision := uint64(index + 1)
		snapshot.PowerSettingsRevision, snapshot.SandboxSettingsRevision, snapshot.TeamSettingsRevision = revision, revision, revision
		transport := &conversationTestTransport{snapshot: snapshot}
		transport.handle = func(request protocol.Request) (link.Response, bool) {
			output := func(kind string, data any) (link.Response, bool) {
				return link.Response{Output: protocol.Output{Kind: kind, Data: data}}, true
			}
			switch request.Command.Kind {
			case "read_power_settings":
				return output("power_settings", protocol.PowerSettings{Revision: revision, PreventSleep: index == 1})
			case "save_power_settings":
				input, _ := detail[protocol.PowerSettings](request.Command.Data)
				input.Revision++
				return output("power_settings", input)
			case "read_compaction_settings":
				return output("compaction_settings", protocol.ConversationCompactionSettings{Revision: revision, Percent: uint8(60 + 10*index)})
			case "save_compaction_settings":
				input, _ := detail[protocol.ConversationCompactionSettings](request.Command.Data)
				input.Revision++
				return output("compaction_settings", input)
			case "read_team_settings":
				return output("team_settings", protocol.TeamSettings{Revision: revision, ChildConcurrency: uint64(4 + index)})
			case "save_team_settings":
				input, _ := detail[protocol.TeamSettings](request.Command.Data)
				input.Revision++
				return output("team_settings", input)
			case "read_media_settings":
				return output("media_settings", protocol.MediaSettings{Revision: revision, Bindings: map[protocol.MediaKind]*protocol.MediaBinding{}})
			case "list_media_models":
				return output("media_models", []protocol.MediaCandidate{})
			case "save_media_settings":
				input, _ := detail[protocol.MediaSettings](request.Command.Data)
				input.Revision++
				return output("media_settings", input)
			case "read_sandbox_settings":
				return output("sandbox_settings", protocol.SandboxSettings{Revision: revision, Enabled: index == 0})
			case "save_sandbox_settings":
				input, _ := detail[protocol.SandboxSettings](request.Command.Data)
				input.Revision++
				return output("sandbox_settings", input)
			case "save_telemetry_settings":
				input, _ := detail[protocol.TelemetryUpdate](request.Command.Data)
				return output("telemetry_settings", protocol.TelemetrySettings{Revision: input.Revision + 1, Errors: input.Errors, Metrics: input.Metrics})
			}
			return link.Response{}, false
		}
		transports[index] = transport
		controllers[index] = client.New(transport)
		if err := controllers[index].Start(t.Context()); err != nil {
			t.Fatal(err)
		}
		defer controllers[index].Close()
	}
	root, err := NewWithPreferences(t.Context(), controllers[0], preferences.Open(t.TempDir()+"/preferences.json"))
	if err != nil {
		t.Fatal(err)
	}
	defer root.Close()
	remote := root.newBinding(controllers[1])
	root.bindings = map[string]*Runtime{nodes[1].String(): remote}
	m := root.App.Settings
	tester := ui.NewTester(func(c *ui.Context) {
		theme.Apply(c, "", theme.Light)
		root.mu.Lock()
		queue := root.queue
		root.queue = nil
		root.mu.Unlock()
		for _, post := range queue {
			post(c)
		}
	}, 640, 480)
	tester.SetLocale("en")
	// settle runs the commands under way to their answers, and the commands
	// those answers start in turn.
	settle := func() {
		for range 8 {
			root.workers.Wait()
			remote.workers.Wait()
			tester.Frame()
		}
	}
	// show selects a host as the rail does and runs the frame's settings steps.
	show := func(host string) {
		root.App.Host = host
		root.nodeSettingsFrame()
		root.workspaceSettingsFrame()
	}
	// sent are the kinds of the commands a Node received.
	sent := func(index int) []string {
		transports[index].mu.Lock()
		defer transports[index].mu.Unlock()
		kinds := make([]string, 0, len(transports[index].requests))
		for _, request := range transports[index].requests {
			kinds = append(kinds, request.Command.Kind)
		}
		return kinds
	}
	count := func(index int, kind string) (n int) {
		for _, other := range sent(index) {
			if other == kind {
				n++
			}
		}
		return n
	}
	// held checks the values the pages show against a Node's.
	held := func(t *testing.T, index int) {
		t.Helper()
		revision := uint64(index + 1)
		if m.Power.Revision != revision || m.Power.Saved != (index == 1) || m.Power.Draft != (index == 1) || m.Power.Pending {
			t.Errorf("prevent sleep shows %+v, want Node %d's", m.Power, index)
		}
		if m.Compaction.Revision != revision || m.Compaction.Saved != float64(60+10*index) || m.Compaction.Draft != m.Compaction.Saved {
			t.Errorf("compaction shows %+v, want Node %d's", m.Compaction, index)
		}
		if m.Concurrency.Revision != revision || m.Concurrency.Saved != float64(4+index) || m.Concurrency.Draft != m.Concurrency.Saved {
			t.Errorf("child concurrency shows %+v, want Node %d's", m.Concurrency, index)
		}
		if m.Media.Revision != revision || m.Media.Pending {
			t.Errorf("media models show revision %d, want Node %d's", m.Media.Revision, index)
		}
		if m.Sandbox.Revision != revision || m.Sandbox.Saved != (index == 0) || m.Sandbox.Draft != (index == 0) {
			t.Errorf("sandbox shows %+v, want Node %d's", m.Sandbox, index)
		}
	}
	reads := []string{"read_power_settings", "list_channel_presets", "read_compaction_settings", "read_team_settings", "read_media_settings", "read_sandbox_settings"}

	root.workspaceSettingsFrame()
	settle()
	t.Run("the pages open on the local Node", func(t *testing.T) {
		held(t, 0)
		for _, kind := range reads {
			if count(0, kind) == 0 || count(1, kind) != 0 {
				t.Errorf("%s was read %d times locally and %d times on the other host", kind, count(0, kind), count(1, kind))
			}
		}
	})

	t.Run("selecting a host turns every page to its Node", func(t *testing.T) {
		m.Compaction.Draft = 95 // a draft of the Node the pages leave
		show(nodes[1].String())
		if m.Compaction.Draft == 95 || len(m.Channels) != 0 {
			t.Error("the previous Node's draft stayed on the pages")
		}
		settle()
		held(t, 1)
		for _, kind := range reads {
			if count(1, kind) == 0 {
				t.Errorf("%s was not read on the selected host", kind)
			}
		}
		if _, ok := m.HooksContext(); ok {
			t.Error("the hooks of a conversation on another host were offered for the selected host")
		}
	})

	t.Run("every save goes to the selected Node", func(t *testing.T) {
		before := sent(0)
		m.Power.Draft = false
		m.SavePower()
		m.Compaction.Draft = 75
		m.ProviderActions.SaveCompaction()
		m.Concurrency.Draft = 2
		m.TeamActions.SaveConcurrency()
		m.SaveMedia()
		m.Sandbox.Draft = true
		m.SaveSandbox()
		m.TeamActions.SaveRole(viewmodel.Role{Key: "review", Name: "Review"}, func(kit.Outcome) {})
		m.ProviderActions.RefreshCatalog()
		m.ConnectorActions.SaveMCP(settings.MCPDraft{Name: "fixture", Server: viewmodel.ConnectorServer{Command: "run"}}, func(bool) {})
		settle()
		for _, kind := range []string{"save_power_settings", "save_compaction_settings", "save_team_settings", "save_media_settings", "save_sandbox_settings", "put_role", "refresh_model_catalog", "install_mcp"} {
			if count(1, kind) != 1 {
				t.Errorf("%s reached the selected host %d times", kind, count(1, kind))
			}
			if slices.Contains(sent(0)[len(before):], kind) {
				t.Errorf("%s was sent to the local Node", kind)
			}
		}
		if m.Power.Revision != 3 || m.Power.Saved || m.Compaction.Revision != 3 || m.Compaction.Saved != 75 || m.Concurrency.Revision != 3 || m.Concurrency.Saved != 2 || m.Sandbox.Revision != 3 || !m.Sandbox.Saved {
			t.Errorf("the saved values were not taken: power %+v, compaction %+v, concurrency %+v, sandbox %+v", m.Power, m.Compaction, m.Concurrency, m.Sandbox)
		}
	})

	t.Run("telemetry consent stays on the local Node", func(t *testing.T) {
		m.Telemetry.Draft.Errors = !m.Telemetry.Draft.Errors
		m.SaveTelemetry()
		settle()
		if count(0, "save_telemetry_settings") != 1 || count(1, "save_telemetry_settings") != 0 || count(1, "read_telemetry_settings") != 0 {
			t.Error("telemetry consent followed the selected host")
		}
	})

	t.Run("an answer for a Node the pages left is dropped", func(t *testing.T) {
		show(nodes[0].String())
		settle()
		held(t, 0)
		// The other host's reads are still on their way when the selection
		// returns: the pages keep the local Node's values.
		show(nodes[1].String())
		show(nodes[0].String())
		settle()
		held(t, 0)
	})

	t.Run("a host without a connection has no Node to edit", func(t *testing.T) {
		before := [2]int{len(sent(0)), len(sent(1))}
		show("0000000000000000000000000000000000000000000000000000000000000009")
		if !m.Power.Pending || !m.Compaction.Pending || !m.Concurrency.Pending || !m.Media.Pending || m.SaveSandbox != nil || m.ProviderActions.Available() {
			t.Error("a host without a connection left a setting editable")
		}
		saved := true
		m.ProviderActions.Save(settings.ProviderDraft{}, func(ok bool) { saved = ok })
		m.SavePower()
		settle()
		if saved || len(sent(0)) != before[0] || len(sent(1)) != before[1] {
			t.Error("a save without a Node was sent to another one")
		}
		show(nodes[0].String())
		settle()
		held(t, 0)
	})
}
