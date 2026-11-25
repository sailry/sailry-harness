package live

import (
	"context"
	"slices"
	"testing"
	"testing/synctest"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/assembly/desktop"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/preferences"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/testassembly"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// TestSettingsTarget drives the Node-owned settings pages over several Nodes
// that answer with different values: device monitoring never changes the
// controller's configuration Node, pending drafts or captured saves.
func TestSettingsTarget(t *testing.T) {
	nodes := []protocol.NodeID{{1}, {2}, {3}}
	projects := []protocol.ProjectID{{1}, {2}, {3}}
	transports := make([]*conversationTestTransport, len(nodes))
	reporting := make([]*settingsTestTransport, len(nodes))
	// external is the compaction revision another controller saved on a
	// Node, 0 for none; the Node's transport guards it.
	external := make([]uint64, len(nodes))
	prompts := make([]protocol.PersonalInstructions, len(nodes))
	// doubt, when set, holds the local Node's next prevent sleep save until
	// it is closed; the Node then answers that it can never tell what the
	// save did.
	var doubt chan struct{}
	var saving chan struct{}
	controllers := make([]*client.Client, len(nodes))
	for index, node := range nodes {
		snapshot := protocol.DefaultSnapshot()
		snapshot.Node = node
		fresh := uint64(time.Now().UnixMilli())
		snapshot.ModelCatalog.UpdatedAtMs, snapshot.ModelCatalog.Scores.UpdatedAtMs = &fresh, &fresh
		snapshot.User = &protocol.User{ID: protocol.UserID{byte(index + 1)}, Name: "Fixture", Local: index == 0, Administrator: index != 2, State: "active"}
		project := protocol.DefaultProject()
		project.ID, project.Name, project.Path, project.Owner = projects[index], "Fixture project", t.TempDir(), &snapshot.User.ID
		snapshot.Projects = []protocol.Project{project}
		// Each Node answers its own values: the local one revision 1, 60 %, 4
		// children, sleep allowed, sandbox and code index on; the remote
		// one revision 2, 70 %, 5 children, sleep prevented, both off.
		revision := uint64(index + 1)
		snapshot.PowerSettingsRevision, snapshot.SandboxSettingsRevision, snapshot.TeamSettingsRevision = revision, revision, revision
		info := protocol.DefaultPluginInfo()
		info.Summary.Name, info.Summary.Digest, info.Summary.Revision = "fixture", "fixture-digest", revision
		snapshot.Plugins = []protocol.PluginSummary{info.Summary}
		transport := &conversationTestTransport{snapshot: snapshot}
		transport.handle = func(request protocol.Request) (link.Response, bool) {
			output := func(kind string, data any) (link.Response, bool) {
				return link.Response{Output: protocol.Output{Kind: kind, Data: data}}, true
			}
			switch request.Command.Kind {
			case "read_personal_instructions":
				return output("personal_instructions", prompts[index])
			case "save_personal_instructions":
				input, _ := detail[protocol.CommandSavePersonalInstructionsPayload](request.Command.Data)
				if current := prompts[index].Revision; (current == nil) != (input.ExpectedRevision == nil) || current != nil && *current != *input.ExpectedRevision {
					return link.Response{Fault: &protocol.Fault{Code: protocol.ErrorCodeRevisionConflict}}, true
				}
				prompts[index] = protocol.PersonalInstructions{Text: input.Text, Revision: &input.Text}
				return output("personal_instructions", prompts[index])
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
			case "read_plugin":
				return output("plugin", info)
			case "set_plugin_enabled":
				input, _ := detail[protocol.CommandSetPluginEnabledPayload](request.Command.Data)
				if input.ExpectedRevision != revision {
					return link.Response{Fault: &protocol.Fault{Code: protocol.ErrorCodeRevisionConflict, Message: "wrong package owner's revision"}}, true
				}
				info.Summary.Enabled = input.Enabled
				info.Summary.Revision++
				return output("plugin", info)
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
			case "read_local_model_settings":
				models := protocol.DefaultModelsLocalSettings()
				models.Revision = revision
				return output("local_model_settings", models)
			case "read_local_models":
				return output("local_models", protocol.ModelsCatalog{Availability: map[protocol.ModelsKind]protocol.ModelsCatalogAvailability{
					protocol.ModelsKindDecision: {Kind: "available"}, protocol.ModelsKindEmbedding: {Kind: "available"},
				}})
			case "save_local_model_settings":
				input, _ := detail[protocol.ModelsLocalSettings](request.Command.Data)
				input.Revision++
				return output("local_model_settings", input)
			case "pause_knowledge_collection":
				input, _ := detail[protocol.CommandPauseKnowledgeCollectionPayload](request.Command.Data)
				value := protocol.DefaultKnowledgeCollection()
				value.ID, value.Node, value.OwnerUser, value.Revision, value.Paused = input.ID, node, snapshot.User.ID, input.ExpectedRevision+1, input.Paused
				return output("knowledge_collection", value)
			case "read_code_index_settings":
				value := protocol.CodeIndexSettings{Revision: revision, Enabled: index == 0}
				if index == 1 {
					value.Persistent = new(false)
				}
				return output("code_index_settings", value)
			case "save_code_index_settings":
				input, _ := detail[protocol.CommandSaveCodeIndexSettingsPayload](request.Command.Data)
				// A second machine switch save finds the original revision,
				// as after another controller's edit. A persistence save uses
				// the revision returned by the first switch save.
				expected := revision
				if input.Settings.Persistent != nil {
					expected++
				}
				if input.ExpectedRevision != expected {
					return link.Response{Fault: &protocol.Fault{Code: protocol.ErrorCodeRevisionConflict, Message: "code index settings changed before saving"}}, true
				}
				if index == 1 && input.Settings.Persistent == nil {
					input.Settings.Persistent = new(false)
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
	assembly := desktop.Harness()
	testassembly.RegisterExecutionFixture(&assembly)
	root, err := NewWithAssembly(t.Context(), controllers[0], preferences.Open(t.TempDir()+"/preferences.json"), assembly)
	if err != nil {
		t.Fatal(err)
	}
	defer root.Close()
	remote := root.newBinding(controllers[1])
	readOnly := root.newBinding(controllers[2])
	t.Cleanup(func() { _ = readOnly.Close() })
	root.bindings = map[string]*Runtime{nodes[1].String(): remote, nodes[2].String(): readOnly}
	m := root.App.Settings
	codeSetting := func(t *testing.T) *testassembly.ExecutionFixture {
		t.Helper()
		if len(m.ExecutionSettings) != 1 {
			t.Fatalf("execution settings = %d, want the selected Node's code index", len(m.ExecutionSettings))
		}
		setting, ok := m.ExecutionSettings[0].(*testassembly.ExecutionFixture)
		if !ok {
			t.Fatalf("execution setting = %T, want code index", m.ExecutionSettings[0])
		}
		return setting
	}
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
			readOnly.workers.Wait()
			tester.Frame()
		}
	}
	openSection := func(section settings.Section) {
		root.App.Page = shell.SettingsPage
		m.Open(section)
		root.nodeSettingsFrame()
		root.workspaceSettingsFrame()
		settle()
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
		if m.Sandbox.Revision != revision || m.Sandbox.Saved != (index == 0) || m.Sandbox.Draft != (index == 0) {
			t.Errorf("sandbox shows %+v, want Node %d's", m.Sandbox, index)
		}
		if value := codeSetting(t).Stored; value.Revision != revision || value.Saved != (index == 0) || value.Draft != (index == 0) || value.Pending {
			t.Errorf("code index shows %+v, want Node %d's", value, index)
		}
	}
	reads := []string{"read_power_settings", "list_channel_presets", "read_compaction_settings", "read_team_settings", "read_media_settings", "read_sandbox_settings", "read_code_index_settings"}

	root.workspaceSettingsFrame()
	settle()
	t.Run("the pages open on the local Node", func(t *testing.T) {
		held(t, 0)
		openSection(settings.Providers)
		if m.Compaction.Revision != 1 || m.Compaction.Saved != 60 || m.Compaction.Draft != 60 {
			t.Errorf("local compaction = %+v", m.Compaction)
		}
		openSection(settings.Teams)
		if m.Concurrency.Revision != 1 || m.Concurrency.Saved != 4 || m.Concurrency.Draft != 4 {
			t.Errorf("local child concurrency = %+v", m.Concurrency)
		}
		openSection(settings.Media)
		if m.Media.Revision != 1 || m.Media.Pending {
			t.Errorf("local media models = %+v", m.Media)
		}
		for _, kind := range reads {
			if count(0, kind) == 0 || kind != "list_channel_presets" && count(1, kind) != 0 {
				t.Errorf("%s was read %d times locally and %d times on the other host", kind, count(0, kind), count(1, kind))
			}
		}
	})

	t.Run("monitoring another host retains the local drafts", func(t *testing.T) {
		openSection(settings.Providers)
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
		openSection(settings.General)
		m.Power.Draft = true
		m.SavePower()
		settle()
		openSection(settings.Providers)
		m.Compaction.Draft = 75
		m.ProviderActions.SaveCompaction()
		settle()
		if m.Compaction.Revision != 2 || m.Compaction.Saved != 75 {
			t.Errorf("compaction save = %+v", m.Compaction)
		}
		m.ProviderActions.RefreshCatalog()
		settle()
		openSection(settings.Teams)
		m.Concurrency.Draft = 2
		m.TeamActions.SaveConcurrency()
		m.TeamActions.SaveRole(viewmodel.Role{Key: "review", Name: "Review"}, func(kit.Outcome) {})
		settle()
		if m.Concurrency.Revision != 2 || m.Concurrency.Saved != 2 {
			t.Errorf("concurrency save = %+v", m.Concurrency)
		}
		openSection(settings.Media)
		m.SaveMedia()
		settle()
		openSection(settings.Conversation)
		m.Sandbox.Draft = false
		m.SaveSandbox()
		codeSetting(t).SaveEnabled(false)
		settle()
		openSection(settings.Connectors)
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
		value := codeSetting(t).Stored
		if m.Power.Revision != 2 || !m.Power.Saved || m.Sandbox.Revision != 2 || m.Sandbox.Saved || value.Revision != 2 || value.Saved || value.Pending {
			t.Errorf("the saved values were not taken: power %+v, sandbox %+v, code index %+v", m.Power, m.Sandbox, value)
		}
	})

	t.Run("a code index save that conflicts puts the switch back", func(t *testing.T) {
		codeSetting(t).SaveEnabled(true)
		settle()
		// The switch shows what the local Node answers the read that follows.
		if value := codeSetting(t).Stored; !value.Draft || !value.Saved || value.Revision != 1 || value.Pending || count(0, "read_code_index_settings") != 2 {
			t.Errorf("code index shows %+v after a conflict, read %d times", value, count(0, "read_code_index_settings"))
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
		openSection(settings.Providers)
		previousRevision := m.Compaction.Revision
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
		if count(0, "read_compaction_settings") != reads || m.Compaction.Revision != previousRevision {
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
	t.Run("an explicit teams target reads and saves only on that Node", func(t *testing.T) {
		root.App.Page = shell.SettingsPage
		m.Open(settings.Teams)
		m.ManagedNode = nodes[1].String()
		root.nodeSettingsFrame()
		settle()
		if remote.currentSettingsPage(settings.Teams) == nil || m.Concurrency.Revision != 2 || m.Concurrency.Saved != 5 {
			t.Fatalf("remote concurrency settings = %+v", m.Concurrency)
		}
		beforeLocal, beforeRemote := count(0, "save_team_settings"), count(1, "save_team_settings")
		m.Concurrency.Draft = 8
		m.TeamActions.SaveConcurrency()
		settle()
		if count(0, "save_team_settings") != beforeLocal || count(1, "save_team_settings") != beforeRemote+1 {
			t.Fatal("explicit host save went to another Node")
		}

		save := m.TeamActions.SaveConcurrency
		m.ManagedNode = "offline-host"
		root.nodeSettingsFrame()
		settle()
		save()
		settle()
		if root.settingsPage(settings.Teams) != nil || m.TeamActions != nil || count(0, "save_team_settings") != beforeLocal || count(1, "save_team_settings") != beforeRemote+1 {
			t.Fatal("unavailable host save used another Node")
		}
	})
	t.Run("package management shows the selected installation", func(t *testing.T) {
		m.Open(settings.Plugins)
		m.ManagedNode = nodes[1].String()
		root.packagesFrame(nil)
		settle()
		root.packagesFrame(nil)
		if root.packageOwner() != remote || len(m.Packages) != 1 || m.Packages[0].Enabled {
			t.Fatalf("remote package list = %+v", m.Packages)
		}
		m.PackageActions.Toggle("fixture", true)
		settle()
		root.packagesFrame(nil)
		if !m.Packages[0].Enabled || count(0, "set_plugin_enabled") != 0 || count(1, "set_plugin_enabled") != 1 {
			t.Fatal("package toggle used another Node's installation")
		}
		m.ManagedNode = ""
		root.packagesFrame(nil)
		settle()
		root.packagesFrame(nil)
		if root.packageOwner() != root || len(m.Packages) != 1 || m.Packages[0].Enabled {
			t.Fatalf("local package list picked up the remote change: %+v", m.Packages)
		}
	})
	t.Run("model and machine settings share the selected host", func(t *testing.T) {
		root.App.Page = shell.SettingsPage
		m.ManagedNode = nodes[1].String()
		m.Open(settings.Providers)
		root.nodeSettingsFrame()
		root.workspaceSettingsFrame()
		settle()
		if m.Compaction.Revision != 2 || m.Compaction.Saved != 70 {
			t.Fatalf("remote compaction = %+v", m.Compaction)
		}
		before := count(0, "save_compaction_settings")
		m.Compaction.Draft = 78
		m.ProviderActions.SaveCompaction()
		settle()
		if count(1, "save_compaction_settings") != 1 || count(0, "save_compaction_settings") != before || m.Compaction.Saved != 78 {
			t.Fatal("compaction save did not remain on the host selected in Models")
		}
		m.Open(settings.Conversation)
		root.nodeSettingsFrame()
		root.workspaceSettingsFrame()
		settle()
		code := codeSetting(t)
		heldMachine := m.ExecutionSettingsStatus == "" && m.Sandbox.Revision == 2 && code.Revision == 2
		if !heldMachine {
			t.Fatalf("remote execution settings = %+v / %+v", m.Sandbox, code.Stored)
		}
		m.Sandbox.Draft = true
		m.SaveSandbox()
		code.SaveEnabled(true)
		settle()
		if count(1, "save_sandbox_settings") != 1 || count(1, "save_code_index_settings") != 1 {
			t.Fatal("conversation settings save did not reach the selected host")
		}
		if code.Persistent == nil || *code.Persistent {
			t.Fatalf("persistence was unavailable or enabled by default: %+v", code.Persistent)
		}
		beforeCodeIndex := count(0, "save_code_index_settings")
		code.SavePersistent(true)
		settle()
		if count(1, "save_code_index_settings") != 2 || count(0, "save_code_index_settings") != beforeCodeIndex ||
			code.Persistent == nil || !*code.Persistent || code.Revision != 4 {
			t.Fatalf("remote persistence did not save: %+v, %v", code.Stored, code.Persistent)
		}
		m.Open(settings.Decision)
		root.localModelsFrame()
		settle()
		root.localModelsFrame()
		if m.Decision.Enabled.Revision != 2 || m.Decision.Enabled.Pending {
			t.Fatalf("remote decision settings = %+v", m.Decision.Enabled)
		}
		m.Decision.Enabled.Draft = !m.Decision.Enabled.Saved
		m.ModelActions.Save(settings.ModelsDecision)
		settle()
		if count(1, "save_local_model_settings") != 1 || count(0, "save_local_model_settings") != 0 {
			t.Fatal("decision settings were written outside the selected host")
		}
	})
	t.Run("knowledge rows retain their owners across settings targets", func(t *testing.T) {
		owned := protocol.DefaultKnowledgeCollection()
		owned.ID, owned.Node, owned.OwnerUser, owned.Revision = protocol.KnowledgeID{1}, nodes[1], protocol.UserID{2}, 5
		owned.Definition.Name = "Writable"
		shared := owned
		shared.Node, shared.OwnerUser = nodes[2], protocol.UserID{99}
		shared.Definition.Name = "Shared"
		root.knowledge.known, root.localModels.knowledge = true, true
		root.knowledge.collections = []protocol.KnowledgeCollection{owned, shared}
		m.ManagedNode = nodes[2].String()
		m.Open(settings.Knowledge)
		root.localModelsFrame()
		if m.LocalModelsStatus != "permission_denied" || len(m.Collections) != 2 || !m.Collections[0].ReadOnly || m.Collections[1].ReadOnly {
			t.Fatalf("remote embedding or collection scope = %s, %+v", m.LocalModelsStatus, m.Collections)
		}
		m.ModelActions.CollectionPause(m.Collections[0].ID, true)
		m.ModelActions.CollectionPause(m.Collections[1].ID, true)
		settle()
		if count(1, "pause_knowledge_collection") != 1 || count(2, "pause_knowledge_collection") != 0 || count(0, "pause_knowledge_collection") != 0 {
			t.Fatal("collection writes did not preserve owner and access scope")
		}
	})
	t.Run("conversation settings links use the execution Node", func(t *testing.T) {
		cv := &viewmodel.Conversation{Node: nodes[1].String(), ID: "draft/settings-target"}
		comp := root.App.Composer(cv.ID)
		comp.Host = cv.Node
		var ctx *composer.Context
		var actions shell.ConversationActions
		conversationView := ui.NewTester(func(c *ui.Context) { ctx, actions = root.conversation(c, cv, comp) }, 320, 240)
		conversationView.Frame()
		liveActions, ok := actions.(*conversationActions)
		if !ok || liveActions.r != remote {
			t.Fatal("the fixture draft did not execute on the remote host")
		}
		if ctx.SelectModel == nil || !liveActions.CanManageTeams() {
			t.Fatal("the assembled settings links are unavailable")
		}
		for _, link := range []struct {
			section settings.Section
			open    func()
		}{{settings.Providers, ctx.SelectModel}, {settings.Teams, actions.ManageTeams}} {
			root.App.Navigate(shell.ConversationPage)
			root.App.Host, m.ManagedNode = nodes[2].String(), nodes[2].String()
			link.open()
			root.nodeSettingsFrame()
			settle()
			if root.App.Page != shell.SettingsPage || m.Section != link.section || m.ManagedNode != nodes[1].String() || remote.currentSettingsPage(link.section) == nil {
				t.Fatalf("%s opened with settings host %q", link.section, m.ManagedNode)
			}
			if root.App.Host != nodes[2].String() {
				t.Fatal("a settings link changed the monitored host")
			}
		}
		m.SetNativePages(nil)
		defer m.SetNativePages(assembly.SettingsPages)
		root.App.Navigate(shell.ConversationPage)
		m.ManagedNode = nodes[2].String()
		section := m.Section
		ctx.SelectModel()
		actions.ManageTeams()
		if root.App.Page != shell.ConversationPage || m.Section != section || m.ManagedNode != nodes[2].String() {
			t.Fatalf("an unavailable link changed navigation: page %s, section %s, host %q", root.App.Page, m.Section, m.ManagedNode)
		}
		conversationView.Frame()
		if ctx.SelectModel != nil || actions.(*conversationActions).CanManageTeams() {
			t.Fatal("the conversation offers links to omitted settings pages")
		}
	})
	t.Run("custom prompt is loaded on demand and scoped to the selected host", func(t *testing.T) {
		root.App.Page = shell.SettingsPage
		m.Open(settings.Conversation)
		showHost := func(host string) { m.ManagedNode = host; root.nodeSettingsFrame(); settle() }
		showHost("")
		if e := m.PersonalInstructions; e == nil || !e.Known || e.Draft != "" {
			t.Fatalf("first prompt = %+v", e)
		}
		m.PersonalInstructions.Draft = "Local draft"
		showHost(nodes[1].String())
		if e := m.PersonalInstructions; e == nil || !e.Known || e.Draft != "" {
			t.Fatalf("remote inherited another host's draft: %+v", e)
		}
		m.PersonalInstructions.Draft = "Remote\nprompt"
		m.RetryPersonalInstructions()
		settle()
		if prompts[1].Text != "Remote\nprompt" || prompts[0].Text != "" {
			t.Fatalf("saved prompts = %+v", prompts)
		}
		other := "remote change"
		prompts[1] = protocol.PersonalInstructions{Revision: &other, Text: other}
		m.PersonalInstructions.Draft = "Edited draft"
		m.RetryPersonalInstructions()
		settle()
		if e := m.PersonalInstructions; !e.Failed || e.Saved != other || e.Draft != "Edited draft" || prompts[1].Text != other {
			t.Fatalf("conflict overwrote a draft or file: %+v", e)
		}
		showHost("")
		if m.PersonalInstructions.Draft != "Local draft" {
			t.Fatal("host switch discarded the local prompt")
		}
		showHost(nodes[2].String())
		if m.PersonalInstructions != nil || count(2, "read_personal_instructions") != 0 {
			t.Fatal("machine instructions exposed without permission")
		}
	})
	t.Run("temporary provider outage keeps the editor binding", func(t *testing.T) {
		root.App.Navigate(shell.SettingsPage)
		m.Open(settings.Providers)
		m.ManagedNode = nodes[1].String()
		root.nodeSettingsFrame()
		settle()
		binding := root.settingsPage(settings.Providers)
		m.Compaction.Draft = 82
		m.ProviderActions.SaveCompaction()
		remote.workers.Wait()
		if !m.Compaction.Pending {
			t.Fatal("the fixture did not leave the save completion queued")
		}
		refresh := func(code protocol.ErrorCode) error {
			reporting[1].mu.Lock()
			reporting[1].nodeFault = code
			reporting[1].mu.Unlock()
			return controllers[1].Refresh(t.Context())
		}
		if err := refresh(protocol.ErrorCodeUnavailable); faultCode(err) != protocol.ErrorCodeUnavailable {
			t.Fatalf("could not simulate a provider host outage: %v", err)
		}
		root.nodeSettingsFrame()
		if root.settingsPage(settings.Providers) != binding || m.ProviderActions.Available() {
			t.Fatal("the outage reset the editor binding or left its actions available")
		}
		settle()
		if m.Compaction.Pending || m.Compaction.Saved != 82 {
			t.Fatalf("an acknowledged save did not settle in the retained editor: %+v", m.Compaction)
		}
		if err := refresh(""); err != nil {
			t.Fatal(err)
		}
		root.nodeSettingsFrame()
		if root.settingsPage(settings.Providers) != binding || !m.ProviderActions.Available() {
			t.Fatal("the same returning host did not retain its editor binding")
		}
		settle()
		if err := refresh(protocol.ErrorCodePermissionDenied); faultCode(err) != protocol.ErrorCodePermissionDenied {
			t.Fatalf("could not simulate permission loss: %v", err)
		}
		root.nodeSettingsFrame()
		if root.settingsPage(settings.Providers) != nil || binding.target.Context.Err() == nil || m.ProviderActions.Available() {
			t.Fatal("permission loss retained the editor binding")
		}
	})
}

type heldSettingsCommand struct {
	request protocol.Request
	done    chan link.Response
}

type heldSettingsTransport struct {
	*conversationTestTransport
	writes chan heldSettingsCommand
}

func (t *heldSettingsTransport) Dispatch(ctx context.Context, request protocol.Request) (link.Admission, error) {
	switch request.Command.Kind {
	case "save_power_settings", "save_team_settings":
		done := make(chan link.Response, 1)
		t.writes <- heldSettingsCommand{request: request, done: done}
		return link.Admission{Receipt: protocol.Receipt{ID: request.ID, Durable: request.Command.Durable()}, Completion: done}, nil
	default:
		return t.conversationTestTransport.Dispatch(ctx, request)
	}
}

func TestPowerAndTeamSettingsFollowSaveBinding(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		snapshot := protocol.DefaultSnapshot()
		snapshot.Node = protocol.NodeID{1}
		transport := &heldSettingsTransport{conversationTestTransport: &conversationTestTransport{snapshot: snapshot}, writes: make(chan heldSettingsCommand, 2)}
		controller := client.New(transport)
		defer controller.Close()
		ctx, cancel := context.WithCancel(t.Context())
		defer cancel()
		if err := controller.Start(ctx); err != nil {
			t.Fatal(err)
		}
		assembly := desktop.Harness()
		app := shell.NewLiveWithAssembly(&viewmodel.Data{Workspace: &viewmodel.Workspace{}}, assembly)
		r := &Runtime{ctx: ctx, controller: controller, App: app, teams: newTeamState(), busy: map[string]bool{}, uncertain: map[string]protocol.Request{}}
		r.bindSettingsPages(assembly.SettingsPages)
		defer r.closeSettingsPages()
		tester := ui.NewTester(func(c *ui.Context) {
			r.mu.Lock()
			queue := r.queue
			r.queue = nil
			r.mu.Unlock()
			for _, complete := range queue {
				complete(c)
			}
		}, 1, 1)
		returnToHost := func() {
			r.settingsShown = &Runtime{}
			r.settingsGeneration++
			r.settingsShown = r
			r.settingsGeneration++
		}
		write := func(kind string, submit func()) heldSettingsCommand {
			t.Helper()
			submit()
			synctest.Wait()
			select {
			case pending := <-transport.writes:
				if pending.request.Command.Kind != kind {
					t.Fatalf("sent %q, want %q", pending.request.Command.Kind, kind)
				}
				return pending
			default:
				t.Fatalf("%s was not dispatched", kind)
				return heldSettingsCommand{}
			}
		}
		complete := func(pending heldSettingsCommand, kind string, data any) {
			pending.done <- link.Response{Output: protocol.Output{Kind: kind, Data: data}}
			synctest.Wait()
			tester.Frame()
		}

		m := app.Settings
		powerDraft := settings.Stored[bool]{Revision: 1, Draft: true}
		m.Power = powerDraft
		power := write("save_power_settings", r.savePower)
		returnToHost()
		m.Power = powerDraft
		r.savePower()
		synctest.Wait()
		if !m.Power.Pending || len(transport.writes) != 0 {
			t.Fatal("a repeated Power save did not wait for the original request")
		}
		complete(power, "power_settings", protocol.PowerSettings{Revision: 2, PreventSleep: true})
		if m.Power.Pending || !m.Power.Saved || m.Power.Revision != 2 {
			t.Fatalf("the original reply did not settle Power settings: %+v", m.Power)
		}

		app.Page = shell.SettingsPage
		m.Open(settings.Teams)
		r.syncSettingsPages()
		m.Concurrency = settings.Stored[float64]{Revision: 3, Saved: 4, Draft: 5}
		old := write("save_team_settings", r.saveConcurrency)
		m.ManagedNode = "offline"
		r.syncSettingsPages()
		m.ManagedNode = ""
		r.syncSettingsPages()
		m.Concurrency = settings.Stored[float64]{Revision: 4, Saved: 5, Draft: 7}
		newer := write("save_team_settings", r.saveConcurrency)
		complete(newer, "team_settings", protocol.TeamSettings{Revision: 5, ChildConcurrency: 7})
		complete(old, "team_settings", protocol.TeamSettings{Revision: 4, ChildConcurrency: 5})
		if m.Concurrency.Pending || m.Concurrency.Saved != 7 || m.Concurrency.Draft != 7 || m.Concurrency.Revision != 5 {
			t.Fatalf("an earlier binding rolled back Teams settings: %+v", m.Concurrency)
		}

		m.ManagedNode = "offline"
		r.syncSettingsPages()
		r.acceptTeamRead(protocol.TeamSettings{Revision: 6, ChildConcurrency: 5, Source: &protocol.ConfigurationOrigin{}})
		if m.ConcurrencyReadOnly {
			t.Fatal("a previous host made the current Teams slider read-only")
		}
	})
}

// settingsTestTransport keeps a fixture Node's subscriptions, so a test can
// send what the Node reports after its first snapshot.
type settingsTestTransport struct {
	*conversationTestTransport
	streams   []*conversationTestSubscription
	nodeFault protocol.ErrorCode
}

func (t *settingsTestTransport) Subscribe(ctx context.Context, topic protocol.Topic) (link.Subscription, error) {
	if topic.Kind == "node" {
		t.mu.Lock()
		code := t.nodeFault
		t.mu.Unlock()
		if code != "" {
			return settingsTestFaultSubscription{code: code}, nil
		}
	}
	subscription, err := t.conversationTestTransport.Subscribe(ctx, topic)
	if stream, ok := subscription.(*conversationTestSubscription); ok && topic.Kind == "node" {
		t.mu.Lock()
		t.streams = append(t.streams, stream)
		t.mu.Unlock()
	}
	return subscription, err
}

type settingsTestFaultSubscription struct{ code protocol.ErrorCode }

func (s settingsTestFaultSubscription) Next(context.Context) (protocol.Update, error) {
	return protocol.Update{}, &protocol.Fault{Code: s.code, Message: "fixture observation unavailable"}
}

func (settingsTestFaultSubscription) Close() error { return nil }

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
	root := &Runtime{ctx: t.Context(), controller: controller, App: shell.NewLiveWithAssembly(&viewmodel.Data{Workspace: &viewmodel.Workspace{}}, testassembly.Full())}
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
