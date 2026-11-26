package desktop

import (
	"slices"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
)

// The provider editor's steps and checks.
func TestProviderEditor(t *testing.T) {
	t.Run("keyless methods have their own group", func(t *testing.T) {
		m := testController()
		m.Brands = []preview.Brand{{ID: "cli", Name: "CLI", Methods: []preview.Method{{ID: "local", Auth: preview.AuthNone}}}}
		tester := ui.NewTester(func(c *ui.Context) {
			i18n.Bind(c, testCatalog)
			brandPicker(c, m, "", nil, nil)
		}, 900, 900)
		tester.SetLocale("en")
		tester.Frame()
		labels := tester.Texts()
		if !slices.Contains(labels, testCatalog.In("en", "provider_access_no_key")) || slices.Contains(labels, testCatalog.In("en", "provider_access_key")) {
			t.Fatalf("keyless provider offered under wrong heading: %v", labels)
		}
		brand := m.Brands[0]
		brand.Methods = append(brand.Methods, preview.Method{ID: "key", Auth: preview.AuthKey})
		for group, want := range map[string]string{"provider_access_key": "key", "provider_access_no_key": "local"} {
			if got := pickerMethods(&brand, group); len(got) != 1 || got[0].ID != want {
				t.Fatalf("%s offers %v", group, got)
			}
		}
	})
	t.Run("account variants share one brand entry", func(t *testing.T) {
		m := testController()
		m.Brands = []preview.Brand{{ID: "workbuddy", Name: "WorkBuddy", Methods: []preview.Method{
			{ID: "china", Name: "WorkBuddy China", Login: "provider_access_login", Access: "provider_access_login"},
			{ID: "global", Name: "WorkBuddy International", Login: "provider_access_login", Access: "provider_access_login"},
			{ID: "codebuddy", Name: "CodeBuddy International", Login: "provider_access_login", Access: "provider_access_login"},
		}}}
		m.provider.add(m)
		tester := ui.NewTesterWith(func(c *ui.Context) {
			i18n.Bind(c, testCatalog)
			providerEditorDialog(c, m)
		}, 900, 900, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true}})
		tester.SetLocale("en")
		tester.Frame()
		tester.Advance(time.Second)
		if !tester.HasText("WorkBuddy") || tester.HasText("WorkBuddy China") || tester.HasText("CodeBuddy International") {
			t.Fatal("account variants were displayed as separate first-level entries")
		}
		if err := tester.Click("WorkBuddy"); err != nil {
			t.Fatal(err)
		}
		tester.Frame()
		if m.provider.step != stepMethod || !tester.HasText("WorkBuddy China") || !tester.HasText("WorkBuddy International") || !tester.HasText("CodeBuddy International") {
			t.Fatal("grouped account entry did not show its method choices")
		}
	})
	m := testController()
	e := &m.provider
	e.add(m)
	e.choose(&m.Brands[0], pickerMethods(&m.Brands[0], "provider_access_key")) // Anthropic: several API methods
	if e.step != stepMethod {
		t.Fatalf("several methods went to step %d", e.step)
	}
	e.configure(m.Brands[0].Methods[1]) // API key
	if got := e.Check(); got != "provider_key_required" {
		t.Errorf("no key: %q", got)
	}
	e.key = "sk-preview"
	if got := e.Check(); got != "provider_model_required" {
		t.Errorf("no model: %q", got)
	}
	e.models = []modelDraft{{id: "a", context: "128000"}, {id: "a", context: "128000"}}
	if got := e.Check(); got != "provider_model_duplicate" {
		t.Errorf("duplicate: %q", got)
	}
	e.models[1].id = "b"
	n := len(m.Channels)
	if got := e.save(m); got != "" || len(m.Channels) != n+1 {
		t.Fatalf("save: %q, %d channels", got, len(m.Channels))
	}
	e.back()
	if e.step != stepMethod {
		t.Errorf("back went to %d", e.step)
	}
	e.choose(&m.Brands[2], pickerMethods(&m.Brands[2], "provider_access_key")) // Google: one method
	if e.step != stepConfigure {
		t.Errorf("one method went to %d", e.step)
	}
	e.back()
	if e.step != stepBrand {
		t.Errorf("back from a single method went to %d", e.step)
	}
	if ProviderStatus(&m.Channels[2]) != "provider_status_missing_key" || ProviderStatus(&m.Channels[5]) != "provider_status_signed_out" {
		t.Error("statuses")
	}
	account := m.Channels[5]
	account.AccountPending = true
	if status := ProviderStatus(&account); status != "" {
		t.Errorf("pending account status = %q, want no sign-in warning", status)
	}
	// A sign-in opens its form alone: Back returns to the brand step, not to
	// the brand's API methods.
	e.choose(&m.Brands[0], m.Brands[0].Methods[:1])
	if e.step != stepConfigure || !e.login() || e.usesKey() {
		t.Errorf("sign-in went to step %d, login %v, key %v", e.step, e.login(), e.usesKey())
	}
	if e.back(); e.step != stepBrand {
		t.Errorf("back from a sign-in went to %d", e.step)
	}
	// A method without a sign-in whose package declares a warning asks
	// before its form: nothing is configured or added until it is accepted,
	// and a sign-in keeps its warning for the sign-in.
	t.Run("a method's warning precedes its form", func(t *testing.T) {
		m := testController()
		e, n := &m.provider, len(m.Channels)
		brand := &preview.Brand{ID: "agent", Name: "Agent"}
		local := preview.Method{ID: "local", Name: "Local agent", Access: "provider_access_endpoint", Warning: "Heavy use may count against this account"}
		e.add(m)
		m.chooseProvider(brand, []preview.Method{local})
		if w := m.loginWarning; !w.open || w.verb != "settings_add" || w.title != "Local agent" || w.text != local.Warning || e.step != stepBrand || e.method.ID != "" {
			t.Fatalf("pick: warning %+v, step %d, method %q", w, e.step, e.method.ID)
		}
		// Cancel drops the prompt: the editor is where it was.
		m.loginWarning = loginWarning{}
		if e.step != stepBrand || len(m.Channels) != n {
			t.Fatalf("cancel: step %d, %d channels", e.step, len(m.Channels))
		}
		m.chooseProvider(brand, []preview.Method{local})
		m.loginWarning.proceed()
		if e.step != stepConfigure || e.method.ID != "local" || len(m.Channels) != n {
			t.Fatalf("continue: step %d, method %q, %d channels", e.step, e.method.ID, len(m.Channels))
		}
		// An answer for an editor since reopened configures nothing.
		e.add(m)
		m.configureProvider(local)
		stale := m.loginWarning.proceed
		e.add(m)
		if stale(); e.step != stepBrand {
			t.Fatalf("a stale answer went to step %d", e.step)
		}
		m.loginWarning = loginWarning{}
		signIn := preview.Method{ID: "account", Name: "Account", Access: "provider_access_login", Login: "provider_access_login", Warning: local.Warning}
		if m.chooseProvider(brand, []preview.Method{signIn}); m.loginWarning.open || e.step != stepConfigure {
			t.Fatalf("a sign-in asked at the pick: %+v, step %d", m.loginWarning, e.step)
		}
	})
	// A method's routes are one entry: the form switches its protocol,
	// product and region, keeping what was typed and following the route's
	// own defaults.
	t.Run("routes", func(t *testing.T) {
		m := testController()
		hosted := preview.Method{ID: "hosted", Name: "Hosted", Access: "provider_access_key", Routes: []preview.Route{
			{ID: "hosted/0", API: "chat_completions", Endpoint: "https://cn.example/v1", Region: "china", Product: "metered"},
			{ID: "hosted/1", API: "chat_completions", Endpoint: "https://global.example/v1", Region: "global", Product: "metered"},
			{ID: "hosted/2", API: "chat_completions", Endpoint: "https://cn.example/coding", Region: "china", Product: "coding", ProductName: "Hosted Code", Models: []string{"coder"}},
			{ID: "hosted/3", API: "chat_completions", Endpoint: "https://global.example/coding", Region: "global", Product: "coding", ProductName: "Hosted Code", Models: []string{"coder"}},
		}}
		compatible := preview.Method{ID: "compatible", Name: "Compatible", Access: "provider_access_endpoint", Keyed: true, Routes: []preview.Route{
			{ID: "compatible/0", API: "responses", Custom: true}, {ID: "compatible/1", API: "chat_completions", Custom: true},
		}}
		brand := &preview.Brand{ID: "fixture", Name: "Fixture", Methods: []preview.Method{hosted, compatible}}
		e := &m.provider
		e.add(m)
		e.choose(brand, pickerMethods(brand, "provider_access_key"))
		if e.step != stepMethod || len(e.methods) != 2 {
			t.Fatalf("two methods went to step %d with %d rows", e.step, len(e.methods))
		}
		e.configure(compatible)
		if got := e.choices(axisProtocol); !slices.Equal(got, []int{0, 1}) || len(e.choices(axisProduct)) != 1 || len(e.choices(axisRegion)) != 1 {
			t.Fatalf("compatible protocols %v, products %v, regions %v", got, e.choices(axisProduct), e.choices(axisRegion))
		}
		e.name, e.endpoint, e.key = "Mine", "https://mine.example/v1", "sk-typed"
		e.models = []modelDraft{{id: "typed", context: "128000"}}
		e.switchRoute(1)
		if e.current().API != "chat_completions" || e.name != "Mine" || e.endpoint != "https://mine.example/v1" || e.key != "sk-typed" || len(e.models) != 1 || e.models[0].id != "typed" {
			t.Fatalf("switched protocol left route %+v, name %q, endpoint %q, key %q, models %+v", e.current(), e.name, e.endpoint, e.key, e.models)
		}
		if !e.usesKey() || e.Check() != "" {
			t.Errorf("a keyed custom endpoint: key %v, check %q", e.usesKey(), e.Check())
		}
		n := len(m.Channels)
		if got := e.save(m); got != "" || m.Channels[n].Method != "compatible/1" || m.Channels[n].API != "chat_completions" {
			t.Fatalf("save %q kept %+v", got, m.Channels[n])
		}
		// One protocol has no control; products and regions each have two.
		e.configure(hosted)
		if len(e.choices(axisProtocol)) != 1 || !slices.Equal(e.choices(axisProduct), []int{0, 2}) || !slices.Equal(e.choices(axisRegion), []int{0, 1}) {
			t.Fatalf("hosted protocols %v, products %v, regions %v", e.choices(axisProtocol), e.choices(axisProduct), e.choices(axisRegion))
		}
		e.switchRoute(1) // Global
		if e.endpoint != "https://global.example/v1" || len(e.models) != 0 {
			t.Fatalf("region left endpoint %q, models %+v", e.endpoint, e.models)
		}
		e.switchRoute(e.choices(axisProduct)[1]) // the coding product keeps the region
		if e.current().ID != "hosted/3" || e.endpoint != "https://global.example/coding" || len(e.models) != 1 || e.models[0].id != "coder" {
			t.Fatalf("product went to %+v, endpoint %q, models %+v", e.current(), e.endpoint, e.models)
		}
		e.switchRoute(e.choices(axisProduct)[0]) // back: the model the product came with goes
		if e.current().ID != "hosted/1" || len(e.models) != 0 {
			t.Fatalf("product back went to %+v, models %+v", e.current(), e.models)
		}
		// A model the user added keeps every model through a switch.
		e.switchRoute(3)
		e.models = append(e.models, modelDraft{id: "mine", context: "128000"})
		e.switchRoute(1)
		if len(e.models) != 2 {
			t.Fatalf("a switch dropped the user's models: %+v", e.models)
		}
		// An edited channel opens on its route, which stays.
		m.Brands = append(m.Brands, *brand)
		ch := preview.Channel{ID: "saved", Name: "Saved", Brand: "fixture", Method: "hosted/2", API: "chat_completions", KeySet: true}
		e.edit(m, &ch)
		if e.method.ID != "hosted" || e.current().ID != "hosted/2" || e.step != stepConfigure {
			t.Fatalf("edit opened %q on %+v at step %d", e.method.ID, e.current(), e.step)
		}
		if e.switchRoute(0); e.current().ID != "hosted/2" {
			t.Errorf("an edited channel changed its route to %+v", e.current())
		}
	})
	// A sign-in's advanced settings are the ones it has, starting on the
	// values in use; a saved channel shows its own and, for one saved
	// without, the value in use.
	t.Run("oauth advanced settings", func(t *testing.T) {
		m := testController()
		m.Live = true
		var saved []settings.ProviderDraft
		m.ProviderActions.Save = func(d settings.ProviderDraft, _ func(settings.ProviderSaveResult)) { saved = append(saved, d) }
		chatgpt := preview.Method{ID: "chatgpt", Name: "ChatGPT OAuth", Access: "provider_access_login", Login: "provider_login_chat_gpt",
			OAuth: [4]string{"0.160.1", "Sailry/0.1"}, Routes: []preview.Route{{ID: "chatgpt/0", API: "responses", Endpoint: "https://chatgpt.example/codex"}}}
		copilot := preview.Method{ID: "copilot", Name: "Copilot", Access: "provider_access_login", Login: "provider_access_login",
			OAuth: [4]string{1: "GitHubCopilotChat/0.68.0", 2: "vscode/1.140.0", 3: "copilot-chat/0.68.0"}, Routes: []preview.Route{{ID: "copilot/0", API: "chat_completions", Endpoint: "https://copilot.example"}}}
		key := preview.Method{ID: "key", Name: "Key", Access: "provider_access_key", Routes: []preview.Route{{ID: "key/0", API: "responses", Endpoint: "https://key.example/v1"}}}
		m.Brands = append(m.Brands, preview.Brand{ID: "fixture", Name: "Fixture", Methods: []preview.Method{chatgpt, copilot, key}})
		e := &m.provider
		tester := ui.NewTester(func(c *ui.Context) {
			i18n.Bind(c, testCatalog)
			providerEditorDialog(c, m)
		}, 900, 900)
		labels := []string{"provider_oauth_catalog_version", "provider_oauth_user_agent", "provider_oauth_editor_version", "provider_oauth_editor_plugin_version"}
		// shown checks the fields the open form offers against want, where
		// "" is a field that must be absent.
		shown := func(name string, want [4]string) {
			t.Helper()
			e.advanced = true
			tester.Frame()
			for i, label := range labels {
				field, ok := tester.AccessByRole(ui.RoleTextField, testCatalog.In("en", label))
				if ok != (want[i] != "") || ok && field.Value != want[i] {
					t.Errorf("%s: field %s is %q (shown %v), want %q", name, label, field.Value, ok, want[i])
				}
			}
		}
		e.add(m)
		e.configure(chatgpt)
		shown("a new ChatGPT channel", chatgpt.OAuth)
		e.models = []modelDraft{{id: "gpt", context: "128000"}}
		if got := e.save(m); got != "" || len(saved) != 1 || saved[0].OAuth != chatgpt.OAuth {
			t.Fatalf("save %q sent %+v", got, saved)
		}
		e.add(m)
		e.configure(copilot)
		shown("a new Copilot channel", copilot.OAuth)
		// The settings of the method before do not stay under another.
		e.configure(key)
		if e.oauth != ([4]string{}) {
			t.Errorf("an API key method kept settings %q", e.oauth)
		}
		e.advanced = true
		tester.Frame()
		if _, ok := tester.AccessByRole(ui.RoleTextField, testCatalog.In("en", "provider_oauth_user_agent")); ok {
			t.Error("an API key method offers advanced sign-in settings")
		}
		if _, ok := tester.AccessByRole(ui.RoleButton, testCatalog.In("en", "mcp_add_header")); !ok {
			t.Error("an API key method does not offer its request headers")
		}
		// A saved channel: its own user agent, and the catalog version in
		// use where it saved none.
		ch := preview.Channel{ID: "saved", Name: "Saved", Method: "chatgpt/0", API: "responses", Auth: preview.AuthOAuth, OAuth: [4]string{1: "Mine/2.0"}}
		e.edit(m, &ch)
		shown("a saved ChatGPT channel", [4]string{"0.160.1", "Mine/2.0"})
	})
	// An Azure OpenAI route asks for the API version, which a save carries,
	// and names a model by its deployment.
	t.Run("azure api version", func(t *testing.T) {
		m := testController()
		m.Live = true
		var saved []settings.ProviderDraft
		m.ProviderActions.Save = func(d settings.ProviderDraft, _ func(settings.ProviderSaveResult)) { saved = append(saved, d) }
		azure := preview.Method{ID: "azure", Name: "Azure OpenAI", Access: "provider_access_endpoint", Keyed: true, Routes: []preview.Route{{ID: "azure/0", API: "azure_open_ai", Custom: true}}}
		e := &m.provider
		e.add(m)
		e.configure(azure)
		if e.apiVersion != "2024-10-21" {
			t.Fatalf("a new Azure form starts on version %q", e.apiVersion)
		}
		e.endpoint, e.key = "https://unit.openai.azure.com", "key"
		e.models = []modelDraft{{id: "deployment", context: "128000"}}
		e.apiVersion = " "
		if got := e.Check(); got != "provider_cloud_required" {
			t.Errorf("no version: %q", got)
		}
		e.apiVersion = " 2025-01-01-preview "
		if got := e.save(m); got != "" || len(saved) != 1 || saved[0].Channel.APIVersion != "2025-01-01-preview" || saved[0].Channel.API != "azure_open_ai" {
			t.Fatalf("save %q sent %+v", got, saved)
		}
		// A saved channel opens on its version; another API has none.
		ch := preview.Channel{ID: "saved", Name: "Saved", Method: "builtin:azure-openai/0", API: "azure_open_ai", Endpoint: "https://unit.openai.azure.com", APIVersion: "2024-06-01", KeySet: true}
		e.edit(m, &ch)
		if !e.azure() || e.apiVersion != "2024-06-01" {
			t.Errorf("edit opened azure %v on version %q", e.azure(), e.apiVersion)
		}
		tester := ui.NewTester(func(c *ui.Context) {
			i18n.Bind(c, testCatalog)
			providerEditorDialog(c, m)
		}, 900, 900)
		tester.Frame()
		version, ok := tester.AccessByRole(ui.RoleTextField, testCatalog.In("en", "provider_api_version"))
		if !ok || version.Value != "2024-06-01" {
			t.Errorf("the form's version field is %+v, %v", version, ok)
		}
		e.models = []modelDraft{{id: "deployment", context: "128000", open: true}}
		tester.Frame()
		if _, ok := tester.AccessByRole(ui.RoleTextField, testCatalog.In("en", "provider_deployment_id")); !ok {
			t.Error("an Azure model is not named by its deployment")
		}
		other := preview.Channel{ID: "other", Name: "Other", Method: "x", API: "chat_completions", Endpoint: "https://other.example/v1", KeySet: true, Models: []preview.ChannelModel{{ID: "a", Context: 128000}}}
		e.edit(m, &other)
		e.models[0].open = true
		tester.Frame()
		if _, ok := tester.AccessByRole(ui.RoleTextField, testCatalog.In("en", "provider_api_version")); ok || e.apiVersion != "" {
			t.Errorf("another API asks for a version (%q)", e.apiVersion)
		}
		if _, ok := tester.AccessByRole(ui.RoleTextField, testCatalog.In("en", "provider_model_id")); !ok {
			t.Error("another API's model is not named by its ID")
		}
	})
	// A Bedrock route asks for its region and a Vertex AI route for its
	// project, location and service account; neither asks for an endpoint,
	// which a save derives from them.
	t.Run("cloud fields", func(t *testing.T) {
		m := testController()
		m.Live = true
		var saved []settings.ProviderDraft
		m.ProviderActions.Save = func(d settings.ProviderDraft, _ func(settings.ProviderSaveResult)) { saved = append(saved, d) }
		e := &m.provider
		tester := ui.NewTester(func(c *ui.Context) {
			i18n.Bind(c, testCatalog)
			providerEditorDialog(c, m)
		}, 900, 900)
		field := func(key string) (ui.AccessInfo, bool) {
			return tester.AccessByRole(ui.RoleTextField, testCatalog.In("en", key))
		}

		bedrock := preview.Method{ID: "bedrock", Name: "Amazon Bedrock", Access: "provider_access_endpoint", Keyed: true, Routes: []preview.Route{{ID: "bedrock/0", API: "bedrock", Custom: true}}}
		e.add(m)
		e.configure(bedrock)
		if e.region != "us-east-1" {
			t.Fatalf("a new Bedrock form starts in region %q", e.region)
		}
		tester.Frame()
		if region, ok := field("provider_region"); !ok || region.Value != "us-east-1" {
			t.Errorf("the Bedrock region field is %+v, %v", region, ok)
		}
		if _, ok := field("provider_endpoint"); ok {
			t.Error("a Bedrock form asks for an endpoint")
		}
		if _, ok := field("provider_project"); ok {
			t.Error("a Bedrock form asks for a Vertex AI project")
		}
		e.key, e.models = "bedrock-key", []modelDraft{{id: "us.anthropic.claude-sonnet-4-6", context: "1000000"}}
		e.region = " "
		if got := e.Check(); got != "provider_cloud_required" {
			t.Errorf("no region: %q", got)
		}
		e.region = " eu-west-1 "
		if got := e.save(m); got != "" || len(saved) != 1 {
			t.Fatalf("save %q sent %+v", got, saved)
		}
		if ch := saved[0].Channel; ch.API != "bedrock" || ch.Region != "eu-west-1" || ch.Endpoint != "https://bedrock-runtime.eu-west-1.amazonaws.com" || ch.Project != "" || saved[0].Key != "bedrock-key" {
			t.Errorf("Bedrock save sent %+v", saved[0])
		}

		vertex := preview.Method{ID: "vertex", Name: "Vertex AI", Access: "provider_access_endpoint", Keyed: true, Routes: []preview.Route{{ID: "vertex/0", API: "vertex_ai", Custom: true}}}
		e.add(m)
		e.configure(vertex)
		tester.Frame()
		for _, key := range []string{"provider_project", "provider_location", "provider_service_account"} {
			if _, ok := field(key); !ok {
				t.Errorf("a Vertex AI form has no %s field", key)
			}
		}
		if _, ok := field("provider_region"); ok {
			t.Error("a Vertex AI form asks for a Bedrock region")
		}
		e.key, e.models = `{"type":"service_account"}`, []modelDraft{{id: "gemini-3.5-flash", context: "1048576"}}
		if got := e.Check(); got != "provider_cloud_required" {
			t.Errorf("no project: %q", got)
		}
		e.project, e.location = "proj-123", "us-central1"
		if got := e.save(m); got != "" || len(saved) != 2 {
			t.Fatalf("save %q sent %+v", got, saved)
		}
		if ch := saved[1].Channel; ch.API != "vertex_ai" || ch.Project != "proj-123" || ch.Location != "us-central1" || ch.Endpoint != "https://us-central1-aiplatform.googleapis.com" || ch.Region != "" {
			t.Errorf("Vertex AI save sent %+v", saved[1].Channel)
		}

		// A saved channel opens on its own settings.
		ch := preview.Channel{ID: "saved", Name: "Saved", Method: "vertex/0", API: "vertex_ai", Endpoint: "https://aiplatform.googleapis.com", Project: "other-proj", Location: "global", KeySet: true}
		e.edit(m, &ch)
		tester.Frame()
		if project, ok := field("provider_project"); !ok || project.Value != "other-proj" || e.location != "global" {
			t.Errorf("edit opened project %+v, %v at %q", project, ok, e.location)
		}
	})
	// A saved private cloud endpoint survives unrelated edits; a public one
	// follows the region. Switching between a cloud route and another keeps
	// neither's endpoint in the other and starts the cloud's defaults.
	t.Run("cloud endpoints", func(t *testing.T) {
		m := testController()
		e := &m.provider
		bedrock := preview.Method{ID: "bedrock", Name: "Amazon Bedrock", Access: "provider_access_endpoint", Keyed: true, Routes: []preview.Route{{ID: "bedrock/0", API: "bedrock", Custom: true}}}
		m.Brands = []preview.Brand{{ID: "bedrock", Name: "Amazon Bedrock", Methods: []preview.Method{bedrock}}}
		private := "https://vpce-1.bedrock-runtime.us-east-1.vpce.amazonaws.com"
		e.edit(m, &preview.Channel{ID: "saved", Name: "Saved", Method: "bedrock/0", API: "bedrock", Endpoint: private, Region: "us-east-1", KeySet: true})
		e.name = "Renamed"
		if got := e.channelEndpoint(); got != private {
			t.Errorf("an edit replaced the private endpoint with %q", got)
		}
		e.edit(m, &preview.Channel{ID: "saved", Name: "Saved", Method: "bedrock/0", API: "bedrock", Endpoint: "https://bedrock-runtime.us-east-1.amazonaws.com", Region: "us-east-1", KeySet: true})
		e.region = "eu-west-1"
		if got := e.channelEndpoint(); got != "https://bedrock-runtime.eu-west-1.amazonaws.com" {
			t.Errorf("a public endpoint did not follow the region: %q", got)
		}

		mixed := preview.Method{ID: "mixed", Name: "Mixed", Access: "provider_access_endpoint", Keyed: true, Routes: []preview.Route{
			{ID: "mixed/0", API: "anthropic", Endpoint: "https://api.example"}, {ID: "mixed/1", API: "bedrock", Custom: true}, {ID: "mixed/2", API: "vertex_ai", Custom: true},
		}}
		m.Brands = []preview.Brand{{ID: "mixed", Name: "Mixed", Methods: []preview.Method{mixed}}}
		e.edit(m, &preview.Channel{ID: "saved", Name: "Saved", Method: "mixed/0", API: "anthropic", Endpoint: "https://api.example", KeySet: true})
		e.switchRoute(1)
		if got := e.channelEndpoint(); got != "https://bedrock-runtime.us-east-1.amazonaws.com" || e.region != bedrockDefaultRegion {
			t.Errorf("entering Bedrock sent %q in region %q", got, e.region)
		}
		e.switchRoute(2)
		if got := e.channelEndpoint(); got != "https://aiplatform.googleapis.com" || e.location != vertexDefaultLocation {
			t.Errorf("entering Vertex AI sent %q at %q", got, e.location)
		}
		e.edit(m, &preview.Channel{ID: "saved", Name: "Saved", Method: "mixed/1", API: "bedrock", Endpoint: "https://bedrock-runtime.us-east-1.amazonaws.com", Region: "us-east-1", KeySet: true})
		e.switchRoute(0)
		if e.endpoint != "https://api.example" {
			t.Errorf("leaving Bedrock kept endpoint %q", e.endpoint)
		}
	})
	t.Run("removed model row", func(t *testing.T) {
		m := testController()
		e := &m.provider
		e.add(m)
		e.configure(preview.Method{Name: "Provider", Auth: preview.AuthKey})
		e.keySet = true
		e.models = []modelDraft{{open: true}, {open: true}}
		undoAfterRemove(t, func(c *ui.Context) { providerEditorDialog(c, m) }, "provider_model_id", "settings_delete", 1400)
		if len(m.provider.models) != 1 || m.provider.models[0].id != "second" {
			t.Errorf("undo in the row that moved up left models %+v", m.provider.models)
		}
	})
	// Removing an effort listed before the default leaves the default.
	t.Run("removed effort", func(t *testing.T) {
		m := testController()
		m.provider = providerEditor{open: true, step: stepConfigure, name: "Provider", keySet: true,
			models: []modelDraft{{open: true, id: "model", efforts: []string{"low", "medium", "high"}, def: 1}}}
		tt := ui.NewTesterWith(func(c *ui.Context) {
			i18n.Bind(c, testCatalog)
			providerEditorDialog(c, m)
		}, 900, 900, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true}})
		tt.SetLocale("en")
		tt.Frame()
		tt.Advance(time.Second)
		for _, x := range tt.AccessAll() {
			if x.Role == ui.RoleButton && x.Name == testCatalog.In("en", "settings_delete") {
				tt.ClickAt(x.Bounds.X+x.Bounds.W/2, x.Bounds.Y+x.Bounds.H/2)
				break
			}
		}
		tt.Frame()
		d := m.provider.models[0]
		if !slices.Equal(d.efforts, []string{"medium", "high"}) || d.efforts[d.def] != "medium" {
			t.Errorf("efforts %v, default %d", d.efforts, d.def)
		}
	})
	// A saved model without catalog or discovered efforts still shows the
	// reasoning effort tag input so the user can add custom levels.
	t.Run("saved model without efforts shows effort input", func(t *testing.T) {
		m := testController()
		m.Live = true
		var saved []settings.ProviderDraft
		m.ProviderActions.Save = func(d settings.ProviderDraft, _ func(settings.ProviderSaveResult)) { saved = append(saved, d) }
		e := &m.provider
		e.add(m)
		e.configure(preview.Method{Name: "Provider", Auth: preview.AuthKey})
		e.keySet = true
		e.models = draftsOf([]preview.ChannelModel{{ID: "quick-smart-think", Context: 400000, Tools: true}})
		e.models[0].open = true
		tt := ui.NewTesterWith(func(c *ui.Context) {
			i18n.Bind(c, testCatalog)
			providerEditorDialog(c, m)
		}, 900, 900, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true}})
		tt.SetLocale("en")
		tt.Frame()
		tt.Advance(time.Second)
		field, ok := tt.AccessByRole(ui.RoleTextField, testCatalog.In("en", "provider_effort_placeholder"))
		if !ok {
			t.Fatalf("saved model without efforts hid the effort input: %q", tt.Texts())
		}
		tt.ClickAt(field.Bounds.X+field.Bounds.W/2, field.Bounds.Y+field.Bounds.H/2)
		tt.Type("low,")
		tt.Frame()
		tt.Type("high")
		tt.Key(0, ui.KeyEnter)
		tt.Frame()
		if problem := e.save(m); problem != "" || len(saved) != 1 {
			t.Fatalf("save = %q, drafts = %d", problem, len(saved))
		}
		got := saved[0].Channel.Models[0]
		if !slices.Equal(got.Efforts, []string{"low", "high"}) || got.DefaultEffort != "low" {
			t.Fatalf("saved efforts = %v default %q, want [low high] default low", got.Efforts, got.DefaultEffort)
		}
	})
	// The capability boxes show a model's tags; a change marks them the
	// user's own, and the save carries them.
	t.Run("capabilities", func(t *testing.T) {
		m := testController()
		m.Live = true
		var saved []settings.ProviderDraft
		m.ProviderActions.Save = func(d settings.ProviderDraft, _ func(settings.ProviderSaveResult)) { saved = append(saved, d) }
		e := &m.provider
		e.add(m)
		e.configure(preview.Method{Name: "Provider", Auth: preview.AuthKey})
		e.keySet = true
		e.models = draftsOf([]preview.ChannelModel{
			{ID: "hosted", Context: 128000, Tools: true, Outputs: []string{"text", "audio"}, Generates: []string{viewmodel.GenerateImageTool, viewmodel.GenerateSpeech}},
			{ID: "plain", Context: 128000, Vision: true},
		})
		for i := range m.provider.models {
			m.provider.models[i].open = true
		}
		tt := ui.NewTesterWith(func(c *ui.Context) {
			i18n.Bind(c, testCatalog)
			providerEditorDialog(c, m)
		}, 900, 1400, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true}})
		tt.SetLocale("en")
		tt.Frame()
		tt.Advance(time.Second)
		outputs := testCatalog.In("en", "provider_cap_output")
		texts, outputGroups := tt.Texts(), 0
		for _, text := range texts {
			if text == outputs {
				outputGroups++
			}
		}
		if outputGroups != 1 || !slices.Contains(texts, "Text, Audio") {
			t.Fatalf("native outputs for hosted and unknown models were wrong: %q", texts)
		}
		// boxes are a model's six, in the form's order.
		in := func(group, name string) string { return testCatalog.In("en", group) + " " + testCatalog.In("en", name) }
		names := []string{in("provider_cap_input", "media_image"), in("provider_cap_functions", "provider_cap_tools"), in("provider_cap_functions", "provider_cap_reasoning"),
			in("provider_cap_generates", "media_image"), in("provider_cap_generates", "media_video"), in("provider_cap_generates", "media_speech")}
		boxes := func(model int) (checked [6]bool, at [6]ui.AccessInfo) {
			t.Helper()
			for i, name := range names {
				seen := 0
				for _, x := range tt.AccessAll() {
					if x.Role == ui.RoleCheckBox && x.Name == name {
						if seen == model {
							checked[i], at[i] = x.Checked, x
						}
						seen++
					}
				}
				if seen != 2 {
					t.Fatalf("%d boxes named %q", seen, name)
				}
			}
			return
		}
		click := func(model, box int) {
			t.Helper()
			_, at := boxes(model)
			tt.ClickAt(at[box].Bounds.X+at[box].Bounds.H/2, at[box].Bounds.Y+at[box].Bounds.H/2)
			tt.Frame()
		}
		if got, _ := boxes(0); got != [6]bool{false, true, false, true, false, true} {
			t.Fatalf("hosted model's boxes %v", got)
		}
		if got, _ := boxes(1); got != [6]bool{true, false, false, false, false, false} {
			t.Fatalf("plain model's boxes %v", got)
		}
		hosted, plain := &m.provider.models[0].model, &m.provider.models[1].model
		if hosted.CustomCapabilities || plain.CustomCapabilities {
			t.Fatal("unedited tags are the user's own")
		}
		// The one Image box keeps the hosted tool the model came with.
		click(0, 3)
		if !slices.Equal(hosted.Generates, []string{viewmodel.GenerateSpeech}) || !hosted.CustomCapabilities {
			t.Fatalf("hosted model without Image: %+v", hosted)
		}
		click(0, 3)
		if !slices.Equal(hosted.Generates, []string{viewmodel.GenerateSpeech, viewmodel.GenerateImageTool}) {
			t.Fatalf("hosted model with Image again generates %v", hosted.Generates)
		}
		click(0, 0)
		click(0, 2)
		// A model without the hosted tool generates by the endpoint.
		click(1, 3)
		click(1, 4)
		click(1, 0)
		if !slices.Equal(plain.Generates, []string{viewmodel.GenerateImage, viewmodel.GenerateVideo}) || plain.Vision || !plain.CustomCapabilities {
			t.Fatalf("plain model: %+v", plain)
		}
		if got, _ := boxes(1); got != [6]bool{false, false, false, true, true, false} {
			t.Fatalf("plain model's boxes after edits %v", got)
		}
		if problem := m.provider.save(m); problem != "" || len(saved) != 1 {
			t.Fatalf("save: %q, %d drafts", problem, len(saved))
		}
		first, second := saved[0].Channel.Models[0], saved[0].Channel.Models[1]
		if !first.CustomCapabilities || !first.Vision || !first.Tools || !first.Reasoning || !slices.Equal(first.Generates, []string{viewmodel.GenerateSpeech, viewmodel.GenerateImageTool}) {
			t.Errorf("saved hosted model %+v", first)
		}
		if !second.CustomCapabilities || second.Vision || second.Tools || !slices.Equal(second.Generates, []string{viewmodel.GenerateImage, viewmodel.GenerateVideo}) {
			t.Errorf("saved plain model %+v", second)
		}
	})
	t.Run("context_limits", func(t *testing.T) {
		editor := providerEditor{keySet: true}
		editor.configure(preview.Method{Name: "Provider", Auth: preview.AuthKey})
		for _, value := range []string{"", "0", "-1", "4295095296", "not a limit"} {
			editor.models = []modelDraft{{id: "model", context: value}}
			if got := editor.Check(); got != "provider_limits_invalid" {
				t.Fatalf("context %q: %q", value, got)
			}
		}
	})
}

// undoAfterRemove types "first" and "second" into the two rows of a dialog's
// list, removes the first and undoes in the row that moved up, which must
// keep its own text: the caller checks the list holds "second" alone. field
// and remove are the i18n keys of a row's input and remove button.
func undoAfterRemove(t *testing.T, draw func(*ui.Context), field, remove string, height int) {
	t.Helper()
	tt := ui.NewTesterWith(func(c *ui.Context) {
		i18n.Bind(c, testCatalog)
		draw(c)
	}, 900, height, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true}})
	tt.SetLocale("en")
	tt.Frame()
	tt.Advance(time.Second)
	named := func(role ui.Role, key string) (found []ui.Rect) {
		for _, x := range tt.AccessAll() {
			if x.Role == role && x.Name == testCatalog.In("en", key) {
				found = append(found, x.Bounds)
			}
		}
		return found
	}
	at := func(r ui.Rect) { tt.ClickAt(r.X+r.W/2, r.Y+r.H/2) }
	inputs := named(ui.RoleTextField, field)
	if len(inputs) != 2 {
		t.Fatalf("%d %s inputs: %q", len(inputs), field, tt.Texts())
	}
	at(inputs[0])
	tt.Type("first")
	at(inputs[1])
	tt.Type("second")
	if buttons := named(ui.RoleButton, remove); len(buttons) > 0 {
		at(buttons[0])
	} else {
		// A list row is a button that takes its title for its name, and its
		// remove action is the 28 DIP button at its end, 16 from the edge.
		row, ok := tt.AccessByRole(ui.RoleButton, "first")
		if !ok {
			t.Fatalf("no %s button and no row named by its text", remove)
		}
		tt.ClickAt(row.Bounds.X+row.Bounds.W-30, row.Bounds.Y+row.Bounds.H/2)
	}
	tt.Frame()
	inputs = named(ui.RoleTextField, field)
	if len(inputs) != 1 {
		t.Fatalf("%d %s inputs after removing one", len(inputs), field)
	}
	at(inputs[0])
	tt.Key(ui.Cmd, ui.KeyZ)
	tt.Frame()
}

func testController() *Controller { return For(settings.New()) }
