package live

import (
	"context"
	"slices"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/preferences"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// TestSettingsTarget drives the Node-owned settings pages over two Nodes
// that answer with different values: device monitoring never changes the
// controller's configuration Node, pending drafts or captured saves.
func TestSettingsTarget(t *testing.T) {
	nodes := []protocol.NodeID{{1}, {2}}
	transports := make([]*conversationTestTransport, len(nodes))
	reporting := make([]*settingsTestTransport, len(nodes))
	// external is the compaction revision another controller saved on a
	// Node, 0 for none; the Node's transport guards it.
	external := make([]uint64, len(nodes))
	// doubt, when set, holds the local Node's next prevent sleep save until
	// it is closed; the Node then answers that it can never tell what the
	// save did.
	var doubt chan struct{}
	var saving chan struct{}
	controllers := make([]*client.Client, len(nodes))
	for index, node := range nodes {
		snapshot := protocol.DefaultSnapshot()
		snapshot.Node = node
		// Each Node answers its own values: the local one revision 1, 60 %, 4
		// children, sleep allowed, sandbox and code index on; the remote
		// one revision 2, 70 %, 5 children, sleep prevented, both off.
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
				if index == 0 && doubt != nil {
					close(saving)
					<-doubt
					return link.Response{Fault: &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "the save was not observed"}}, true
				}
				input, _ := detail[protocol.PowerSettings](request.Command.Data)
				input.Revision++
				return output("power_settings", input)
			case "read_compaction_settings":
				if external[index] != 0 {
					return output("compaction_settings", protocol.ConversationCompactionSettings{Revision: external[index], Percent: 85})
				}
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
			case "read_code_index_settings":
				return output("code_index_settings", protocol.CodeIndexSettings{Revision: revision, Enabled: index == 0})
			case "save_code_index_settings":
				input, _ := detail[protocol.CommandSaveCodeIndexSettingsPayload](request.Command.Data)
				// The fixture keeps no state: a second save finds the
				// revision it read first, as after another controller's.
				if input.ExpectedRevision != revision {
					return link.Response{Fault: &protocol.Fault{Code: protocol.ErrorCodeRevisionConflict, Message: "code index settings changed before saving"}}, true
				}
				input.Settings.Revision++
				return output("code_index_settings", input.Settings)
			case "inspect_request":
				if query, ok := detail[protocol.CommandInspectRequestPayload](request.Command.Data); ok && index == 0 && doubt != nil {
					return output("request_outcome", protocol.OutputRequestOutcomePayload{ID: query.ID, Outcome: protocol.RequestOutcome{Kind: "unknown"}})
				}
			case "save_telemetry_settings":
				input, _ := detail[protocol.TelemetryUpdate](request.Command.Data)
				return output("telemetry_settings", protocol.TelemetrySettings{Revision: input.Revision + 1, Errors: input.Errors, Metrics: input.Metrics})
			}
			return link.Response{}, false
		}
		transports[index] = transport
		reporting[index] = &settingsTestTransport{conversationTestTransport: transport}
		controllers[index] = client.New(reporting[index])
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
		kit.ToastHost(c)
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
	// show changes only the monitored device and runs the settings frame steps.
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
		if m.CodeIndex.Revision != revision || m.CodeIndex.Saved != (index == 0) || m.CodeIndex.Draft != (index == 0) || m.CodeIndex.Pending {
			t.Errorf("code index shows %+v, want Node %d's", m.CodeIndex, index)
		}
	}
	reads := []string{"read_power_settings", "list_channel_presets", "read_compaction_settings", "read_team_settings", "read_media_settings", "read_sandbox_settings", "read_code_index_settings"}

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

	t.Run("monitoring another host retains the local drafts", func(t *testing.T) {
		m.Compaction.Draft = 95
		before := sent(1)
		show(nodes[1].String())
		settle()
		if m.Compaction.Draft != 95 || m.Compaction.Saved != 60 || m.Power.Revision != 1 {
			t.Fatal("device monitor reset local settings draft")
		}
		if root.settingsOwner() != root || remote.settingsOwner() != root || root.packageOwner() != root || remote.packageOwner() != root {
			t.Fatal("configuration ownership followed monitored host")
		}
		if len(sent(1)) != len(before) {
			t.Fatalf("monitor read remote settings: %v", sent(1)[len(before):])
		}
		m.Compaction.Draft = m.Compaction.Saved
		held(t, 0)
	})

	t.Run("every global save goes to the local Node", func(t *testing.T) {
		before := sent(1)
		m.Power.Draft = true
		m.SavePower()
		m.Compaction.Draft = 75
		m.ProviderActions.SaveCompaction()
		m.Concurrency.Draft = 2
		m.TeamActions.SaveConcurrency()
		m.SaveMedia()
		m.Sandbox.Draft = false
		m.SaveSandbox()
		m.CodeIndex.Draft = false
		m.SaveCodeIndex()
		m.TeamActions.SaveRole(viewmodel.Role{Key: "review", Name: "Review"}, func(kit.Outcome) {})
		m.ProviderActions.RefreshCatalog()
		m.ConnectorActions.SaveMCP(settings.MCPDraft{Name: "fixture", Server: viewmodel.ConnectorServer{Command: "run"}}, func(bool) {})
		settle()
		for _, kind := range []string{"save_power_settings", "save_compaction_settings", "save_team_settings", "save_media_settings", "save_sandbox_settings", "save_code_index_settings", "put_role", "refresh_model_catalog", "install_mcp"} {
			if count(0, kind) != 1 {
				t.Errorf("%s reached local Node %d times", kind, count(0, kind))
			}
			if slices.Contains(sent(1)[len(before):], kind) {
				t.Errorf("%s was sent to monitored remote", kind)
			}
		}
		if m.Power.Revision != 2 || !m.Power.Saved || m.Compaction.Revision != 2 || m.Compaction.Saved != 75 || m.Concurrency.Revision != 2 || m.Concurrency.Saved != 2 || m.Sandbox.Revision != 2 || m.Sandbox.Saved || m.CodeIndex.Revision != 2 || m.CodeIndex.Saved || m.CodeIndex.Pending {
			t.Errorf("the saved values were not taken: power %+v, compaction %+v, concurrency %+v, sandbox %+v, code index %+v", m.Power, m.Compaction, m.Concurrency, m.Sandbox, m.CodeIndex)
		}
	})

	t.Run("a code index save that conflicts puts the switch back", func(t *testing.T) {
		m.CodeIndex.Draft = true
		m.SaveCodeIndex()
		settle()
		// The switch shows what the local Node answers the read that follows.
		if !m.CodeIndex.Draft || !m.CodeIndex.Saved || m.CodeIndex.Revision != 1 || m.CodeIndex.Pending || count(0, "read_code_index_settings") != 2 {
			t.Errorf("code index shows %+v after a conflict, read %d times", m.CodeIndex, count(0, "read_code_index_settings"))
		}
		if !tester.HasText(i18n.In("en", "code_index_settings_conflict")) {
			t.Errorf("the conflict was not reported: %q", tester.Texts())
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

	t.Run("monitor changes preserve a pending local save", func(t *testing.T) {
		doubt, saving = make(chan struct{}), make(chan struct{})
		m.Power.Draft = false
		m.SavePower()
		select {
		case <-saving:
		case <-time.After(5 * time.Second):
			t.Fatal("local save did not start")
		}
		show(nodes[1].String())
		show("offline-host")
		if !m.Power.Pending || m.Power.Draft {
			t.Fatal("monitor change discarded pending local save")
		}
		close(doubt)
		settle()
		doubt = nil
		if m.Power.Pending {
			t.Fatal("local completion did not settle after monitor change")
		}
		if !tester.HasText(i18n.In("en", settings.PowerMessages.Uncertain)) {
			t.Errorf("unknown local save was not reported: %q", tester.Texts())
		}
		if count(1, "save_power_settings") != 0 {
			t.Fatal("save escaped to monitored host")
		}
	})
	t.Run("offline monitor keeps local configuration available", func(t *testing.T) {
		show("offline-host")
		settle()
		if root.shownSettings() != root || !m.ProviderActions.Available() || m.SaveSandbox == nil {
			t.Fatal("offline monitor disabled local settings")
		}
		before := count(0, "save_power_settings")
		m.Power.Draft = !m.Power.Draft
		m.SavePower()
		settle()
		if count(0, "save_power_settings") != before+1 || count(1, "save_power_settings") != 0 {
			t.Fatal("offline monitor redirected local save")
		}
	})

	t.Run("another controller's compaction save is followed", func(t *testing.T) {
		transports[0].mu.Lock()
		external[0] = 4
		later := transports[0].snapshot
		transports[0].mu.Unlock()
		later.Cursor++
		later.CompactionSettingsRevision = 4
		reporting[0].report(later)
		for end := time.Now().Add(10 * time.Second); controllers[0].Snapshot().CompactionSettingsRevision != 4; time.Sleep(time.Millisecond) {
			if time.Now().After(end) {
				t.Fatal("the Node's newer compaction revision did not reach the Client")
			}
		}
		reads := count(0, "read_compaction_settings")
		// A draft waits: the read would move the value under the slider.
		m.Compaction.Draft = 90
		root.nodeSettingsFrame()
		settle()
		if count(0, "read_compaction_settings") != reads || m.Compaction.Revision != 2 {
			t.Errorf("the threshold was read over a draft: %+v", m.Compaction)
		}
		m.Compaction.Draft = m.Compaction.Saved
		root.nodeSettingsFrame()
		settle()
		if count(0, "read_compaction_settings") != reads+1 || m.Compaction.Revision != 4 || m.Compaction.Saved != 85 || m.Compaction.Draft != 85 {
			t.Errorf("the pages show %+v after %d reads, want the other controller's 85 at revision 4", m.Compaction, count(0, "read_compaction_settings")-reads)
		}
		root.nodeSettingsFrame()
		settle()
		if count(0, "read_compaction_settings") != reads+1 {
			t.Error("a revision already read was read again")
		}
	})
}

// settingsTestTransport keeps a fixture Node's subscriptions, so a test can
// send what the Node reports after its first snapshot.
type settingsTestTransport struct {
	*conversationTestTransport
	streams []*conversationTestSubscription
}

func (t *settingsTestTransport) Subscribe(ctx context.Context, topic protocol.Topic) (link.Subscription, error) {
	subscription, err := t.conversationTestTransport.Subscribe(ctx, topic)
	if stream, ok := subscription.(*conversationTestSubscription); ok && topic.Kind == "node" {
		t.mu.Lock()
		t.streams = append(t.streams, stream)
		t.mu.Unlock()
	}
	return subscription, err
}

// report sends a later snapshot to every subscription of the Node.
func (t *settingsTestTransport) report(snapshot protocol.Snapshot) {
	t.mu.Lock()
	defer t.mu.Unlock()
	for _, stream := range t.streams {
		select {
		case stream.updates <- protocol.Update{Kind: "snapshot", Data: snapshot}:
		default:
		}
	}
}

func TestGlobalSettingsFramesIgnoreMonitoredHost(t *testing.T) {
	transport := &conversationTestTransport{snapshot: protocol.DefaultSnapshot()}
	transport.snapshot.Node = protocol.NodeID{1}
	controller := client.New(transport)
	defer controller.Close()
	root := &Runtime{ctx: t.Context(), controller: controller, App: shell.NewLive(&viewmodel.Data{Workspace: &viewmodel.Workspace{}})}
	remote := &Runtime{parent: root, App: root.App}
	root.terminalSettings.known = true
	root.terminalSettings.model = settings.Stored[settings.TerminalHost]{Revision: 3}
	root.backupSettings.known, root.backupSettings.saved.Revision = true, 4
	root.localModels.known, root.localModels.watching = true, true
	root.localModels.settings = protocol.DefaultModelsLocalSettings()
	root.localModels.settings.Revision = 5
	root.localModels.availability = map[protocol.ModelsKind]protocol.ModelsCatalogAvailability{
		protocol.ModelsKindOcr: {Kind: "available"},
	}
	root.App.Page = shell.SettingsPage
	root.App.Settings.Section = settings.General
	root.terminalSettingsFrame()
	root.backupSettingsFrame(nil)
	root.App.Settings.Host.Draft.Env = []viewmodel.EnvVar{{Key: "DRAFT", Value: "retained"}}
	root.App.Settings.Backup.Draft.Directory = "/draft/backup"
	for _, monitored := range []string{"paired-host", "offline-host"} {
		root.App.Host = monitored
		root.terminalSettingsFrame()
		root.backupSettingsFrame(nil)
		root.App.Settings.Section = settings.OCR
		root.localModelsFrame()
		root.App.Settings.Section = settings.General
		if root.terminalSettings.shown != root || root.backupSettings.shown != root || root.localModels.shown != root || remote.packageOwner() != root {
			t.Fatal("a global configuration frame followed the device monitor")
		}
		if got := root.App.Settings.Host; len(got.Draft.Env) != 1 || got.Draft.Env[0].Key != "DRAFT" || got.Revision != 3 {
			t.Fatalf("terminal draft lost across monitor navigation: %+v", got)
		}
		if root.App.Settings.Backup.Draft.Directory != "/draft/backup" || root.App.Settings.OCR.Enabled.Revision != 5 {
			t.Fatal("backup or model configuration was replaced by monitor navigation")
		}
	}
	transport.mu.Lock()
	defer transport.mu.Unlock()
	if len(transport.requests) != 0 {
		t.Fatalf("changing only a monitor dispatched configuration requests: %+v", transport.requests)
	}
}
