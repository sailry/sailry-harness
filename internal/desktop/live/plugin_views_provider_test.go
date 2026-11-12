package live

import (
	"encoding/json"
	"reflect"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	desktopplugins "github.com/sailry/sailry-harness/internal/desktop/plugins"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestProviderPluginSlot(t *testing.T) {
	node := protocol.NodeID{1}
	entry := "ui.js"
	var declarations []protocol.PluginUiContribution
	if err := json.Unmarshal([]byte(`[
		{"id":"usage","slot":"footer","kind":"metric","label":{"label":"Remaining usage"}},
		{"id":"provider_usage","slot":"provider","kind":"metric","label":{"label":"Remaining usage"}}
	]`), &declarations); err != nil {
		t.Fatal(err)
	}
	// status declares the sign-in it reads, as provider-status does; owner
	// brings its own preset, as workbuddy does.
	info := func(name string, resources ...protocol.PluginModelAuthResource) protocol.PluginInfo {
		return protocol.PluginInfo{
			Summary: protocol.PluginSummary{Name: name, Enabled: true, Compatibility: protocol.PluginCompatibility{Kind: "compatible"}},
			Extension: &protocol.PluginExtension{
				Scope: protocol.PluginScopeHost, Desktop: &protocol.PluginDesktopManifest{UiEntry: &entry},
				Ui: declarations, ModelResources: resources,
			},
		}
	}
	status := info("status", protocol.PluginModelAuthResource{Authentication: protocol.AuthenticationChatGptOAuth})
	owner := info("owner")

	credentials := map[protocol.CredentialID]protocol.Credential{}
	channel := func(authentication protocol.Authentication, revision uint64) protocol.ConversationProvider {
		provider := protocol.ConversationProvider{
			ID: protocol.NewProviderID(), Revision: revision, Name: "Account", Authentication: authentication,
			Endpoint: "https://private.example.test", Enabled: true, DefaultModel: "selected",
			Credential: &protocol.CredentialRef{Node: node, ID: protocol.NewCredentialID()},
		}
		credentials[provider.Credential.ID] = protocol.Credential{
			ID: provider.Credential.ID, Provider: provider.ID, Authentication: authentication, Revision: 7,
		}
		return provider
	}
	account := channel(protocol.AuthenticationChatGptOAuth, 3)
	key := channel(protocol.AuthenticationApiKey, 1)
	disabled := channel(protocol.AuthenticationChatGptOAuth, 1)
	disabled.Enabled = false
	revoked := channel(protocol.AuthenticationChatGptOAuth, 1)
	signedOut := credentials[revoked.Credential.ID]
	signedOut.Revoked = true
	credentials[revoked.Credential.ID] = signedOut
	unsigned := channel(protocol.AuthenticationChatGptOAuth, 1)
	unsigned.Credential = nil
	preset := channel(protocol.AuthenticationPlugin, 5)
	preset.Preset = &protocol.ConversationPresetsPresetRef{
		Source: protocol.ConversationPresetsSource{Kind: "package", Data: protocol.ConversationPresetsSourcePackagePayload{Name: "owner"}}, ID: "owner",
	}
	providers := []protocol.ConversationProvider{account, key, disabled, revoked, unsigned, preset}

	t.Run("a provider view mounts the provider controls alone", func(t *testing.T) {
		shown := pluginViewDeclarations("status", declarations, "provider")
		if len(shown) != 1 || shown[0].ID != "provider_usage" {
			t.Fatalf("provider controls = %+v", shown)
		}
		if shown := pluginViewDeclarations("status", declarations, "composer"); len(shown) != 1 || shown[0].ID != "usage" {
			t.Fatalf("composer controls = %+v", shown)
		}
		if shown := pluginViewDeclarations("status", declarations[:1], "provider"); len(shown) != 0 {
			t.Fatalf("a package without a provider control mounts %+v", shown)
		}
	})

	t.Run("a package is told its own channels and their scopes", func(t *testing.T) {
		value, scopes := providerPluginContexts(true, providers, credentials, status)
		want := protocol.PluginUiProviderScope{Provider: account.ID, Revision: "3:7", Credential: *account.Credential}
		if !reflect.DeepEqual(scopes, []protocol.PluginUiProviderScope{want}) || !want.Valid() {
			t.Fatalf("declared sign-in scopes = %+v", scopes)
		}
		body, err := json.Marshal(value)
		if err != nil {
			t.Fatal(err)
		}
		var read struct {
			Connected bool `json:"connected"`
			Providers []struct {
				ID             protocol.ProviderID            `json:"id"`
				Revision       string                         `json:"revision"`
				Model          string                         `json:"model"`
				Authentication protocol.Authentication        `json:"authentication"`
				Scope          protocol.PluginUiProviderScope `json:"scope"`
			} `json:"providers"`
		}
		if err := json.Unmarshal(body, &read); err != nil {
			t.Fatal(err)
		}
		if !read.Connected || len(read.Providers) != 1 || read.Providers[0].ID != account.ID || read.Providers[0].Revision != "3" ||
			read.Providers[0].Model != "selected" || read.Providers[0].Authentication != protocol.AuthenticationChatGptOAuth || read.Providers[0].Scope != want {
			t.Fatalf("provider contexts = %s", body)
		}
		if strings.Contains(string(body), "private.example") {
			t.Fatalf("provider contexts carry the endpoint: %s", body)
		}

		_, scopes = providerPluginContexts(true, providers, credentials, owner)
		if len(scopes) != 1 || scopes[0].Provider != preset.ID || scopes[0].Revision != "5:7" {
			t.Fatalf("own preset scopes = %+v", scopes)
		}

		// A package that declares neither the sign-in nor the preset, and a
		// Node that is away, are told of no channel.
		value, scopes = providerPluginContexts(true, providers, credentials, info("other"))
		if body, _ := json.Marshal(value["providers"]); string(body) != "[]" || len(scopes) != 0 {
			t.Fatalf("another package is told of %s", body)
		}
		value, scopes = providerPluginContexts(false, providers, credentials, status)
		if body, _ := json.Marshal(value); string(body) != `{"connected":false,"providers":[]}` || len(scopes) != 0 {
			t.Fatalf("disconnected contexts = %s", body)
		}

		// A renewed credential is a new scope: the old account's figure goes.
		renewed := credentials[account.Credential.ID]
		renewed.Revision++
		scope, scoped := providerPluginScope(account, map[protocol.CredentialID]protocol.Credential{renewed.ID: renewed})
		if !scoped || scope.Revision != "3:8" || scope == want {
			t.Fatalf("renewed scope = %+v", scope)
		}
	})

	t.Run("a figure shows on the row of the channel it was published for", func(t *testing.T) {
		value, scopes := providerPluginContexts(true, providers, credentials, status)
		registry := desktopplugins.NewRegistry(desktopplugins.RegistryOptions{
			Node: node, Context: protocol.PluginContext{Surface: protocol.PluginDesktopSurface{Kind: "provider"}},
		})
		t.Cleanup(func() { _ = registry.Close() })
		reference := protocol.PluginReference{Name: "status", Digest: strings.Repeat("a", 64)}
		hooks, err := registry.Hooks(reference, pluginViewDeclarations("status", declarations, "provider"))
		if err != nil {
			t.Fatal(err)
		}
		if err := registry.SetProviderScopes(reference, scopes); err != nil {
			t.Fatal(err)
		}
		if err := registry.SetActivity(reference, true, false); err != nil {
			t.Fatal(err)
		}
		// The view publishes under the scope it read, as the packages do.
		contexts, err := json.Marshal(value["providers"])
		if err != nil {
			t.Fatal(err)
		}
		var read []struct {
			Scope json.RawMessage `json:"scope"`
		}
		if err := json.Unmarshal(contexts, &read); err != nil || len(read) != 1 {
			t.Fatalf("provider contexts = %s (%v)", contexts, err)
		}
		publish := func(scope json.RawMessage, visible bool) error {
			state, err := json.Marshal([]map[string]any{{
				"id": "provider_usage", "scope": scope, "visible": visible, "value": "64%",
				"details": []map[string]any{{"label": map[string]any{"label": "5 hours"}, "value": "64%", "progress": 64}},
			}})
			if err != nil {
				t.Fatal(err)
			}
			return hooks.Publish(state)
		}
		if err := publish(read[0].Scope, true); err != nil {
			t.Fatal(err)
		}
		entries := registry.ProviderEntries(scopes[0])
		if len(entries) != 1 || entries[0].Key.ID != "provider_usage" || len(entries[0].State.Details) != 1 {
			t.Fatalf("the channel's entries = %+v", entries)
		}
		// Another channel, and a channel the package was not told of, show
		// nothing.
		other, _ := providerPluginScope(preset, credentials)
		if entries := registry.ProviderEntries(other); len(entries) != 0 {
			t.Fatalf("another channel shows %+v", entries)
		}
		foreign, err := json.Marshal(other)
		if err != nil {
			t.Fatal(err)
		}
		if publish(foreign, true) == nil {
			t.Fatal("a figure was accepted for a channel the package was not told of")
		}
		// A figure the package hides, as before its first answer, shows nothing.
		if err := publish(read[0].Scope, false); err != nil {
			t.Fatal(err)
		}
		if entries := registry.ProviderEntries(scopes[0]); len(entries) != 0 {
			t.Fatalf("a hidden figure shows %+v", entries)
		}
	})

	t.Run("a row shows two meters or one value", func(t *testing.T) {
		meter := func(label string, progress float64) conversation.Row {
			return conversation.Row{Label: label, Value: label + " left", Meter: true, Progress: progress}
		}
		for name, test := range map[string]struct {
			figure conversation.Metric
			want   []settings.UsageLine
		}{
			"the first two meters": {
				conversation.Metric{Value: "12%", Rows: []conversation.Row{
					meter(" 5 hours ", 64), {Label: "Resets", Value: "2026-11-12 18:00"}, meter("Week", 12), meter("Opus", 90),
				}},
				[]settings.UsageLine{
					{Label: "5 hours", Value: " 5 hours  left", Meter: true, Progress: 64},
					{Label: "Week", Value: "Week left", Meter: true, Progress: 12},
				},
			},
			"the first row without a meter": {
				conversation.Metric{Value: "unlimited", Rows: []conversation.Row{{Label: "Credits", Value: "41.5"}, {Label: "Extra", Value: "0 / 50"}}},
				[]settings.UsageLine{{Value: "41.5"}},
			},
			"the figure's own value without rows": {
				conversation.Metric{Value: "1,280"},
				[]settings.UsageLine{{Value: "1,280"}},
			},
		} {
			if lines := providerUsageLines(test.figure); !reflect.DeepEqual(lines, test.want) {
				t.Fatalf("%s: lines = %+v", name, lines)
			}
		}
	})

	t.Run("the slot is mounted for the Node Settings holds, outside Demo mode", func(t *testing.T) {
		shown, other := &Runtime{}, &Runtime{}
		if !providerSlotShown(shown, shown, false) {
			t.Fatal("the Node Settings holds mounts no provider slot")
		}
		if providerSlotShown(shown, shown, true) {
			t.Fatal("Demo mode keeps the provider slot mounted")
		}
		if providerSlotShown(shown, other, false) || providerSlotShown(nil, nil, false) {
			t.Fatal("a Node Settings does not hold keeps the provider slot mounted")
		}
	})
}
