package live

import (
	"encoding/json"
	"slices"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	desktopplugins "github.com/sailry/sailry-harness/internal/desktop/plugins"
	"github.com/sailry/sailry-harness/internal/desktop/preferences"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	view "github.com/sailry/sailry-harness/internal/desktop/viewapi"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestWindowProviderAccountLookup(t *testing.T) {
	snap := protocol.DefaultSnapshot()
	snap.Node = protocol.NodeID{1}
	id, credID := protocol.NewProviderID(), protocol.NewCredentialID()
	snap.Providers = []protocol.ConversationProvider{{
		ID: id, Revision: 3, Authentication: protocol.AuthenticationClaudeOAuth,
		Credential: &protocol.CredentialRef{ID: credID}, Models: make([]protocol.ConversationModel, 1000),
	}}
	source := &terminalTabsTestNode{pluginViewTestNode: pluginViewTestNode{snapshot: snap, updates: make(chan protocol.Update, 1)}}
	r, services, _ := newWindowServicesFixture(t, source, nil)
	account, ok := services.ProviderAccount(snap.Node, id)
	if !ok || account.Credential != credID || account.ProviderRevision != 3 || account.Authentication != protocol.AuthenticationClaudeOAuth || account.CredentialKnown {
		t.Fatalf("provider binding before credential metadata loaded: %+v, %v", account, ok)
	}
	r.authorizations = &authorizationState{
		credentialKey: "v1",
		credentials: map[protocol.CredentialID]protocol.Credential{
			credID: {ID: credID, Provider: id, Revision: 5},
		},
	}
	tester := ui.NewTester(r.App.View, 960, 600)
	tester.Frame()
	if allocs := testing.AllocsPerRun(100, func() {
		account, ok = services.ProviderAccount(snap.Node, id)
	}); allocs != 0 || !ok || !account.CredentialKnown || account.CredentialRevision != 5 {
		t.Fatalf("unchanged lookup: %+v, %v, %.1f allocs", account, ok, allocs)
	}
	r.authorizations.credentialKey = "v2"
	r.authorizations.credentials[credID] = protocol.Credential{ID: credID, Provider: id, Revision: 6, Revoked: true}
	tester.Frame()
	if _, ok := services.ProviderAccount(snap.Node, id); ok {
		t.Fatal("revoked credential still available")
	}
}

func TestWindowServicesMultiNodeAndUsage(t *testing.T) {
	localNodeID := protocol.NodeID{1}
	remoteNodeID := protocol.NodeID{2}
	sessionID := protocol.SessionID{9}

	localSnap := protocol.DefaultSnapshot()
	localSnap.Node = localNodeID
	localSession := protocol.DefaultSession()
	localSession.ID = sessionID
	localSnap.Sessions = []protocol.Session{localSession}

	remoteSnap := protocol.DefaultSnapshot()
	remoteSnap.Node = remoteNodeID
	remoteProj := protocol.DefaultProject()
	remoteProj.ID = protocol.ProjectID{4}
	remoteProj.Name = "Remote Project"
	remoteSnap.Projects = []protocol.Project{remoteProj}
	remoteProvID := protocol.NewProviderID()
	displayName := "Claude Sonnet"
	remoteProv := protocol.ConversationProvider{
		ID:           remoteProvID,
		Name:         "Anthropic Remote",
		Api:          protocol.ConversationModelApiAnthropic,
		Enabled:      true,
		DefaultModel: "claude-sonnet",
		Models: []protocol.ConversationModel{{
			ID:            "claude-sonnet",
			DisplayName:   &displayName,
			Reasoning:     true,
			Efforts:       []protocol.Effort{{Kind: "low"}, {Kind: "high"}},
			DefaultEffort: protocol.Effort{Kind: "high"},
		}},
	}
	disabledProv := protocol.ConversationProvider{
		ID:      protocol.NewProviderID(),
		Name:    "Disabled",
		Api:     protocol.ConversationModelApiResponses,
		Enabled: false,
		Models:  []protocol.ConversationModel{{ID: "hidden-model"}},
	}
	remoteSnap.Providers = []protocol.ConversationProvider{remoteProv, disabledProv}

	makeReport := func(node protocol.NodeID, query protocol.UsageQuery, responses uint64) protocol.UsageReport {
		report := protocol.DefaultUsageReport()
		report.Node = node
		report.Query = query
		report.Cursor = responses
		report.Totals.Responses = responses
		report.Totals.Tokens = &protocol.ConversationUsage{Input: responses * 100, Output: responses * 20}
		return report
	}

	localSource := &terminalTabsTestNode{pluginViewTestNode: pluginViewTestNode{snapshot: localSnap, updates: make(chan protocol.Update, 4)}}
	localSource.respond = func(req protocol.Request) link.Response {
		switch req.Command.Kind {
		case "read_usage":
			query, _ := detail[protocol.UsageQuery](req.Command.Data)
			return link.Response{Output: protocol.Output{Kind: "usage", Data: makeReport(localNodeID, query, 3)}}
		case "list_reminders":
			return link.Response{Output: protocol.Output{Kind: "reminders", Data: protocol.ReminderPage{Items: []protocol.ReminderEntry{}}}}
		}
		return link.Response{Fault: &protocol.Fault{Code: protocol.ErrorCodeUnavailable}}
	}

	remoteSource := &terminalTabsTestNode{pluginViewTestNode: pluginViewTestNode{snapshot: remoteSnap, updates: make(chan protocol.Update, 4)}}
	remoteSource.respond = func(req protocol.Request) link.Response {
		switch req.Command.Kind {
		case "read_usage":
			query, _ := detail[protocol.UsageQuery](req.Command.Data)
			return link.Response{Output: protocol.Output{Kind: "usage", Data: makeReport(remoteNodeID, query, 5)}}
		case "remove_reminder":
			return link.Response{Output: protocol.Output{Kind: "reminder_removed", Data: protocol.ReminderRemoved{ID: "r1", Removed: true}}}
		case "inspect_request":
			query, _ := detail[protocol.CommandInspectRequestPayload](req.Command.Data)
			out := protocol.Output{Kind: "reminder_removed", Data: protocol.ReminderRemoved{ID: "r1", Removed: true}}
			outcome := protocol.RequestOutcome{Kind: "completed", Data: protocol.Result[protocol.Output, protocol.Fault]{Ok: &out}}
			return link.Response{Output: protocol.Output{Kind: "request_outcome", Data: protocol.OutputRequestOutcomePayload{ID: query.ID, Outcome: outcome}}}
		}
		return link.Response{Fault: &protocol.Fault{Code: protocol.ErrorCodeUnavailable}}
	}

	remoteClient := client.New(link.NewLocal(remoteNodeID, remoteNodeID, remoteSource))
	t.Cleanup(func() { _ = remoteClient.Close() })
	if err := remoteClient.Start(t.Context()); err != nil {
		t.Fatal(err)
	}

	r, services, _ := newWindowServicesFixture(t, localSource, nil)
	r.bindings = map[string]*Runtime{remoteNodeID.String(): r.newBinding(remoteClient)}
	credID := protocol.NewCredentialID()
	// Unloaded authorizations (empty credentialKey, even with initialized map) must report nil Credentials.
	r.authorizations = &authorizationState{
		credentials: map[protocol.CredentialID]protocol.Credential{},
	}

	tester := ui.NewTester(r.App.View, 960, 600)
	tester.SetLocale("en")
	tester.Frame()

	if unloaded := services.Credentials(localNodeID); unloaded != nil {
		t.Fatalf("Credentials(local) before load = %+v, want nil", unloaded)
	}

	r.authorizations.credentialKey = "v1"
	r.authorizations.credentials[credID] = protocol.Credential{ID: credID, Revision: 4}
	tester.Frame()

	nodes := services.Nodes()
	if len(nodes) != 2 || !nodes[0].Local || nodes[0].ID != localNodeID || nodes[0].Label != "Local" ||
		nodes[1].Local || nodes[1].ID != remoteNodeID || !nodes[1].Connected || nodes[1].Label != remoteNodeID.String()[:8] {
		t.Fatalf("Nodes() = %+v", nodes)
	}
	unknownNodeID := protocol.NodeID{0xaa, 0xbb, 0xcc, 0xdd}
	if got := services.NodeLabel(localNodeID); got != "Local" {
		t.Fatalf("NodeLabel(local) = %q, want Local", got)
	}
	if got := services.NodeLabel(protocol.NodeID{}); got != "Local" {
		t.Fatalf("NodeLabel(zero) = %q, want Local", got)
	}
	if got := services.NodeLabel(unknownNodeID); got != unknownNodeID.String()[:8] {
		t.Fatalf("NodeLabel(unknown) = %q, want %q", got, unknownNodeID.String()[:8])
	}
	// Remembered/named peer resolves identically across Nodes, NodeLabel, Usage.Sources, Usage.Inventory, providerHosts, composerHosts, and pluginUsageSources.
	offlinePeerID := protocol.NodeID{0x55, 0x66, 0x77, 0x88}
	offlinePeerName := "Offline Lab"
	devices := map[string]preferences.Device{
		offlinePeerID.String(): {Name: &offlinePeerName, Execution: true},
	}
	r.Preferences.Data.Devices = &devices
	r.resourceCatalog = &protocol.ResourceCatalog{
		Entry: localNodeID,
		Sources: []protocol.ResourceSource{{
			Node: remoteNodeID,
			Name: "Build Box",
		}},
	}
	tester.Frame()
	if got := services.NodeLabel(offlinePeerID); got != "Offline Lab" {
		t.Fatalf("NodeLabel(remembered offline peer) = %q, want Offline Lab", got)
	}
	if got := services.NodeLabel(remoteNodeID); got != "Build Box" {
		t.Fatalf("NodeLabel(named remote) = %q, want Build Box", got)
	}
	if nodes := services.Nodes(); len(nodes) != 2 || nodes[1].Label != "Build Box" {
		t.Fatalf("Nodes() after peer rename = %+v", nodes)
	}
	if sources := services.Usage.Sources(); len(sources) != 2 || sources[0].Label != "Local" || sources[1].Label != "Build Box" {
		t.Fatalf("Usage.Sources() labels = %+v", sources)
	}
	if inv, ok := services.Usage.Inventory(remoteNodeID); !ok || inv.Label != "Build Box" {
		t.Fatalf("Usage.Inventory(remote) label = %+v, %v", inv, ok)
	}
	if opts := services.ModelOptions(remoteNodeID); len(opts) != 1 ||
		opts[0].ID != remoteProvID.String()+"/claude-sonnet" ||
		opts[0].Name != "Claude Sonnet" ||
		opts[0].Provider != "Anthropic Remote" ||
		opts[0].Brand != "anthropic" ||
		opts[0].Mark == nil ||
		opts[0].DefaultEffort != "high" ||
		!slices.Equal(opts[0].Efforts, []string{"low", "high"}) {
		t.Fatalf("ModelOptions(remote) = %+v", opts)
	}
	if ph := r.providerHosts(); len(ph) != 2 || ph[0].Name != "local_host_name" || ph[1].Name != "Build Box" {
		t.Fatalf("providerHosts() = %+v", ph)
	}
	if ps := r.pluginUsageSources(); len(ps) != 2 || ps[0].Label != "Local" || ps[1].Label != "Build Box" {
		t.Fatalf("pluginUsageSources() = %+v", ps)
	}

	// Dynamic locale change updates local host label without stale caching and fires OnChange.
	localeChanges := 0
	var localeMu sync.Mutex
	unsubLocale := services.OnChange(func() {
		localeMu.Lock()
		localeChanges++
		localeMu.Unlock()
	})
	tester.SetLocale("zh-CN")
	tester.Frame()
	unsubLocale()
	if got := services.NodeLabel(localNodeID); got != "本地" {
		t.Fatalf("NodeLabel(local, zh-CN) = %q, want 本地", got)
	}
	if nodes := services.Nodes(); len(nodes) != 2 || nodes[0].Label != "本地" || nodes[1].Label != "Build Box" {
		t.Fatalf("Nodes() in zh-CN = %+v", nodes)
	}
	localeMu.Lock()
	gotLocaleChanges := localeChanges
	localeMu.Unlock()
	if gotLocaleChanges == 0 {
		t.Fatal("OnChange did not fire when active locale changed local host label")
	}
	tester.SetLocale("en")
	tester.Frame()
	if snap, ok := services.Snapshot(remoteNodeID); !ok || snap.Node != remoteNodeID || len(snap.Projects) != 1 {
		t.Fatalf("Snapshot(remote) = %+v, %v", snap, ok)
	}
	creds := services.Credentials(localNodeID)
	if len(creds) != 1 || creds[credID].Revision != 4 {
		t.Fatalf("Credentials(local) = %+v", creds)
	}

	req, err := services.PrepareOn(remoteNodeID, protocol.Command{Kind: "remove_reminder", Data: protocol.CommandRemoveReminderPayload{ID: "r1"}})
	if err != nil || req.Target != remoteNodeID {
		t.Fatalf("PrepareOn(remote) = %+v, %v", req, err)
	}
	if out, err := services.ExecuteOn(t.Context(), remoteNodeID, req); err != nil || out.Kind != "reminder_removed" {
		t.Fatalf("ExecuteOn(remote) = %+v, %v", out, err)
	}
	if out, err := services.SettleOn(t.Context(), remoteNodeID, req); err != nil || out.Kind != "reminder_removed" {
		t.Fatalf("SettleOn(remote) = %+v, %v", out, err)
	}
	if receipt, err := services.ResolveOn(t.Context(), remoteNodeID, req); err != nil || receipt.State != "applied" {
		t.Fatalf("ResolveOn(remote) = %+v, %v", receipt, err)
	}
	if outcome, err := services.OutcomeOn(t.Context(), remoteNodeID, req); err != nil || outcome.Kind != "completed" {
		t.Fatalf("OutcomeOn(remote) = %+v, %v", outcome, err)
	}

	// OnChange fires on snapshot updates and stops after unsubscribe.
	changeCh := make(chan struct{}, 8)
	var changeCount int
	var changeMu sync.Mutex
	unsub := services.OnChange(func() {
		changeMu.Lock()
		changeCount++
		changeMu.Unlock()
		select {
		case changeCh <- struct{}{}:
		default:
		}
	})
	updated := localSnap
	updated.Cursor++
	localSource.updates <- protocol.Update{Kind: "snapshot", Data: updated}
	awaitWindowValue(t, changeCh)
	unsub()
	unsub()
	changeMu.Lock()
	gotChanges := changeCount
	changeMu.Unlock()

	updated.Cursor++
	localSource.updates <- protocol.Update{Kind: "snapshot", Data: updated}
	deadline := time.Now().Add(2 * time.Second)
	for r.controller.Snapshot().Cursor != updated.Cursor && time.Now().Before(deadline) {
		time.Sleep(5 * time.Millisecond)
	}
	changeMu.Lock()
	afterUnsub := changeCount
	changeMu.Unlock()
	if afterUnsub != gotChanges {
		t.Fatalf("OnChange fired after unsubscribe: before=%d after=%d", gotChanges, afterUnsub)
	}

	// OpenSession and RevealDetails queue UI work safely.
	services.OpenSession(localNodeID, sessionID)
	services.RevealDetails()
	tester.Frame()
	if r.App.Sidebar.Selected != sessionID.String() {
		t.Fatalf("OpenSession did not select session: got %q", r.App.Sidebar.Selected)
	}

	// Live Usage services: Sources, Inventory, ReadUsage, WatchOverview (aggregated across both nodes).
	if sources := services.Usage.Sources(); len(sources) != 2 {
		t.Fatalf("Usage.Sources() = %+v", sources)
	}
	if inv, ok := services.Usage.Inventory(remoteNodeID); !ok || inv.Node != remoteNodeID || len(inv.Projects) != 1 {
		t.Fatalf("Usage.Inventory(remote) = %+v, %v", inv, ok)
	}
	query := protocol.DefaultUsageQuery()
	query.StartMs, query.EndMs, query.Dimension = 0, 86400000, protocol.UsageDimensionModel
	if report, err := services.Usage.ReadUsage(t.Context(), remoteNodeID, query); err != nil || report.Totals.Responses != 5 {
		t.Fatalf("Usage.ReadUsage(remote) = %+v, %v", report, err)
	}
	if _, demo := services.Usage.Sample(); demo {
		t.Fatal("Usage.Sample() reported active outside Demo mode")
	}

	completeCh := make(chan view.UsageOverview, 4)
	watch, err := services.Usage.WatchOverview(t.Context(), protocol.NodeID{}, query, func(ov view.UsageOverview, err error) {
		if err == nil && ov.Complete && ov.Summary != nil {
			select {
			case completeCh <- ov:
			default:
			}
		}
	})
	if err != nil {
		t.Fatalf("Usage.WatchOverview: %v", err)
	}
	gotOverview := awaitWindowValue(t, completeCh)
	if gotOverview.Summary == nil || gotOverview.Summary.Totals.Responses != 8 {
		t.Fatalf("aggregated WatchOverview responses=%+v want 8", gotOverview.Summary)
	}
	watch.Close()
	watch.Close()

	// Demo mode: Usage.Sample activates, serves DemoUsage without hitting Nodes, and OnChange fires on toggle/locale change.
	demoChanges := 0
	unsubDemo := services.OnChange(func() {
		changeMu.Lock()
		demoChanges++
		changeMu.Unlock()
	})
	defer unsubDemo()

	r.App.Settings.DemoMode = true
	tester.Frame()
	sample1, ok := services.Usage.Sample()
	if !ok || sample1 == nil {
		t.Fatal("Usage.Sample() was unavailable in Demo mode")
	}
	if sources := services.Usage.Sources(); len(sources) != 1 || sources[0].ID != localNodeID {
		t.Fatalf("Demo Usage.Sources() = %+v", sources)
	}
	if inv, ok := services.Usage.Inventory(protocol.NodeID{}); !ok || len(inv.Providers) == 0 {
		t.Fatalf("Demo Usage.Inventory() = %+v, %v", inv, ok)
	}
	demoOverviewCh := make(chan view.UsageOverview, 2)
	demoWatch, err := services.Usage.WatchOverview(t.Context(), protocol.NodeID{}, query, func(ov view.UsageOverview, err error) {
		if err == nil {
			select {
			case demoOverviewCh <- ov:
			default:
			}
		}
	})
	if err != nil {
		t.Fatalf("Demo WatchOverview: %v", err)
	}
	demoOv := awaitWindowValue(t, demoOverviewCh)
	if !demoOv.Complete || demoOv.Summary == nil {
		t.Fatal("Demo WatchOverview did not emit a complete overview")
	}
	demoWatch.Close()

	tester.SetLocale("zh-CN")
	tester.Frame()
	sample2, ok := services.Usage.Sample()
	if !ok || sample2 == nil || sample2.Revision == sample1.Revision {
		t.Fatalf("Demo locale switch did not advance sample revision: %+v vs %+v", sample1, sample2)
	}
	changeMu.Lock()
	gotDemoChanges := demoChanges
	changeMu.Unlock()
	if gotDemoChanges < 2 {
		t.Fatalf("expected OnChange on Demo toggle and locale change, got %d", gotDemoChanges)
	}

	if err := r.Close(); err != nil {
		t.Fatal(err)
	}
}

func TestNativeMetricContributionsAndLegacyReplacement(t *testing.T) {
	nodeID := protocol.NodeID{3}
	providerID := protocol.NewProviderID()
	credID := protocol.NewCredentialID()

	snap := protocol.DefaultSnapshot()
	snap.Node = nodeID
	provider := protocol.ConversationProvider{
		ID:             providerID,
		Revision:       5,
		Name:           "Codex",
		Authentication: protocol.AuthenticationChatGptOAuth,
		Enabled:        true,
		DefaultModel:   "gpt-5",
		Credential:     &protocol.CredentialRef{Node: nodeID, ID: credID},
	}
	snap.Providers = []protocol.ConversationProvider{provider}

	source := &terminalTabsTestNode{
		pluginViewTestNode: pluginViewTestNode{snapshot: snap, updates: make(chan protocol.Update)},
		respond: func(protocol.Request) link.Response {
			return link.Response{Fault: &protocol.Fault{Code: protocol.ErrorCodeUnavailable}}
		},
	}
	controller := client.New(link.NewLocal(nodeID, nodeID, source))
	t.Cleanup(func() { _ = controller.Close() })
	if err := controller.Start(t.Context()); err != nil {
		t.Fatal(err)
	}

	var demoScopes []shell.ComposerMetricScope
	assembly := shell.Assemble(func(a *shell.Assembly) {
		a.PluginControlReplacements = append(a.PluginControlReplacements,
			shell.PluginControlReplacement{Package: "provider-status", Slot: protocol.PluginUiSlotFooter, ID: "usage"},
			shell.PluginControlReplacement{Package: "provider-status", Slot: protocol.PluginUiSlotProvider, ID: "provider_usage"},
			shell.PluginControlReplacement{Package: "statistics", Slot: protocol.PluginUiSlotStatistics, ID: "composer_tokens"},
		)
		a.MetricContributions = append(a.MetricContributions, shell.MetricContribution{
			Bind: func(shell.WindowContext) shell.MetricBinding {
				return shell.MetricBinding{
					Lead: func(_ *ui.Context, scope shell.ComposerMetricScope) []conversation.Metric {
						return []conversation.Metric{{ID: "native_lead", Order: 10, Label: "Native Lead", Value: "99%"}}
					},
					Statistics: func(_ *ui.Context, scope shell.ComposerMetricScope) []conversation.Metric {
						if scope.Demo {
							demoScopes = append(demoScopes, scope)
						}
						return []conversation.Metric{{ID: "statistics:composer_tokens", Order: 0, Label: "Tokens", Value: "12.3k"}}
					},
					Provider: func(_ *ui.Context, node, channel string) *settings.ChannelUsage {
						if channel == providerID.String() {
							return &settings.ChannelUsage{Lines: []settings.UsageLine{{Value: "88%"}}}
						}
						return nil
					},
				}
			},
		})
	})

	root, err := NewWithProfile(t.Context(), controller, t.TempDir(), assembly)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = root.Close() })

	// Replaced official legacy controls must be skipped in pluginMetrics while third-party controls stay visible.
	registry := desktopplugins.NewRegistry(desktopplugins.RegistryOptions{
		Node: nodeID, Context: protocol.PluginContext{Surface: protocol.PluginDesktopSurface{Kind: "composer"}},
	})
	t.Cleanup(func() { _ = registry.Close() })

	var legacyDecl []protocol.PluginUiContribution
	if err := json.Unmarshal([]byte(`[
		{"id":"usage","slot":"footer","kind":"metric","label":{"label":"Legacy Usage"}},
		{"id":"composer_tokens","slot":"statistics","kind":"metric","label":{"label":"Legacy Tokens"}}
	]`), &legacyDecl); err != nil {
		t.Fatal(err)
	}
	officialStatus := protocol.PluginReference{Name: "provider-status", Digest: "9a9b74e9e34eef2aa5c04d9f79e0877889e71e14a077c967625eacaf9d934404"}
	officialStats := protocol.PluginReference{Name: "statistics", Digest: "057028a0b6908707a38f61d4c4f12127fa166b4f45e914dfd2301fa227809ccf"}
	// Same package name "statistics" with an unverified digest exercises
	// third-party coexistence: must NOT panic on duplicate UI element key.
	thirdParty := protocol.PluginReference{Name: "statistics", Digest: strings.Repeat("c", 64)}

	for _, entry := range []struct {
		ref  protocol.PluginReference
		decl []protocol.PluginUiContribution
		pub  string
	}{
		{officialStatus, legacyDecl[:1], `[{"id":"usage","visible":true,"enabled":true,"value":"legacy-footer"}]`},
		{officialStats, legacyDecl[1:], `[{"id":"composer_tokens","visible":true,"enabled":true,"value":"legacy-stat"}]`},
		{thirdParty, legacyDecl[1:], `[{"id":"composer_tokens","visible":true,"enabled":true,"value":"third-party-stat"}]`},
	} {
		hooks, err := registry.Hooks(entry.ref, entry.decl)
		if err != nil {
			t.Fatal(err)
		}
		if err := registry.SetActivity(entry.ref, true, false); err != nil {
			t.Fatal(err)
		}
		if err := hooks.Publish([]byte(entry.pub)); err != nil {
			t.Fatal(err)
		}
	}

	var comp composer.State
	root.projectComposerMetrics(&comp, registry, shell.ComposerMetricScope{
		Node: nodeID.String(), Provider: providerID.String(), Connected: true, HasMessages: true,
	})
	tester := ui.NewTester(func(c *ui.Context) {
		theme.Apply(c, "", theme.Light)
		open := false
		comp.Metrics(c, false, &open)
	}, 960, 600)
	tester.SetLocale("en")
	tester.Frame()

	if !tester.HasText("99%") || !tester.HasText("12.3k") || !tester.HasText("third-party-stat") {
		t.Fatal("combined native + third-party composer metrics did not render")
	}
	if tester.HasText("legacy-footer") || tester.HasText("legacy-stat") {
		t.Fatal("replaced legacy plugin metrics duplicated native metrics")
	}

	// Native ProviderUsage takes precedence on Settings › Providers.
	if usage := root.providerUsage(nil, nodeID.String(), providerID.String()); usage == nil || len(usage.Lines) != 1 || usage.Lines[0].Value != "88%" {
		t.Fatalf("native providerUsage = %+v", usage)
	}

	// Demo mode invokes native MetricStatistics without requiring moejs!
	root.App.Settings.DemoMode = true
	appTester := ui.NewTester(root.App.View, 960, 600)
	appTester.SetLocale("en")
	appTester.Frame()
	if !slices.ContainsFunc(demoScopes, func(s shell.ComposerMetricScope) bool {
		return s.Demo && s.Connected && s.HasMessages && s.Statistics != nil
	}) {
		t.Fatalf("Demo mode did not invoke native MetricStatistics with synthetic statistics: %+v", demoScopes)
	}
	if !appTester.HasText("12.3k") {
		t.Fatal("Demo conversation did not display native statistics metric")
	}
}
