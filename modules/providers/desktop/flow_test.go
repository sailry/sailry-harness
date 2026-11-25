package desktop

import (
	"image/png"
	"os"
	"path/filepath"
	"slices"
	"strconv"
	"strings"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
)

func TestProviderFamilyFilterAndAddFlow(t *testing.T) {
	const node = "fixture-node"
	const compatible = "compatible"
	assembly := shell.Assemble(Register)
	catalog := i18n.New(assembly.Locales...)
	m := settings.NewLive()
	m.SetNativePages(assembly.SettingsPages)
	m.Open(settings.Providers)
	m.Catalog.Bundled = true
	m.ManagedHosts = func() []settings.ProviderHost {
		return []settings.ProviderHost{{Node: node, Local: true, Connected: true}}
	}
	api := func(id, name string) viewmodel.Method {
		return viewmodel.Method{Node: node, ID: id, Name: name, Icon: id, Keyed: true, Access: "provider_access_key", API: "chat_completions", Endpoint: "https://provider.example.test/v1"}
	}
	signIn := func(id, name, key string) viewmodel.Method {
		return viewmodel.Method{Node: node, ID: id, Name: name, Icon: id, Auth: viewmodel.AuthOAuth, Access: "provider_access_login", Login: key, API: "responses"}
	}
	want := []string{"Amazon Bedrock", "Anthropic", "Baidu", "Cohere", "DeepSeek", "Doubao", "Gemini", "Kimi", "MiniMax", "Mistral", "OpenAI", "OpenCode", "Qwen", "Vertex AI", "xAI", "Zhipu"}
	for _, name := range slices.Insert(slices.Clone(want), 7, "GitHub Copilot") {
		id := strings.ToLower(strings.ReplaceAll(name, " ", "-"))
		brand := viewmodel.Brand{Node: node, ID: id, Name: name, Methods: []viewmodel.Method{api(id, name)}}
		switch name {
		case "GitHub Copilot":
			brand.Methods = []viewmodel.Method{signIn("copilot", "Copilot", "provider_access_login")}
		case "Anthropic":
			brand.Methods = append([]viewmodel.Method{signIn("claude-oauth", "Claude", "provider_access_login")}, brand.Methods...)
		case "OpenAI":
			brand.Methods = append([]viewmodel.Method{signIn("chat-gpt-oauth", "ChatGPT", "provider_access_login")}, brand.Methods...)
			brand.Methods = append(brand.Methods, viewmodel.Method{Node: node, ID: compatible, Name: "OpenAI Compatible", Access: "provider_access_endpoint", Keyed: true, Custom: true, Routes: []viewmodel.Route{
				{ID: compatible + "/0", API: "responses", Custom: true}, {ID: compatible + "/1", API: "chat_completions", Custom: true},
			}})
		case "Zhipu":
			for i, product := range []string{"metered", "coding"} {
				for j, region := range []string{"china", "global"} {
					brand.Methods[0].Routes = append(brand.Methods[0].Routes, viewmodel.Route{ID: "zhipu/" + strconv.Itoa(2*i+j), API: "chat_completions", Endpoint: "https://zhipu.example.test/v1", Product: product, Region: region})
				}
			}
		}
		m.Brands = append(m.Brands, brand)
	}
	for i, row := range []struct{ name, brand, label, icon string }{
		{"Work", "openai", "OpenAI", "openai"}, {"Subscription", "anthropic", "Anthropic", "claude-oauth"},
		{"Studio", "gemini", "Gemini", "gemini"}, {"Coding plan", "zhipu", "Zhipu", "zhipu"},
		{"Reasoner", "deepseek", "DeepSeek", "deepseek"}, {"Gateway", "openrouter", "OpenRouter", "openrouter"}, {"Local", "openai", "OpenAI", ""},
	} {
		ch := viewmodel.Channel{ID: row.name, Node: node, Name: row.name, Brand: row.brand, BrandName: row.label, Icon: row.icon, Enabled: true, KeySet: true,
			API: "chat_completions", Models: []viewmodel.ChannelModel{{ID: "model"}}}
		if i == 1 {
			ch.Auth, ch.SignedIn, ch.API = viewmodel.AuthOAuth, true, "anthropic"
		}
		if channelMark(&ch) == nil {
			t.Errorf("channel %q has no tile mark", ch.Name)
		}
		m.Channels = append(m.Channels, ch)
	}
	var saves []settings.ProviderDraft
	m.ProviderActions.NewID = func() string { return "new-provider" }
	m.ProviderActions.Hosts = m.ManagedHosts
	m.ProviderActions.Save = func(draft settings.ProviderDraft, done func(settings.ProviderSaveResult)) {
		saves = append(saves, draft)
		done(settings.ProviderSaveResult{Succeeded: []string{node}})
	}
	view := ui.NewTesterWith(func(c *ui.Context) {
		i18n.Bind(c, catalog)
		settings.Page(c, m, nil, settings.Look{})
		ui.ToastViewportBase(c, func(ui.Element, []ui.Toast) {})
	}, 1100, 900, ui.TesterOptions{Dark: true, Preferences: ui.Preferences{ReduceTransparency: true, ReduceMotion: os.Getenv("SAILRY_SNAPSHOTS") == ""}})
	view.SetLocale("zh-CN")
	if os.Getenv("SAILRY_SNAPSHOTS") == "" {
		view.SetScale(0.125)
	}
	zh := func(key string) string { return catalog.In("zh-CN", key) }
	settle := func() { view.Advance(time.Second); view.Frame() }
	shot := func(name string) {
		t.Helper()
		dir := os.Getenv("SAILRY_SNAPSHOTS")
		if dir == "" {
			return
		}
		if err := os.MkdirAll(dir, 0o755); err != nil {
			t.Fatal(err)
		}
		for _, dark := range []bool{false, true} {
			view.SetDark(dark)
			settle()
			file, err := os.Create(filepath.Join(dir, name+map[bool]string{false: "-light", true: "-dark"}[dark]+".png"))
			if err != nil {
				t.Fatal(err)
			}
			err = png.Encode(file, view.Image())
			closeErr := file.Close()
			if err != nil || closeErr != nil {
				t.Fatalf("write snapshot: %v / %v", err, closeErr)
			}
		}
	}
	named := func(role ui.Role) (names []string) {
		for _, x := range view.AccessAll() {
			if x.Role == role {
				names = append(names, x.Name)
			}
		}
		return
	}
	click := func(role ui.Role, name string) {
		t.Helper()
		x, ok := view.AccessByRole(role, name)
		if !ok {
			t.Fatalf("no %q among %q", name, named(role))
		}
		view.ClickAt(x.Bounds.X+x.Bounds.W/2, x.Bounds.Y+x.Bounds.H/2)
		settle()
	}
	view.Frame()
	shot("providers-list")
	if !view.HasText("OAuth · " + catalog.In("zh-CN", "provider_model_count", "count", 1)) {
		t.Error("an OAuth channel was labeled by its transport protocol")
	}

	filter := zh("provider_type")
	click(ui.RoleMenuButton, filter)
	var options []string
	for _, x := range view.AccessAll() {
		if x.Role == ui.RoleMenuItem || x.Role == ui.RoleMenuItemRadio || x.Role == ui.RoleMenuItemCheckBox {
			options = append(options, x.Name)
		}
	}
	if expected := append(append([]string{zh("provider_all")}, slices.Insert(slices.Clone(want), 7, "GitHub Copilot")...), "OpenRouter"); !slices.Equal(options, expected) {
		t.Errorf("filter options %q, want %q", options, expected)
	}
	if icons.BrandColor("openrouter") == nil {
		t.Error("the unoffered brand's option has no mark")
	}
	shot("providers-family-filter")
	chooseFamily := func(name string) {
		t.Helper()
		for _, role := range []ui.Role{ui.RoleMenuItem, ui.RoleMenuItemRadio, ui.RoleMenuItemCheckBox} {
			if _, ok := view.AccessByRole(role, name); ok {
				click(role, name)
				return
			}
		}
		t.Fatalf("no channel family %q: %q", name, view.Texts())
	}
	chooseFamily("Anthropic")
	if view.HasText("Work") || !view.HasText("Subscription") {
		t.Fatalf("the Anthropic filter shows %q", view.Texts())
	}
	click(ui.RoleMenuButton, filter)
	chooseFamily(zh("provider_all"))
	if !view.HasText("Work") || !view.HasText("Subscription") {
		t.Fatalf("clearing the filter shows %q", view.Texts())
	}

	click(ui.RoleButton, zh("settings_add"))
	login, ok := view.Find(zh("provider_access_login"))
	key, found := view.Find(zh("provider_access_key"))
	if !ok || !found || login.Y >= key.Y {
		t.Fatalf("the brand step's headings are at %v and %v: %q", login, key, view.Texts())
	}
	for _, name := range []string{"ChatGPT", "Claude", "Copilot"} {
		if chip, ok := view.AccessByRole(ui.RoleButton, name); !ok || chip.Bounds.Y <= login.Y || chip.Bounds.Y >= key.Y {
			t.Errorf("sign-in %q is at %v, outside its group", name, chip.Bounds)
		}
	}
	for _, name := range want {
		if chip, ok := view.AccessByRole(ui.RoleButton, name); !ok || chip.Bounds.Y <= key.Y {
			t.Errorf("brand %q is at %v, outside its group", name, chip.Bounds)
		}
	}
	if _, ok := view.AccessByRole(ui.RoleButton, "ChatGPT OAuth"); ok {
		t.Error("a sign-in is listed as a brand")
	}
	shot("provider-brand-step")

	click(ui.RoleButton, "DeepSeek")
	if !view.HasText(zh("provider_configure")) || view.HasText(zh("provider_choose_method")) || len(named(ui.RoleRadio)) != 0 {
		t.Fatalf("one method did not open its form without choices: %q", view.Texts())
	}
	click(ui.RoleButton, zh("provider_step_back"))
	if !view.HasText(zh("provider_add_title")) {
		t.Fatalf("Back from a single method went to %q", view.Texts())
	}
	click(ui.RoleButton, "Zhipu")
	if tabs := named(ui.RoleRadio); len(tabs) != 4 || !slices.Contains(tabs, zh("provider_region_china")) || !slices.Contains(tabs, zh("provider_region_global")) {
		t.Errorf("Zhipu's choices %q", tabs)
	}
	shot("provider-form-zhipu")
	click(ui.RoleButton, zh("provider_step_back"))

	click(ui.RoleButton, "OpenAI")
	if !view.HasText(zh("provider_choose_method")) || !view.HasText("OpenAI Compatible") || view.HasText("ChatGPT OAuth") {
		t.Fatalf("OpenAI's method step shows %q", view.Texts())
	}
	if !view.HasText(zh("provider_access_key")) || !view.HasText(zh("provider_access_endpoint")) {
		t.Errorf("the methods are not told apart by access: %q", view.Texts())
	}
	shot("provider-method-step")
	typeIn := func(label, text string) {
		t.Helper()
		click(ui.RoleTextField, label)
		view.Key(ui.Cmd, ui.KeyA)
		view.Type(text)
		view.Frame()
	}
	configure := func() {
		t.Helper()
		if err := view.Click("OpenAI Compatible"); err != nil {
			t.Fatal(err)
		}
		settle()
		if tabs := named(ui.RoleRadio); !slices.Equal(tabs, []string{"Responses", "Chat Completions"}) {
			t.Fatalf("the protocol choices are %q", tabs)
		}
		typeIn(zh("provider_endpoint"), "https://gateway.example/v1")
		typeIn(zh("provider_api_key"), "sk-typed")
		click(ui.RoleRadio, "Chat Completions")
	}
	configure()
	endpoint, _ := view.AccessByRole(ui.RoleTextField, zh("provider_endpoint"))
	name, _ := view.AccessByRole(ui.RoleTextField, zh("settings_name"))
	if endpoint.Value != "https://gateway.example/v1" || name.Value != "OpenAI Compatible" {
		t.Errorf("the switch left endpoint %q and name %q", endpoint.Value, name.Value)
	}
	shot("provider-form-compatible")
	click(ui.RoleButton, zh("provider_step_back"))
	if !view.HasText(zh("provider_choose_method")) {
		t.Fatalf("Back from the form went to %q", view.Texts())
	}
	configure()
	click(ui.RoleButton, zh("provider_add_model"))
	typeIn(zh("provider_model_id"), "gateway-model")
	click(ui.RoleButton, "128K")
	shot("provider-form-compatible-model")
	click(ui.RoleButton, zh("provider_save"))
	if len(saves) != 1 {
		t.Fatalf("the form sent %d drafts: %q", len(saves), view.Texts())
	}
	ch := saves[0].Channel
	if ch.Node != node || ch.API != "chat_completions" || ch.Method != compatible+"/1" || ch.Endpoint != "https://gateway.example/v1" ||
		saves[0].Key != "sk-typed" || len(ch.Models) != 1 || ch.Models[0].ID != "gateway-model" || ch.Models[0].Context != 128000 {
		t.Errorf("the form saved %+v with a %d-byte key", ch, len(saves[0].Key))
	}
	if _, open := view.AccessByRole(ui.RoleButton, zh("provider_save")); open {
		t.Fatal("a successful save did not close the provider form")
	}

	click(ui.RoleButton, zh("settings_add"))
	click(ui.RoleButton, "ChatGPT")
	if _, ok := view.AccessByRole(ui.RoleTextField, zh("provider_api_key")); ok {
		t.Error("a sign-in asks for an API key")
	}
	if _, ok := view.AccessByRole(ui.RoleButton, zh("provider_save_login")); !ok {
		t.Errorf("a sign-in has no sign-in action: %q", named(ui.RoleButton))
	}
	shot("provider-form-sign-in")
	click(ui.RoleButton, zh("provider_step_back"))
	if !view.HasText(zh("provider_add_title")) {
		t.Errorf("Back from a sign-in went to %q", view.Texts())
	}
}

func TestDeclaredPresetWarningPrecedesProviderForm(t *testing.T) {
	const warning = "Heavy use may count against this account"
	m := settings.NewLive()
	assembly := shell.Assemble(Register)
	m.SetNativePages(assembly.SettingsPages)
	m.Open(settings.Providers)
	m.Catalog.Bundled = true
	m.Brands = []viewmodel.Brand{
		{ID: "local", Name: "Local agent", Methods: []viewmodel.Method{{ID: "local", Name: "Local agent", Access: "provider_access_endpoint", API: "plugin", Warning: warning}}},
		{ID: "quiet", Name: "Quiet agent", Methods: []viewmodel.Method{{ID: "quiet", Name: "Quiet agent", Access: "provider_access_endpoint", API: "plugin"}}},
	}
	writes := 0
	m.ProviderActions = settings.ProviderActions{
		Save:     func(settings.ProviderDraft, func(settings.ProviderSaveResult)) { writes++ },
		Discover: func(settings.ProviderDraft, func([]viewmodel.ChannelModel)) { writes++ },
		SignIn:   func(string, string) { writes++ },
	}
	view := ui.NewTesterWith(func(c *ui.Context) {
		i18n.Bind(c, testCatalog)
		settings.Page(c, m, nil, settings.Look{})
	}, 1100, 900, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true, ReduceMotion: true}})
	view.SetLocale("en")
	view.Frame()
	en := func(key string) string { return testCatalog.In("en", key) }
	click := func(label string) {
		t.Helper()
		if err := view.Click(label); err != nil {
			t.Fatalf("%v: %q", err, view.Texts())
		}
		view.Frame()
		view.Advance(time.Second)
	}
	withForm := func() bool { return view.HasText(en("provider_configure")) }
	click(en("settings_add"))
	click("Local agent")
	if !view.HasText(warning) || !view.HasText(en("settings_add")+" Local agent") || withForm() || writes != 0 {
		t.Fatalf("the warning did not precede the form: %d actions, %q", writes, view.Texts())
	}
	click(en("settings_cancel"))
	if view.HasText(warning) || !view.HasText(en("provider_add_title")) || withForm() || writes != 0 || len(m.Channels) != 0 {
		t.Fatalf("Cancel left %d actions, %d channels: %q", writes, len(m.Channels), view.Texts())
	}
	click("Local agent")
	if !view.HasText(warning) {
		t.Fatal("a second pick bypassed the warning")
	}
	click(en("provider_login_continue"))
	if view.HasText(warning) || !withForm() || writes != 0 || len(m.Channels) != 0 {
		t.Fatalf("Continue left the warning or made a channel: %d actions, %q", writes, view.Texts())
	}
	click(en("provider_step_back"))
	click("Quiet agent")
	if !withForm() || view.HasText(warning) || writes != 0 {
		t.Fatalf("a preset without a warning asked or submitted: %d actions, %q", writes, view.Texts())
	}
}
