package live

import (
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/preferences"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// testProviderLoginWarning signs in to a package provider whose sign-in
// declares a warning: the dialog shows before any login command is sent,
// Cancel sends nothing and Continue starts the flow. A provider without a
// warning starts at once.
func testProviderLoginWarning(t *testing.T) {
	source := protocol.ConversationPresetsSource{Kind: "package", Data: protocol.ConversationPresetsSourcePackagePayload{Name: "fixture"}}
	saved := func(n byte, name, preset string) protocol.ConversationProvider {
		p := protocol.DefaultConversationProvider()
		p.ID, p.Name, p.Api, p.Enabled, p.Revision, p.Authentication = protocol.ProviderID{n}, name, protocol.ConversationModelApiPlugin, true, 1, protocol.AuthenticationPlugin
		p.Preset = &protocol.ConversationPresetsPresetRef{Source: source, ID: preset}
		return p
	}
	entry := func(id string, warning *protocol.PluginModelAuthWarning) protocol.ConversationPresetsEntry {
		value := protocol.DefaultConversationPresetsEntry()
		value.Source, value.Provenance = source, protocol.ConversationPresetsProvenanceThirdParty
		value.Preset.ID, value.Preset.Name = id, id
		// A preset a provider can be saved from has a route; the directory
		// is kept by it.
		value.Preset.Routes = []protocol.ConversationPresetsRoute{{Protocol: protocol.ConversationPresetsProtocolPlugin}}
		value.Login = &protocol.PluginModelAuthPresentation{Kind: "polling"}
		value.LoginWarning = warning
		return value
	}
	listing := protocol.DefaultConversationPresetsListing()
	listing.Entries = []protocol.ConversationPresetsEntry{
		entry("risky", &protocol.PluginModelAuthWarning{Text: "The service may suspend this account", Locales: map[string]string{"zh-CN": "该服务可能封禁此账号"}}),
		entry("plain", nil),
	}
	snapshot := protocol.DefaultSnapshot()
	snapshot.Node = protocol.NodeID{1}
	snapshot.Providers = []protocol.ConversationProvider{saved(1, "Risky account", "risky"), saved(2, "Plain account", "plain")}
	transport := &conversationTestTransport{snapshot: snapshot}
	transport.handle = func(request protocol.Request) (link.Response, bool) {
		switch request.Command.Kind {
		case "list_channel_presets":
			return link.Response{Output: protocol.Output{Kind: "channel_presets", Data: listing}}, true
		case "begin_provider_login":
			return link.Response{Fault: &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "fixture has no sign-in service"}}, true
		}
		return link.Response{}, false
	}
	begun := func() (providers []protocol.ProviderID) {
		transport.mu.Lock()
		defer transport.mu.Unlock()
		for _, request := range transport.requests {
			if request.Command.Kind == "begin_provider_login" {
				input, _ := detail[protocol.CommandBeginProviderLoginPayload](request.Command.Data)
				providers = append(providers, input.Provider)
			}
		}
		return
	}
	controller := client.New(transport)
	if err := controller.Start(t.Context()); err != nil {
		t.Fatal(err)
	}
	defer controller.Close()
	r, err := NewWithPreferences(t.Context(), controller, preferences.Open(t.TempDir()+"/preferences.json"))
	if err != nil {
		t.Fatal(err)
	}
	defer r.Close()
	m := r.App.Settings
	m.Open(settings.Providers)
	tester := ui.NewTesterWith(func(c *ui.Context) {
		r.mu.Lock()
		queue := r.queue
		r.queue = nil
		r.mu.Unlock()
		for _, post := range queue {
			post(c)
		}
		settings.Page(c, m, r.App.Data, settings.Look{Theme: &r.App.Theme, Mode: &r.App.Mode, Effects: &r.App.Effects, Surfaces: &r.App.Surfaces})
		ui.ToastViewportBase(c, func(ui.Element, []ui.Toast) {})
	}, 1100, 900, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true}})
	tester.SetLocale("en")
	en := func(key string) string { return i18n.In("en", key) }
	await := func(what string, done func() bool) {
		t.Helper()
		deadline := time.Now().Add(10 * time.Second)
		for tester.Frame(); !done(); tester.Frame() {
			if time.Now().After(deadline) {
				t.Fatalf("timed out waiting for %s: %q", what, tester.Texts())
			}
			time.Sleep(2 * time.Millisecond)
		}
	}
	click := func(name string) {
		t.Helper()
		x, ok := tester.AccessByRole(ui.RoleButton, name)
		if !ok {
			t.Fatalf("no button %q among %q", name, tester.Texts())
		}
		tester.ClickAt(x.Bounds.X+x.Bounds.W/2, x.Bounds.Y+x.Bounds.H/2)
		tester.Advance(time.Second)
		tester.Frame()
	}
	const warning = "The service may suspend this account"
	risky, plain := protocol.ProviderID{1}, protocol.ProviderID{2}
	await("the preset directory", func() bool { return len(r.providerPresets) == 2 && len(m.Channels) == 2 })

	m.ProviderActions.SignIn(risky.String())
	await("the warning", func() bool { return tester.HasText(warning) })
	if !tester.HasText(en("provider_reconnect")+" Risky account") || len(begun()) != 0 || m.ProviderAuthorization().Open {
		t.Fatalf("the warning did not precede the sign-in: %d commands, %q", len(begun()), tester.Texts())
	}
	click(en("settings_cancel"))
	await("the warning to close", func() bool { return !tester.HasText(warning) })
	if len(begun()) != 0 || m.ProviderAuthorization().Open {
		t.Fatalf("Cancel sent %d login commands", len(begun()))
	}

	// Each attempt asks again.
	m.ProviderActions.SignIn(risky.String())
	await("the warning of the second attempt", func() bool { return tester.HasText(warning) })
	click(en("provider_login_continue"))
	await("the login command", func() bool { return len(begun()) == 1 })
	if sent := begun(); sent[0] != risky || tester.HasText(warning) {
		t.Fatalf("Continue began %v with the warning still shown: %q", sent, tester.Texts())
	}

	m.ProviderActions.SignIn(plain.String())
	await("the sign-in without a warning", func() bool { return len(begun()) == 2 })
	if sent := begun(); sent[1] != plain || tester.HasText(warning) {
		t.Fatalf("a sign-in without a warning began %v: %q", sent, tester.Texts())
	}
}

// testProviderPresetWarning adds a channel from a package preset that has no
// sign-in and declares a warning: picking it shows the dialog before the
// form, Cancel returns to the brands with nothing made or sent, Continue
// opens the form as ever, and each pick asks again. A preset without a
// warning opens its form at once.
func testProviderPresetWarning(t *testing.T) {
	source := protocol.ConversationPresetsSource{Kind: "package", Data: protocol.ConversationPresetsSourcePackagePayload{Name: "fixture"}}
	entry := func(id, name string, warning *protocol.PluginModelAuthWarning) protocol.ConversationPresetsEntry {
		value := protocol.DefaultConversationPresetsEntry()
		value.Source, value.Provenance = source, protocol.ConversationPresetsProvenanceThirdParty
		value.Package = &protocol.PluginReference{Name: "fixture"}
		value.Preset.ID, value.Preset.Name, value.Preset.Offered = id, name, true
		value.Preset.Routes = []protocol.ConversationPresetsRoute{{Protocol: protocol.ConversationPresetsProtocolPlugin, Endpoint: "https://example.test"}}
		value.Preset.Auth = protocol.ConversationPresetsAuth{Kind: "none"}
		value.LoginWarning = warning
		return value
	}
	const warning = "Heavy use may count against this account"
	listing := protocol.DefaultConversationPresetsListing()
	listing.Entries = []protocol.ConversationPresetsEntry{
		entry("local", "Local agent", &protocol.PluginModelAuthWarning{Text: warning, Locales: map[string]string{"zh-CN": "大量使用可能计入此账号的额度"}}),
		entry("quiet", "Quiet agent", nil),
	}
	snapshot := protocol.DefaultSnapshot()
	snapshot.Node = protocol.NodeID{1}
	transport := &conversationTestTransport{snapshot: snapshot}
	transport.handle = func(request protocol.Request) (link.Response, bool) {
		if request.Command.Kind == "list_channel_presets" {
			return link.Response{Output: protocol.Output{Kind: "channel_presets", Data: listing}}, true
		}
		return link.Response{}, false
	}
	// written counts the commands that make, change or probe a channel.
	written := func() (n int) {
		transport.mu.Lock()
		defer transport.mu.Unlock()
		for _, request := range transport.requests {
			switch request.Command.Kind {
			case "save_provider", "put_provider", "discover_models", "begin_provider_login":
				n++
			}
		}
		return
	}
	controller := client.New(transport)
	if err := controller.Start(t.Context()); err != nil {
		t.Fatal(err)
	}
	defer controller.Close()
	r, err := NewWithPreferences(t.Context(), controller, preferences.Open(t.TempDir()+"/preferences.json"))
	if err != nil {
		t.Fatal(err)
	}
	defer r.Close()
	m := r.App.Settings
	m.Open(settings.Providers)
	tester := ui.NewTesterWith(func(c *ui.Context) {
		r.mu.Lock()
		queue := r.queue
		r.queue = nil
		r.mu.Unlock()
		for _, post := range queue {
			post(c)
		}
		settings.Page(c, m, r.App.Data, settings.Look{Theme: &r.App.Theme, Mode: &r.App.Mode, Effects: &r.App.Effects, Surfaces: &r.App.Surfaces})
		ui.ToastViewportBase(c, func(ui.Element, []ui.Toast) {})
	}, 1100, 900, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true}})
	tester.SetLocale("en")
	en := func(key string) string { return i18n.In("en", key) }
	await := func(what string, done func() bool) {
		t.Helper()
		deadline := time.Now().Add(10 * time.Second)
		for tester.Frame(); !done(); tester.Frame() {
			if time.Now().After(deadline) {
				t.Fatalf("timed out waiting for %s: %q", what, tester.Texts())
			}
			time.Sleep(2 * time.Millisecond)
		}
	}
	click := func(name string) {
		t.Helper()
		x, ok := tester.AccessByRole(ui.RoleButton, name)
		if !ok {
			t.Fatalf("no button %q among %q", name, tester.Texts())
		}
		tester.ClickAt(x.Bounds.X+x.Bounds.W/2, x.Bounds.Y+x.Bounds.H/2)
		tester.Advance(time.Second)
		tester.Frame()
	}
	await("the preset directory", func() bool { return len(m.Brands) == 2 })
	for _, b := range m.Brands {
		if want := map[string]string{"Local agent": warning, "Quiet agent": ""}[b.Name]; len(b.Methods) != 1 || b.Methods[0].Warning != want {
			t.Fatalf("brand %q projects %+v", b.Name, b.Methods)
		}
	}

	click(en("settings_add"))
	await("the brand step", func() bool { return tester.HasText(en("provider_add_title")) })
	click("Local agent")
	await("the warning", func() bool { return tester.HasText(warning) })
	if !tester.HasText(en("settings_add")+" Local agent") || tester.HasText(en("provider_configure")) || written() != 0 {
		t.Fatalf("the warning did not precede the form: %d commands, %q", written(), tester.Texts())
	}
	click(en("settings_cancel"))
	await("the warning to close", func() bool { return !tester.HasText(warning) })
	if !tester.HasText(en("provider_add_title")) || tester.HasText(en("provider_configure")) || written() != 0 || len(m.Channels) != 0 {
		t.Fatalf("Cancel left %d commands, %d channels: %q", written(), len(m.Channels), tester.Texts())
	}

	// Each pick asks again; Continue opens the form the pick always opened.
	click("Local agent")
	await("the warning of the second pick", func() bool { return tester.HasText(warning) })
	click(en("provider_login_continue"))
	await("the form", func() bool { return tester.HasText(en("provider_configure")) })
	if tester.HasText(warning) || len(m.Channels) != 0 {
		t.Fatalf("Continue left the warning or made a channel: %q", tester.Texts())
	}

	click(en("provider_step_back"))
	await("the brand step again", func() bool { return tester.HasText(en("provider_add_title")) })
	click("Quiet agent")
	await("the form without a warning", func() bool { return tester.HasText(en("provider_configure")) })
	if tester.HasText(warning) {
		t.Fatalf("a preset without a warning asked: %q", tester.Texts())
	}
}
