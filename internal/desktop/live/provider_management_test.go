package live

import (
	"github.com/sailry/sailry-harness/internal/assembly/desktop"
	"path/filepath"
	"slices"
	"testing"

	"github.com/egoist/mygo/ui"

	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/preferences"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestOfflineProviderSaveShowsOneToast(t *testing.T) {
	controller := client.New(&conversationTestTransport{snapshot: protocol.DefaultSnapshot()})
	defer controller.Close()
	root := &Runtime{controller: controller}
	node := protocol.NodeID{2}.String()
	var result settings.ProviderSaveResult
	root.saveManagedProvider(settings.ProviderDraft{Channel: viewmodel.Channel{Node: node}}, func(value settings.ProviderSaveResult) { result = value })
	if !slices.Equal(result.Failed, []string{node}) || result.Resume == nil {
		t.Fatalf("offline save result: %+v", result)
	}
	var shown []ui.Toast
	window := ui.NewTester(func(c *ui.Context) {
		for _, deliver := range root.queue {
			deliver(c)
		}
		root.queue = nil
		ui.ToastViewportBase(c, func(_ ui.Element, toasts []ui.Toast) { shown = slices.Clone(toasts) })
	}, 320, 240)
	window.SetLocale("en")
	window.Frame()
	window.Frame()
	if len(shown) != 1 || shown[0].Title != i18n.In("en", "provider_save_failed") {
		t.Fatalf("offline save toasts: %+v", shown)
	}
}

func TestProviderGroupForSaveDistinguishesCreationFromMove(t *testing.T) {
	nodeA, nodeB := protocol.NodeID{1}, protocol.NodeID{2}
	provider := protocol.ProviderID{3}
	memberA := preferences.ProviderMember{Node: nodeA, Provider: provider}
	memberB := preferences.ProviderMember{Node: nodeB, Provider: provider}

	t.Run("new channel has no previous owner", func(t *testing.T) {
		group, members := providerGroupForSave(preferences.Data{}, nodeA.String(), provider, 0)
		if group.ID != (protocol.RequestID{}) || len(members) != 0 {
			t.Fatalf("new channel inherited a phantom owner: group=%+v members=%v", group, members)
		}
	})

	t.Run("existing channel retains its owner", func(t *testing.T) {
		group, members := providerGroupForSave(preferences.Data{}, nodeA.String(), provider, 7)
		if group.ID != (protocol.RequestID{}) || len(members) != 1 || members[0] != memberA {
			t.Fatalf("existing channel lost its owner: group=%+v members=%v", group, members)
		}
	})

	t.Run("explicit replicas retain their mapping", func(t *testing.T) {
		id := protocol.NewRequestID()
		data := preferences.Data{ProviderGroups: []preferences.ProviderGroup{{ID: id, Members: []preferences.ProviderMember{memberA, memberB}}}}
		group, members := providerGroupForSave(data, nodeA.String(), provider, 7)
		if group.ID != id || len(members) != 2 || members[0] != memberA || members[1] != memberB {
			t.Fatalf("replica mapping changed: group=%+v members=%v", group, members)
		}
	})
}

func TestProviderTargetExpectedRevisionUsesCapturedPerNodeValues(t *testing.T) {
	targets := map[string]uint64{"node-a": 4, "node-b": 9}
	for _, test := range []struct {
		node string
		want uint64
	}{
		{node: "node-a", want: 4},
		{node: "node-b", want: 9},
		{node: "node-c", want: 0},
	} {
		if got := providerTargetExpectedRevision(test.node, "node-a", 3, targets); got != test.want {
			t.Fatalf("expected revision for %s = %d, want %d", test.node, got, test.want)
		}
	}
	if got := providerTargetExpectedRevision("node-a", "node-a", 3, nil); got != 3 {
		t.Fatalf("source editor revision = %d, want 3", got)
	}
}

func TestProviderGroupProjectionRetainsNodeIDsAndRevisions(t *testing.T) {
	nodeA, nodeB := protocol.NodeID{1}, protocol.NodeID{2}
	providerA, providerB := providerRouteFixture(), providerRouteFixture()
	providerA.ID, providerA.Revision = protocol.ProviderID{3}, 4
	providerA.Credential = &protocol.CredentialRef{Node: nodeA, ID: protocol.CredentialID{5}}
	providerB.ID, providerB.Revision = protocol.ProviderID{6}, 9
	providerB.Credential = &protocol.CredentialRef{Node: nodeB, ID: protocol.CredentialID{7}}

	ownerFor := func(node protocol.NodeID, provider protocol.ConversationProvider) *Runtime {
		snapshot := protocol.DefaultSnapshot()
		snapshot.Node, snapshot.Providers = node, []protocol.ConversationProvider{provider}
		controller := client.New(&conversationTestTransport{snapshot: snapshot})
		if err := controller.Start(t.Context()); err != nil {
			t.Fatal(err)
		}
		r := &Runtime{ctx: t.Context(), controller: controller, authorizations: &authorizationState{credentialLoad: true}, providerPresetsLoaded: true}
		t.Cleanup(func() { r.workers.Wait(); _ = controller.Close() })
		return r
	}
	root, remote := ownerFor(nodeA, providerA), ownerFor(nodeB, providerB)
	root.App = shell.NewLive(&viewmodel.Data{Workspace: &viewmodel.Workspace{}})
	root.App.Page = shell.SettingsPage
	root.App.Settings.Open(settings.Providers)
	root.Preferences = &preferences.Store{Data: preferences.Default()}
	group := protocol.NewRequestID()
	root.Preferences.Data.ProviderGroups = []preferences.ProviderGroup{{ID: group, Members: []preferences.ProviderMember{
		{Node: nodeA, Provider: providerA.ID}, {Node: nodeB, Provider: providerB.ID},
	}}}
	root.bindings = map[string]*Runtime{nodeB.String(): remote}
	remote.parent, remote.App, remote.Preferences = root, root.App, root.Preferences
	root.projectProviderSnapshot()
	channels := root.App.Settings.Channels
	if len(channels) != 1 {
		t.Fatalf("grouped channels = %+v", channels)
	}
	channel := channels[0]
	if channel.Group != group.String() || len(channel.Targets) != 2 || channel.TargetIDs[nodeA.String()] != providerA.ID.String() || channel.TargetIDs[nodeB.String()] != providerB.ID.String() ||
		channel.TargetRevisions[nodeA.String()] != providerA.Revision || channel.TargetRevisions[nodeB.String()] != providerB.Revision {
		t.Fatalf("grouped target mapping lost its per-Node identity or revision: %+v", channel)
	}
	rows := root.App.Settings.ChannelsByNode[nodeB.String()]
	if len(rows) != 1 || rows[0].ID != providerB.ID.String() || rows[0].Revision != providerB.Revision || rows[0].TargetIDs[nodeA.String()] != providerA.ID.String() {
		t.Fatalf("remote settings row lost its own identity or explicit copy target: %+v", rows)
	}
}

func TestManagedProviderRowWritesOnlyItsNode(t *testing.T) {
	nodes := []protocol.NodeID{{1}, {2}}
	providers := []protocol.ConversationProvider{providerRouteFixture(), providerRouteFixture()}
	controllers := make([]*client.Client, len(nodes))
	transports := make([]*conversationTestTransport, len(nodes))
	members := make([]preferences.ProviderMember, len(nodes))
	for i, node := range nodes {
		provider := providers[i]
		provider.ID, provider.Revision = protocol.ProviderID{byte(i + 1)}, uint64(i+1)
		providers[i] = provider
		members[i] = preferences.ProviderMember{Node: node, Provider: provider.ID}
		snapshot := protocol.DefaultSnapshot()
		snapshot.Node, snapshot.Providers = node, []protocol.ConversationProvider{provider}
		transport := &conversationTestTransport{snapshot: snapshot}
		transport.handle = func(request protocol.Request) (link.Response, bool) {
			var changed protocol.ConversationProvider
			switch request.Command.Kind {
			case "save_provider":
				input, _ := detail[protocol.CommandSaveProviderPayload](request.Command.Data)
				changed = input.Provider
			case "put_provider":
				input, _ := detail[protocol.CommandPutProviderPayload](request.Command.Data)
				changed = input.Provider
			case "remove_provider":
				transport.snapshot.Cursor++
				transport.snapshot.Providers = nil
				return link.Response{Output: protocol.Output{Kind: "providers", Data: []protocol.ConversationProvider{}}}, true
			default:
				return link.Response{}, false
			}
			changed.Revision = transport.snapshot.Providers[0].Revision + 1
			transport.snapshot.Cursor++
			transport.snapshot.Providers = []protocol.ConversationProvider{changed}
			return link.Response{Output: protocol.Output{Kind: "provider", Data: changed}}, true
		}
		controller := client.New(transport)
		if err := controller.Start(t.Context()); err != nil {
			t.Fatal(err)
		}
		controllers[i], transports[i] = controller, transport
		t.Cleanup(func() { _ = controller.Close() })
	}
	root, err := NewWithAssembly(t.Context(), controllers[0], preferences.Open(filepath.Join(t.TempDir(), "preferences.json")), desktop.Harness())
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = root.Close() })
	remote := root.newBinding(controllers[1])
	root.bindings = map[string]*Runtime{nodes[1].String(): remote}
	root.Preferences.Data.ProviderGroups = []preferences.ProviderGroup{{ID: protocol.NewRequestID(), Members: members}}
	root.App.Page = shell.SettingsPage
	root.App.Settings.Open(settings.Providers)
	root.App.Settings.ManagedNode = nodes[1].String()
	root.nodeSettingsFrame()
	window := ui.NewTester(func(c *ui.Context) { root.drain(c) }, 320, 240)
	settle := func() {
		for range 5 {
			root.workers.Wait()
			remote.workers.Wait()
			window.Frame()
		}
	}
	rows := root.App.Settings.ChannelsByNode[nodes[1].String()]
	if len(rows) != 1 {
		t.Fatalf("remote channel was missing from settings: %+v", rows)
	}
	row := rows[0]
	row.Name = "Remote edit"
	var result settings.ProviderSaveResult
	actions := root.App.Settings.ProviderActions
	actions.Save(settings.ProviderDraft{Channel: row, Group: row.Group, Targets: []string{row.Node}, ExpectedRevision: row.Revision}, func(saved settings.ProviderSaveResult) { result = saved })
	settle()
	if !slices.Equal(result.Succeeded, []string{row.Node}) || len(result.Failed) != 0 || len(root.Preferences.Data.ProviderGroups) != 1 {
		t.Fatalf("editing one row changed its copy association or targeted another Node: %+v", result)
	}
	actions.Enable(row.Node, row.ID, result.TargetRevisions[row.Node], false)
	settle()
	actions.Remove(row.Node, row.ID, result.TargetRevisions[row.Node]+1)
	settle()
	for i, transport := range transports {
		var writes []string
		transport.mu.Lock()
		for _, request := range transport.requests {
			switch request.Command.Kind {
			case "save_provider", "put_provider", "remove_provider":
				writes = append(writes, request.Command.Kind)
			}
		}
		transport.mu.Unlock()
		var want []string
		if i == 1 {
			want = []string{"save_provider", "put_provider", "remove_provider"}
		}
		if !slices.Equal(writes, want) {
			t.Fatalf("Node %s received writes %v, want %v", nodes[i].String(), writes, want)
		}
	}
	if local := controllers[0].Snapshot().Providers; len(local) != 1 || local[0].ID != providers[0].ID || local[0].Revision != providers[0].Revision {
		t.Fatalf("editing or deleting the remote channel changed its local copy: %+v", local)
	}
}

func TestPartialProviderSaveInspectsOriginalRequest(t *testing.T) {
	t.Run("selected destination", func(t *testing.T) { testPartialProviderSave(t, false, "") })
	t.Run("deselected destination", func(t *testing.T) { testPartialProviderSave(t, true, "") })
	t.Run("terminal result at first save", func(t *testing.T) { testPartialProviderSave(t, true, "first save") })
	t.Run("terminal result during recovery", func(t *testing.T) { testPartialProviderSave(t, true, "recovery") })
}

func testPartialProviderSave(t *testing.T, deselectDestination bool, terminalAt string) {
	t.Helper()
	nodes := []protocol.NodeID{{1}, {2}}
	controllers := make([]*client.Client, len(nodes))
	saves := make([]int, len(nodes))
	var original protocol.RequestID
	var inspected []protocol.RequestID
	var discoveredOn []protocol.NodeID
	var stored protocol.Output
	remoteOutcome := "admitted"
	if terminalAt == "first save" {
		remoteOutcome = "unknown"
	}
	for i, node := range nodes {
		snapshot := protocol.DefaultSnapshot()
		snapshot.Node = node
		transport := &conversationTestTransport{snapshot: snapshot}
		transport.handle = func(request protocol.Request) (link.Response, bool) {
			switch request.Command.Kind {
			case "discover_models":
				discoveredOn = append(discoveredOn, node)
				return link.Response{Output: protocol.Output{Kind: "discovered_models", Data: protocol.DefaultConversationDiscoveryCatalog()}}, true
			case "save_provider":
				saves[i]++
				input, _ := detail[protocol.CommandSaveProviderPayload](request.Command.Data)
				saved := input.Provider
				saved.Revision = 1
				output := protocol.Output{Kind: "provider", Data: saved}
				if i == 0 {
					return link.Response{Output: output}, true
				}
				original, stored = request.ID, output
				return link.Response{Fault: &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "fixture receipt unavailable"}}, true
			case "inspect_request":
				input, _ := detail[protocol.CommandInspectRequestPayload](request.Command.Data)
				inspected = append(inspected, input.ID)
				outcome := protocol.RequestOutcome{Kind: remoteOutcome}
				if remoteOutcome == "completed" {
					outcome.Data = protocol.Result[protocol.Output, protocol.Fault]{Ok: &stored}
				}
				return link.Response{Output: protocol.Output{Kind: "request_outcome", Data: protocol.OutputRequestOutcomePayload{ID: input.ID, Outcome: outcome}}}, true
			}
			return link.Response{}, false
		}
		c := client.New(transport)
		if err := c.Start(t.Context()); err != nil {
			t.Fatal(err)
		}
		controllers[i] = c
		t.Cleanup(func() { _ = c.Close() })
	}
	root, err := NewWithAssembly(t.Context(), controllers[0], preferences.Open(filepath.Join(t.TempDir(), "preferences.json")), desktop.Harness())
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = root.Close() })
	remote := root.newBinding(controllers[1])
	root.bindings = map[string]*Runtime{nodes[1].String(): remote}
	root.initProviders()
	draft := settings.ProviderDraft{Key: "fixture-key", Targets: []string{nodes[0].String(), nodes[1].String()}, Channel: viewmodel.Channel{
		Node: nodes[0].String(), ID: protocol.NewProviderID().String(), Name: "Shared", API: "responses",
	}}
	var result settings.ProviderSaveResult
	done := func(r settings.ProviderSaveResult) { result = r }
	window := ui.NewTester(func(c *ui.Context) { root.drain(c) }, 320, 240)
	settle := func() {
		for range 5 {
			root.workers.Wait()
			remote.workers.Wait()
			window.Frame()
		}
	}
	discovery := draft
	discovery.Targets = draft.Targets[1:]
	discoveryDone := false
	root.App.Settings.ProviderActions.Discover(discovery, func([]viewmodel.ChannelModel) { discoveryDone = true })
	settle()
	if !discoveryDone || !slices.Equal(discoveredOn, []protocol.NodeID{nodes[1]}) {
		t.Fatalf("unsaved channel discovery ran on %v, want only the selected owner", discoveredOn)
	}
	root.saveManagedProvider(draft, done)
	settle()
	wantUncertain, wantUnconfirmed := draft.Targets[1:], []string(nil)
	if terminalAt == "first save" {
		wantUncertain, wantUnconfirmed = nil, draft.Targets[1:]
	}
	if !slices.Equal(result.Succeeded, draft.Targets[:1]) || !slices.Equal(result.Uncertain, wantUncertain) ||
		!slices.Equal(result.Unconfirmed, wantUnconfirmed) || len(result.Failed) != 0 || result.Resume == nil {
		t.Fatalf("partial save: successes %v, uncertain %v, unconfirmed %v, failed %v", result.Succeeded, result.Uncertain, result.Unconfirmed, result.Failed)
	}
	draft.TargetRevisions = result.TargetRevisions
	draft.ExpectedRevision = result.TargetRevisions[nodes[0].String()]
	draft.Channel.TargetIDs = result.TargetIDs
	if terminalAt != "first save" {
		delete(root.bindings, nodes[1].String())
		result.Resume(draft, done)
		root.bindings[nodes[1].String()] = remote
		if !slices.Equal(result.Succeeded, draft.Targets[:1]) || !slices.Equal(result.Uncertain, draft.Targets[1:]) || len(result.Failed) != 0 || result.Resume == nil {
			t.Fatalf("offline retry lost a confirmed save: successes %v, uncertain %v, failed %v", result.Succeeded, result.Uncertain, result.Failed)
		}
	}
	if deselectDestination {
		draft.Targets = draft.Targets[:1]
	}
	if terminalAt == "recovery" {
		remoteOutcome = "unknown"
		result.Resume(draft, done)
		settle()
		if !slices.Equal(result.Succeeded, draft.Targets) || !slices.Equal(result.Unconfirmed, []string{nodes[1].String()}) ||
			len(result.Failed)+len(result.Uncertain) != 0 || result.Resume == nil || !slices.Equal(saves, []int{1, 1}) {
			t.Fatalf("terminal receipt started another save: result %+v, saves %v", result, saves)
		}
	}
	remoteOutcome = "completed"
	result.Resume(draft, done)
	settle()
	wantInspected := []protocol.RequestID{original, original}
	if terminalAt == "first save" {
		wantInspected = wantInspected[:1]
	}
	if !slices.Equal(saves, []int{1, 1}) || original == (protocol.RequestID{}) || !slices.Equal(inspected, wantInspected) {
		t.Fatalf("retry issued %v saves and inspected %v, original %v", saves, inspected, original)
	}
	if !slices.Equal(result.Succeeded, draft.Targets) || len(result.Failed)+len(result.Uncertain)+len(result.Unconfirmed) != 0 ||
		(result.Group == "") != (terminalAt != "") {
		t.Fatalf("recovered save: successes %v, uncertain %v, unconfirmed %v, failed %v, group %q", result.Succeeded, result.Uncertain, result.Unconfirmed, result.Failed, result.Group)
	}
	if groups := root.Preferences.Data.ProviderGroups; terminalAt == "" && (len(groups) != 1 || len(groups[0].Members) != 2 || result.TargetIDs[nodes[1].String()] == "") {
		t.Fatalf("the confirmed copy was missing from the association: %+v", groups)
	}
}

func TestProviderCopyFinishesWhenDestinationDetaches(t *testing.T) {
	local, remote := protocol.NodeID{1}, protocol.NodeID{2}
	source := protocol.DefaultConversationProvider()
	source.ID, source.Revision, source.Name = protocol.ProviderID{3}, 1, "Original channel"
	source.Authentication = protocol.AuthenticationApiKey
	source.Credential = &protocol.CredentialRef{Node: local, ID: protocol.CredentialID{4}}
	key := protocol.NewSecret("fixture-key")
	localTransport := &conversationTestTransport{snapshot: protocol.DefaultSnapshot()}
	localTransport.snapshot.Node = local
	localTransport.snapshot.Providers = []protocol.ConversationProvider{source}
	localTransport.handle = func(request protocol.Request) (link.Response, bool) {
		switch request.Command.Kind {
		case "read_provider_key":
			return link.Response{Output: protocol.Output{Kind: "provider_key", Data: &key}}, true
		case "save_provider":
			input, _ := detail[protocol.CommandSaveProviderPayload](request.Command.Data)
			saved := input.Provider
			saved.Revision = input.ExpectedRevision + 1
			return link.Response{Output: protocol.Output{Kind: "provider", Data: saved}}, true
		}
		return link.Response{}, false
	}
	remoteTransport := &conversationTestTransport{snapshot: protocol.DefaultSnapshot()}
	remoteTransport.snapshot.Node = remote
	controller, peer := client.New(localTransport), client.New(remoteTransport)
	for _, c := range []*client.Client{controller, peer} {
		t.Cleanup(func() { _ = c.Close() })
		if err := c.Start(t.Context()); err != nil {
			t.Fatal(err)
		}
	}
	root, err := NewWithAssembly(t.Context(), controller, preferences.Open(filepath.Join(t.TempDir(), "preferences.json")), desktop.Harness())
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = root.Close() })
	bound := root.newBinding(peer)
	root.bindings = map[string]*Runtime{remote.String(): bound}
	targets := []string{local.String(), remote.String()}
	draft := settings.ProviderDraft{ExpectedRevision: source.Revision, Targets: targets, Channel: viewmodel.Channel{
		Node: local.String(), ID: source.ID.String(), Name: "Updated channel", API: "responses",
	}}
	var result settings.ProviderSaveResult
	root.saveManagedProvider(draft, func(value settings.ProviderSaveResult) { result = value })
	root.workers.Wait()
	localTransport.mu.Lock()
	readKey := slices.ContainsFunc(localTransport.requests, func(request protocol.Request) bool { return request.Command.Kind == "read_provider_key" })
	localTransport.mu.Unlock()
	if !readKey {
		t.Fatal("the copy did not read the existing source key")
	}
	// The source read has answered, but its frame has not started the copies.
	if err := bound.Close(); err != nil {
		t.Fatal(err)
	}
	delete(root.bindings, remote.String())
	window := ui.NewTester(func(c *ui.Context) { root.drain(c) }, 320, 240)
	for range 5 {
		root.workers.Wait()
		window.Frame()
	}
	if !slices.Equal(result.Succeeded, targets[:1]) || !slices.Equal(result.Failed, targets[1:]) || result.Resume == nil {
		t.Fatalf("detached copy did not settle: successes %v, failed %v", result.Succeeded, result.Failed)
	}
	remoteTransport.mu.Lock()
	defer remoteTransport.mu.Unlock()
	if slices.ContainsFunc(remoteTransport.requests, func(request protocol.Request) bool { return request.Command.Kind == "save_provider" }) {
		t.Fatal("a provider write escaped to the detached binding")
	}
}

func TestProviderSaveKeyPreservesExistingReplicas(t *testing.T) {
	existing := &protocol.CredentialRef{Node: protocol.NodeID{1}, ID: protocol.CredentialID{2}}
	if key := providerSaveKey("", "source-key", existing); key != nil {
		t.Fatal("expanding a group rotated an existing replica's credential")
	}
	if key := providerSaveKey("", "source-key", nil); key == nil || key.Expose() != "source-key" {
		t.Fatal("new replica did not inherit the source key")
	}
	if key := providerSaveKey("edited-key", "source-key", existing); key == nil || key.Expose() != "edited-key" {
		t.Fatal("explicit key edit did not reach the existing replica")
	}
}
