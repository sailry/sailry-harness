package settings

import (
	"reflect"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

func TestSavedProviderProtocolAndKey(t *testing.T) {
	m := NewModel(preview.SettingsData())
	m.Live = true
	m.Brands = []preview.Brand{{ID: "fixture", Methods: []preview.Method{{ID: "compatible", Access: "provider_access_endpoint", Keyed: true,
		Routes: []preview.Route{{ID: "compatible/0", API: "responses", Custom: true}, {ID: "compatible/1", API: "chat_completions", Custom: true}},
	}}}}
	models := []preview.ChannelModel{{ID: "model-a", Name: "model-a", Context: 128000, DefaultEffort: "default"}, {ID: "model-b", Name: "model-b", Context: 200000, DefaultEffort: "default"}}
	m.Channels = []preview.Channel{{ID: "saved", Revision: 7, Name: "Saved", Brand: "fixture", Method: "compatible/1", API: "chat_completions",
		Endpoint: "https://provider.example/v1", Auth: preview.AuthKey, KeySet: true, Models: models, Default: 1}}
	var read func(string)
	m.ProviderActions.ReadKey = func(_, _ string, done func(string)) { read = done }
	var saved []ProviderDraft
	m.ProviderActions.Save = func(draft ProviderDraft, done func(ProviderSaveResult)) {
		saved = append(saved, draft)
		done(ProviderSaveResult{Failed: []string{"fixture-node"}})
	}
	e := &m.provider
	e.edit(m, &m.Channels[0])
	read("fixture-key")
	tt := ui.NewTesterWith(func(c *ui.Context) {
		theme.Apply(c, "", theme.Dark)
		providerEditorDialog(c, m)
	}, 1100, 900, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true, ReduceMotion: true}})
	tt.SetLocale("en")
	tt.Frame()
	tt.Frame()
	click := func(role ui.Role, label string) {
		t.Helper()
		entry, ok := tt.AccessByRole(role, label)
		if !ok || entry.Disabled {
			t.Fatalf("missing or disabled editor control %q", label)
		}
		tt.ClickAt(entry.Bounds.X+entry.Bounds.W/2, entry.Bounds.Y+entry.Bounds.H/2)
		tt.Advance(time.Second)
		tt.Frame()
	}
	keyLabel := i18n.In("en", "provider_api_key")
	field, ok := tt.AccessByRole(ui.RoleTextField, keyLabel)
	if !ok || !field.Password || field.Value == "fixture-key" {
		t.Fatal("a saved API key is not masked by default")
	}
	click(ui.RoleButton, i18n.In("en", "mcp_show_secret"))
	field, _ = tt.AccessByRole(ui.RoleTextField, keyLabel)
	if field.Password || field.Value != "fixture-key" {
		t.Fatal("the inline eye did not reveal the stored key")
	}
	click(ui.RoleButton, i18n.In("en", "mcp_hide_secret"))
	for _, api := range []string{"Responses", "Chat Completions", "Responses"} {
		click(ui.RoleRadio, api)
		click(ui.RoleButton, i18n.In("en", "provider_save"))
		draft := saved[len(saved)-1]
		wantAPI := "responses"
		if api == "Chat Completions" {
			wantAPI = "chat_completions"
		}
		if draft.Channel.API != wantAPI || draft.Key != "" || draft.ExpectedRevision != 7 || draft.Channel.ID != "saved" ||
			draft.Channel.Endpoint != "https://provider.example/v1" || draft.Channel.Default != 1 || !reflect.DeepEqual(draft.Channel.Models, models) {
			t.Fatal("changing protocol replaced a stored key, identity, endpoint, model metadata or default model")
		}
	}
	e.key = "replacement-fixture-key"
	if problem := e.save(m); problem != "" || saved[len(saved)-1].Key != e.key {
		t.Fatal("a replacement key was not included in the save")
	}
}

func TestSavedProviderKeyReadKeepsDraft(t *testing.T) {
	m := NewModel(preview.SettingsData())
	m.Live = true
	var reads []func(string)
	m.ProviderActions.ReadKey = func(_, _ string, done func(string)) { reads = append(reads, done) }
	channel := preview.Channel{ID: "saved", Auth: preview.AuthKey, KeySet: true}
	e := &m.provider
	e.edit(m, &channel)
	e.key = "typed-fixture-key"
	reads[0]("stored-fixture-key")
	if e.key != "typed-fixture-key" || e.keyUpdate() != "typed-fixture-key" {
		t.Fatal("a late stored-key read replaced the user's draft")
	}
	e.edit(m, &channel)
	reads[0]("stale-fixture-key")
	if e.key != "" {
		t.Fatal("a stored-key read from the previous editor was accepted")
	}
	reads[1]("stored-fixture-key")
	if e.key != "stored-fixture-key" || e.keyUpdate() != "" || e.keyShown {
		t.Fatal("the current stored-key read was not retained masked and unchanged")
	}
}

func TestProviderEditorKeepsConfigurationIsolated(t *testing.T) {
	m := NewModel(preview.SettingsData())
	m.Live = true
	m.Brands = []preview.Brand{{ID: "fixture", Methods: []preview.Method{{ID: "compatible", API: "responses", Custom: true, Keyed: true}}}}
	ch := preview.Channel{ID: "shared", Name: "Shared", Brand: "fixture", Method: "compatible", API: "responses", Endpoint: "https://fixture.test", Auth: preview.AuthKey, Shared: true, KeySet: true, Models: []preview.ChannelModel{{ID: "model", Name: "Model", Context: 128000}}}
	m.ProviderActions.Save = func(draft ProviderDraft, done func(ProviderSaveResult)) {
		if draft.Channel.Shared {
			t.Error("editor retained obsolete host sharing")
		}
		done(ProviderSaveResult{Failed: []string{"fixture-node"}})
	}
	m.provider.edit(m, &ch)
	if problem := m.provider.save(m); problem != "" {
		t.Fatal(problem)
	}
}

func TestFollowedProviderEditorDoesNotReadCopiedKey(t *testing.T) {
	m := NewModel(preview.SettingsData())
	m.Live = true
	m.ProviderActions.ReadKey = func(string, string, func(string)) { t.Fatal("readonly copy attempted key read") }
	m.provider.edit(m, &preview.Channel{ID: "copy", Auth: preview.AuthKey, ConfigurationReadOnly: true, Shared: true})
}

func TestProviderTargetsRespectAuthentication(t *testing.T) {
	if !providerAuthAllowsMultipleTargets(preview.AuthKey) || !providerAuthAllowsMultipleTargets(preview.AuthNone) {
		t.Fatal("static and unauthenticated channels should allow multiple hosts")
	}
	for _, auth := range []preview.Auth{preview.AuthOAuth, preview.AuthPlugin} {
		if providerAuthAllowsMultipleTargets(auth) {
			t.Fatalf("auth kind %d should remain single-host", auth)
		}
	}

	got := toggleProviderTarget([]string{"a"}, "b", true, true)
	if !reflect.DeepEqual(got, []string{"a", "b"}) {
		t.Fatalf("multi-host selection = %v", got)
	}
	got = toggleProviderTarget(got, "a", false, true)
	if !reflect.DeepEqual(got, []string{"a"}) {
		t.Fatalf("single-host selection = %v", got)
	}
	got = toggleProviderTarget([]string{"a"}, "a", true, false)
	if len(got) != 0 {
		t.Fatalf("removed host remains selected: %v", got)
	}
	result := completeProviderSaveResult([]string{"node-a", "node-b"}, ProviderSaveResult{Succeeded: []string{"node-a"}})
	if !reflect.DeepEqual(result.Succeeded, []string{"node-a"}) || !reflect.DeepEqual(result.Failed, []string{"node-b"}) {
		t.Fatalf("an unreported host was treated as success: %+v", result)
	}
}

func TestProviderIdentityIsNodeScopedAndGroupTargetsAreExplicit(t *testing.T) {
	m := NewModel(preview.SettingsData())
	m.Channels = []preview.Channel{
		{Node: "node-a", ID: "same", Name: "A"},
		{Node: "node-b", ID: "same", Name: "B"},
	}
	if m.channel(channelKey("node-a", "same")).Name != "A" || m.channel(channelKey("node-b", "same")).Name != "B" {
		t.Fatal("same provider IDs from separate Nodes resolved to the same channel")
	}
	if m.channel("same") != nil {
		t.Fatal("unqualified lookup resolved an ambiguous provider ID across Nodes")
	}

	grouped := preview.Channel{Node: "node-a", Group: "group-a", ID: "same", Auth: preview.AuthKey,
		TargetIDs: map[string]string{"node-a": "same", "node-b": "replica"}}
	e := &m.provider
	e.edit(m, &grouped)
	if !reflect.DeepEqual(e.targets, []string{"node-a", "node-b"}) || e.targetIDs["node-b"] != "replica" {
		t.Fatalf("explicit group mapping was not restored: targets=%v ids=%v", e.targets, e.targetIDs)
	}
	legacy := preview.Channel{Node: "node-a", ID: "same", Auth: preview.AuthKey, Targets: []string{"node-a", "node-b"}}
	e.edit(m, &legacy)
	if !reflect.DeepEqual(e.targets, []string{"node-a"}) || len(e.targetIDs) != 0 {
		t.Fatalf("legacy channels inferred a group without persisted identity: targets=%v ids=%v", e.targets, e.targetIDs)
	}
}

func TestProviderSaveCarriesNodeTargetsAndSignsInOnMovedOAuthTarget(t *testing.T) {
	newEditor := func(m *Model, auth preview.Auth) *providerEditor {
		e := &m.provider
		e.open = true
		e.generation = 1
		e.id = "fixture-provider"
		e.owner = "node-a"
		e.targets = []string{"node-a", "node-b"}
		e.targetRevisions = map[string]uint64{"node-a": 2, "node-b": 3}
		e.name = "Fixture"
		e.method = preview.Method{ID: "fixture", API: "responses", Auth: auth}
		e.models = []modelDraft{{id: "fixture-model", context: "4096", model: preview.ChannelModel{Output: 1024}}}
		return e
	}

	t.Run("staticChannel", func(t *testing.T) {
		m := NewModel(preview.SettingsData())
		m.Live = true
		m.ProviderActions.Hosts = func() []ProviderHost { return []ProviderHost{{Node: "node-a"}, {Node: "node-b"}} }
		var saved ProviderDraft
		m.ProviderActions.Save = func(draft ProviderDraft, done func(ProviderSaveResult)) {
			saved = draft
			done(ProviderSaveResult{Failed: draft.Targets})
		}
		e := newEditor(m, preview.AuthKey)
		e.keySet = true
		if problem := e.save(m); problem != "" {
			t.Fatal(problem)
		}
		if saved.Channel.Node != "node-a" || !reflect.DeepEqual(saved.Targets, []string{"node-a", "node-b"}) ||
			saved.Key != "" || saved.Channel.ID != "fixture-provider" || saved.TargetRevisions["node-a"] != 2 || saved.TargetRevisions["node-b"] != 3 {
			t.Fatalf("save did not preserve node-scoped channel identity and targets: %+v", saved)
		}
	})

	t.Run("oauthMove", func(t *testing.T) {
		m := NewModel(preview.SettingsData())
		m.Live = true
		m.ProviderActions.Hosts = func() []ProviderHost { return []ProviderHost{{Node: "node-a"}, {Node: "node-b"}} }
		var saved ProviderDraft
		var signedNode, signedID string
		m.ProviderActions.Save = func(draft ProviderDraft, done func(ProviderSaveResult)) {
			saved = draft
			done(ProviderSaveResult{Succeeded: draft.Targets})
		}
		m.ProviderActions.SignIn = func(node, id string) { signedNode, signedID = node, id }
		e := newEditor(m, preview.AuthOAuth)
		e.method.Login = "provider_access_login"
		e.targets = []string{"node-b"}
		if problem := e.save(m); problem != "" {
			t.Fatal(problem)
		}
		if !reflect.DeepEqual(saved.Targets, []string{"node-b"}) || signedNode != "node-b" || signedID != "fixture-provider" {
			t.Fatalf("OAuth target move did not start sign-in on the destination: saved=%+v sign-in=%s/%s", saved, signedNode, signedID)
		}
	})

	t.Run("oauthMoveStartsLoginWhenOldHostIsUnavailable", func(t *testing.T) {
		m := NewModel(preview.SettingsData())
		m.Live = true
		m.ProviderActions.Hosts = func() []ProviderHost { return []ProviderHost{{Node: "node-a"}, {Node: "node-b"}} }
		var signedNode, signedID string
		m.ProviderActions.Save = func(draft ProviderDraft, done func(ProviderSaveResult)) {
			done(ProviderSaveResult{
				Succeeded: []string{"node-b"}, Failed: []string{"node-a"},
				TargetIDs: map[string]string{"node-b": "replica"},
			})
		}
		m.ProviderActions.SignIn = func(node, id string) { signedNode, signedID = node, id }
		e := newEditor(m, preview.AuthOAuth)
		e.method.Login = "provider_access_login"
		e.targets = []string{"node-b"}
		if problem := e.save(m); problem != "" {
			t.Fatal(problem)
		}
		if signedNode != "node-b" || signedID != "replica" || !e.open || !reflect.DeepEqual(e.targetFailed, []string{"node-a"}) {
			t.Fatalf("destination login did not proceed with the old host failure visible: sign-in=%s/%s editor=%+v", signedNode, signedID, e)
		}
	})

	t.Run("partialResultsKeepEditorOpenAndRecordIdentity", func(t *testing.T) {
		m := NewModel(preview.SettingsData())
		m.Live = true
		m.ProviderActions.Hosts = func() []ProviderHost { return []ProviderHost{{Node: "node-a"}, {Node: "node-b"}} }
		m.ProviderActions.Save = func(draft ProviderDraft, done func(ProviderSaveResult)) {
			done(ProviderSaveResult{Succeeded: []string{"node-a"}, Failed: []string{"node-b"}, Group: "group-a", TargetIDs: map[string]string{"node-a": "fixture-provider", "node-b": "replica"}})
		}
		e := newEditor(m, preview.AuthKey)
		e.keySet = true
		if problem := e.save(m); problem != "" {
			t.Fatal(problem)
		}
		if !e.open || e.group != "group-a" || e.targetIDs["node-b"] != "replica" ||
			!reflect.DeepEqual(e.targetSucceeded, []string{"node-a"}) || !reflect.DeepEqual(e.targetFailed, []string{"node-b"}) {
			t.Fatalf("partial save result was not retained: %+v", e)
		}
	})
}

func TestProviderPresetSourceDoesNotReplaceChosenOwner(t *testing.T) {
	const selectedOwner = "target-node"
	m := NewModel(preview.SettingsData())
	m.Live = true
	m.ProviderActions.DefaultTarget = func() string { return selectedOwner }
	m.ProviderActions.Hosts = func() []ProviderHost {
		return []ProviderHost{{Node: "preset-node", Connected: true}, {Node: selectedOwner, Connected: true}}
	}
	e := &m.provider
	e.add(m)
	e.configure(preview.Method{Node: "preset-node", ID: "preset-node/builtin:fixture/0", API: "responses", Auth: preview.AuthNone,
		Access: "provider_access_endpoint", Routes: []preview.Route{{ID: "preset-node/builtin:fixture/0", API: "responses", Endpoint: "https://fixture.test/v1"}}})
	if e.owner != selectedOwner || !reflect.DeepEqual(e.targets, []string{selectedOwner}) || e.auth() != preview.AuthNone {
		t.Fatalf("preset source replaced the chosen owner or authentication: owner=%q targets=%v auth=%d", e.owner, e.targets, e.auth())
	}
}

func TestProviderEditorCustomHeaders(t *testing.T) {
	m := NewModel(preview.SettingsData())
	m.Live = true
	m.Brands = []preview.Brand{{ID: "fixture", Methods: []preview.Method{{ID: "compatible", Access: "provider_access_endpoint", Keyed: true,
		Routes: []preview.Route{{ID: "compatible/0", API: "anthropic", Custom: true}},
	}}}}
	m.Channels = []preview.Channel{{ID: "saved", Revision: 3, Name: "Saved", Brand: "fixture", Method: "compatible/0", API: "anthropic",
		Endpoint: "https://gateway.example", Auth: preview.AuthKey, KeySet: true, Headers: map[string]string{"x-b": "2", "User-Agent": "claude-cli/9"},
		Models: []preview.ChannelModel{{ID: "model", Name: "model", Context: 128000, DefaultEffort: "default"}}}}
	var saved []ProviderDraft
	m.ProviderActions.Save = func(draft ProviderDraft, done func(ProviderSaveResult)) {
		saved = append(saved, draft)
		done(ProviderSaveResult{Failed: []string{"fixture-node"}})
	}
	e := &m.provider
	e.edit(m, &m.Channels[0])
	if !reflect.DeepEqual(e.headers, []header{{"User-Agent", "claude-cli/9"}, {"x-b", "2"}}) {
		t.Fatalf("headers draft = %q", e.headers)
	}
	tt := ui.NewTesterWith(func(c *ui.Context) {
		theme.Apply(c, "", theme.Dark)
		providerEditorDialog(c, m)
	}, 1100, 900, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true, ReduceMotion: true}})
	tt.SetLocale("en")
	tt.Frame()
	advanced, ok := tt.AccessByRole(ui.RoleButton, i18n.In("en", "provider_oauth_advanced"))
	if !ok {
		t.Fatal("an API-key channel has no advanced settings")
	}
	tt.ClickAt(advanced.Bounds.X+advanced.Bounds.W/2, advanced.Bounds.Y+advanced.Bounds.H/2)
	tt.Advance(time.Second)
	tt.Frame()
	if _, ok := tt.AccessByRole(ui.RoleButton, i18n.In("en", "mcp_add_header")); !ok {
		t.Fatal("advanced settings do not show the request headers")
	}
	// The saved headers are a row each, and Add header adds an empty one.
	if _, ok := tt.AccessByRole(ui.RoleTextField, i18n.In("en", "provider_headers")+" "+i18n.In("en", "mcp_slot_name")); !ok {
		t.Fatal("the saved headers show no rows")
	}
	add, _ := tt.AccessByRole(ui.RoleButton, i18n.In("en", "mcp_add_header"))
	tt.ClickAt(add.Bounds.X+add.Bounds.W/2, add.Bounds.Y+add.Bounds.H/2)
	tt.Frame()
	if len(e.headers) != 3 || e.headers[2] != (header{}) {
		t.Fatalf("Add header made %q", e.headers)
	}
	e.headers = []header{{}, {" X-Trace ", "  on "}, {"user-agent", "claude-cli/10"}}
	if problem := e.save(m); problem != "" || !reflect.DeepEqual(saved[len(saved)-1].Channel.Headers, map[string]string{"X-Trace": "on", "user-agent": "claude-cli/10"}) {
		t.Fatalf("saved headers = %v (%s)", saved[len(saved)-1].Channel.Headers, problem)
	}
	e.headers = nil
	if problem := e.save(m); problem != "" || saved[len(saved)-1].Channel.Headers != nil {
		t.Fatal("clearing the headers did not save an empty set")
	}
	e.headers = []header{{" ", "value"}}
	if problem := e.save(m); problem != "provider_headers_invalid" {
		t.Errorf("a value without a name was accepted: %q", problem)
	}
	// A channel without headers opens its advanced settings on one empty
	// row, which saving skips.
	e.headers, e.advanced = nil, false
	tt.Frame()
	advanced, _ = tt.AccessByRole(ui.RoleButton, i18n.In("en", "provider_oauth_advanced"))
	tt.ClickAt(advanced.Bounds.X+advanced.Bounds.W/2, advanced.Bounds.Y+advanced.Bounds.H/2)
	tt.Frame()
	if !reflect.DeepEqual(e.headers, []header{{}}) {
		t.Fatalf("opened with rows %q, want one empty row", e.headers)
	}
	if problem := e.save(m); problem != "" || saved[len(saved)-1].Channel.Headers != nil {
		t.Fatalf("the empty row saved %v (%s)", saved[len(saved)-1].Channel.Headers, problem)
	}
}
