package live

import (
	"image/png"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/preferences"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/providers"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// testProviderFlow drives Settings › Providers against a Node that lists
// the bundled preset catalog: the brands and their marks, the family
// filter, and the Add provider flow down to the command a save sends. It
// writes the frames it passes to .runtime/ui-shots.
func testProviderFlow(t *testing.T) {
	listing := providers.Listing(protocol.ConversationPresetsCatalog{}, false)
	source := protocol.ConversationPresetsSource{Kind: "builtin"}
	saved := func(n byte, name, preset string, api protocol.ConversationModelApi, authentication protocol.Authentication) protocol.ConversationProvider {
		p := protocol.DefaultConversationProvider()
		p.ID, p.Name, p.Api, p.Enabled, p.Revision, p.Authentication = protocol.ProviderID{n}, name, api, true, 1, authentication
		if preset != "" {
			p.Preset = &protocol.ConversationPresetsPresetRef{Source: source, ID: preset}
		}
		model := protocol.DefaultConversationModel()
		model.ID, model.Context, model.Output = strings.ToLower(name)+"-model", 128000, 16384
		p.Models, p.DefaultModel = []protocol.ConversationModel{model}, model.ID
		return p
	}
	snapshot := protocol.DefaultSnapshot()
	snapshot.Node = protocol.NodeID{1}
	snapshot.Providers = []protocol.ConversationProvider{
		saved(1, "Work", "openai", protocol.ConversationModelApiResponses, protocol.AuthenticationApiKey),
		saved(2, "Subscription", "claude-oauth", protocol.ConversationModelApiAnthropic, protocol.AuthenticationClaudeOAuth),
		saved(3, "Studio", "gemini", protocol.ConversationModelApiGemini, protocol.AuthenticationApiKey),
		saved(4, "Coding plan", "zhipu", protocol.ConversationModelApiChatCompletions, protocol.AuthenticationApiKey),
		saved(5, "Reasoner", "deepseek", protocol.ConversationModelApiDeepSeek, protocol.AuthenticationApiKey),
		saved(6, "Gateway", "openrouter", protocol.ConversationModelApiChatCompletions, protocol.AuthenticationApiKey),
		saved(7, "Local", "", protocol.ConversationModelApiChatCompletions, protocol.AuthenticationApiKey),
	}
	var saves []protocol.CommandSaveProviderPayload
	transport := &conversationTestTransport{snapshot: snapshot}
	transport.handle = func(request protocol.Request) (link.Response, bool) {
		switch request.Command.Kind {
		case "list_channel_presets":
			return link.Response{Output: protocol.Output{Kind: "channel_presets", Data: listing}}, true
		case "save_provider":
			input, _ := detail[protocol.CommandSaveProviderPayload](request.Command.Data)
			saves = append(saves, input)
			return link.Response{Output: protocol.Output{Kind: "provider", Data: input.Provider}}, true
		}
		return link.Response{}, false
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
	tester.SetLocale("zh-CN")
	tester.SetDark(true)
	zh := func(key string) string { return i18n.In("zh-CN", key) }
	await := func(what string, done func() bool) {
		t.Helper()
		deadline := time.Now().Add(10 * time.Second)
		for tester.Frame(); !done(); tester.Frame() {
			if time.Now().After(deadline) {
				t.Fatalf("timed out waiting for %s", what)
			}
			time.Sleep(2 * time.Millisecond)
		}
	}
	settle := func() {
		tester.Advance(time.Second)
		tester.Frame()
	}
	dir := filepath.Join("..", "..", "..", ".runtime", "ui-shots")
	if err := os.MkdirAll(dir, 0o755); err != nil {
		t.Fatal(err)
	}
	// shot writes the frame in the dark and the light appearance.
	shot := func(name string) {
		t.Helper()
		for _, dark := range []bool{false, true} {
			tester.SetDark(dark)
			settle()
			file, err := os.Create(filepath.Join(dir, name+map[bool]string{false: "-light", true: "-dark"}[dark]+".png"))
			if err != nil {
				t.Fatal(err)
			}
			if err := png.Encode(file, tester.Image()); err != nil {
				t.Fatal(err)
			}
			file.Close()
		}
	}
	named := func(role ui.Role) (names []string) {
		for _, x := range tester.AccessAll() {
			if x.Role == role {
				names = append(names, x.Name)
			}
		}
		return
	}
	click := func(role ui.Role, name string) {
		t.Helper()
		x, ok := tester.AccessByRole(role, name)
		if !ok {
			t.Fatalf("no %q among %q", name, named(role))
		}
		tester.ClickAt(x.Bounds.X+x.Bounds.W/2, x.Bounds.Y+x.Bounds.H/2)
		settle()
	}
	await("the preset directory", func() bool { return len(m.Brands) > 0 })

	// The brands of the bundled catalog: each named for the brand, not for
	// whichever of its entries the listing has first, with one method an
	// entry and no two methods a reader could not tell apart.
	var brands, logins, keyed []string
	for _, b := range m.Brands {
		brands = append(brands, b.Name)
		rows := map[string]bool{}
		api := 0
		for _, x := range b.Methods {
			if icons.Brand(x.Icon) == nil {
				t.Errorf("method %q of %q has no mark", x.ID, b.Name)
			}
			if x.Login != "" {
				logins = append(logins, zh(x.Login))
				if x.Access != "provider_access_login" || x.Keyed {
					t.Errorf("sign-in %q is accessed by %q", x.ID, x.Access)
				}
				continue
			}
			api++
			if row := x.Name + "\n" + x.Access; rows[row] {
				t.Errorf("brand %q lists %q twice", b.Name, x.Name)
			} else {
				rows[row] = true
			}
			if x.Access == "provider_access_login" {
				t.Errorf("API method %q is under the sign-ins", x.ID)
			}
		}
		if api > 0 {
			keyed = append(keyed, b.Name)
		}
		if icons.Brand(b.ID) == nil || icons.BrandColor(b.ID) == nil {
			t.Errorf("brand %q (%s) has no mark", b.Name, b.ID)
		}
	}
	want := []string{"Anthropic", "Baidu", "Cohere", "DeepSeek", "Doubao", "Gemini", "Kimi", "MiniMax", "Mistral", "OpenAI", "OpenCode", "Qwen", "xAI", "Zhipu"}
	if !slices.Equal(brands, want) || !slices.Equal(keyed, want) {
		t.Errorf("brands %q, with API methods %q, want %q", brands, keyed, want)
	}
	if slices.Sort(logins); !slices.Equal(logins, []string{"ChatGPT", "Claude"}) {
		t.Errorf("sign-ins %q", logins)
	}
	// Every entry of the catalog, offered or not, has a mark in the
	// collection, as its brand has.
	for _, entry := range listing.Entries {
		if icons.Brand(entry.Preset.ID) == nil || icons.Brand(presetBrand(entry)) == nil {
			t.Errorf("preset %q of brand %q has no mark", entry.Preset.ID, presetBrand(entry))
		}
	}
	methods := func(brand string) (names []string) {
		for _, b := range m.Brands {
			if b.ID == brand {
				for _, x := range b.Methods {
					names = append(names, x.Name)
				}
			}
		}
		return
	}
	if got := methods("openai"); !slices.Equal(got, []string{"ChatGPT OAuth", "OpenAI", "OpenAI Compatible"}) {
		t.Errorf("OpenAI's methods %q", got)
	}

	// The saved channels belong to those brands, a channel without a preset
	// by its protocol, and one whose preset is no longer offered by it.
	await("the channels", func() bool { return len(m.Channels) == len(snapshot.Providers) && m.Channels[0].Brand != "" })
	for i, want := range [][3]string{{"openai", "OpenAI", "openai"}, {"anthropic", "Anthropic", "claude-oauth"}, {"gemini", "Gemini", "gemini"}, {"zhipu", "Zhipu", "zhipu"}, {"deepseek", "DeepSeek", "deepseek"}, {"openrouter", "OpenRouter", "openrouter"}, {"openai", "OpenAI", ""}} {
		ch := m.Channels[i]
		if ch.Brand != want[0] || ch.BrandName != want[1] || ch.Icon != want[2] || channelIcon(ch.Icon, ch.Brand) == nil {
			t.Errorf("channel %q is of brand %q (%q) with mark %q", ch.Name, ch.Brand, ch.BrandName, ch.Icon)
		}
	}
	shot("providers-list")

	// The filter: All, the offered brands, then the brand only a channel
	// has; each brand's option has its mark.
	filter := zh("provider_type")
	click(ui.RoleMenuButton, filter)
	var options []string
	for _, x := range tester.AccessAll() {
		if x.Role == ui.RoleMenuItem || x.Role == ui.RoleMenuItemRadio || x.Role == ui.RoleMenuItemCheckBox {
			options = append(options, x.Name)
		}
	}
	if expected := append(append([]string{zh("provider_all")}, want...), "OpenRouter"); !slices.Equal(options, expected) {
		t.Errorf("filter options %q, want %q", options, expected)
	}
	if icons.BrandColor("openrouter") == nil {
		t.Error("the unoffered brand's option has no mark")
	}
	shot("providers-family-filter")
	if x, ok := tester.Access("Anthropic"); ok {
		tester.ClickAt(x.Bounds.X+x.Bounds.W/2, x.Bounds.Y+x.Bounds.H/2)
		settle()
	}
	if m.Family != "anthropic" {
		t.Fatalf("the filter chose %q", m.Family)
	}
	if tester.HasText("Work") || !tester.HasText("Subscription") {
		t.Errorf("the Anthropic filter shows %q", tester.Texts())
	}
	m.Family = ""
	settle()

	// Add: the sign-ins under their heading, then the brands under theirs.
	click(ui.RoleButton, zh("settings_add"))
	login, ok := tester.Find(zh("provider_access_login"))
	key, found := tester.Find(zh("provider_access_key"))
	if !ok || !found || login.Y >= key.Y {
		t.Fatalf("the brand step's headings are at %v and %v: %q", login, key, tester.Texts())
	}
	for _, name := range []string{"ChatGPT", "Claude"} {
		if chip, ok := tester.AccessByRole(ui.RoleButton, name); !ok || chip.Bounds.Y <= login.Y || chip.Bounds.Y >= key.Y {
			t.Errorf("sign-in %q is at %v, outside its group", name, chip.Bounds)
		}
	}
	for _, name := range want {
		if chip, ok := tester.AccessByRole(ui.RoleButton, name); !ok || chip.Bounds.Y <= key.Y {
			t.Errorf("brand %q is at %v, outside its group", name, chip.Bounds)
		}
	}
	if _, ok := tester.AccessByRole(ui.RoleButton, "ChatGPT OAuth"); ok {
		t.Error("a sign-in is listed as a brand")
	}
	shot("provider-brand-step")

	// A brand with one API method opens its form, and Back returns to the
	// brands.
	click(ui.RoleButton, "DeepSeek")
	if !tester.HasText(zh("provider_configure")) || tester.HasText(zh("provider_choose_method")) {
		t.Fatalf("one method did not open its form: %q", tester.Texts())
	}
	if tabs := named(ui.RoleRadio); len(tabs) != 0 {
		t.Errorf("one route shows choices %q", tabs)
	}
	click(ui.RoleButton, zh("provider_step_back"))
	if !tester.HasText(zh("provider_add_title")) {
		t.Fatalf("Back from a single method went to %q", tester.Texts())
	}
	// Routes that differ in product and region are choices of one form.
	click(ui.RoleButton, "Zhipu")
	if tabs := named(ui.RoleRadio); len(tabs) != 4 || !slices.Contains(tabs, zh("provider_region_china")) || !slices.Contains(tabs, zh("provider_region_global")) {
		t.Errorf("Zhipu's choices %q", tabs)
	}
	shot("provider-form-zhipu")
	click(ui.RoleButton, zh("provider_step_back"))

	// OpenAI has two API methods, distinct; its sign-in is not among them.
	click(ui.RoleButton, "OpenAI")
	if !tester.HasText(zh("provider_choose_method")) || !tester.HasText("OpenAI Compatible") || tester.HasText("ChatGPT OAuth") {
		t.Fatalf("OpenAI's method step shows %q", tester.Texts())
	}
	if !tester.HasText(zh("provider_access_key")) || !tester.HasText(zh("provider_access_endpoint")) {
		t.Errorf("the methods are not told apart by access: %q", tester.Texts())
	}
	shot("provider-method-step")

	// OpenAI Compatible: its two protocols are one segmented control.
	if err := tester.Click("OpenAI Compatible"); err != nil {
		t.Fatal(err)
	}
	settle()
	if tabs := named(ui.RoleRadio); !slices.Equal(tabs, []string{"Responses", "Chat Completions"}) {
		t.Fatalf("the protocol choices are %q", tabs)
	}
	typed := func(label, text string) {
		t.Helper()
		click(ui.RoleTextField, label)
		tester.Type(text)
		tester.Frame()
	}
	typed(zh("provider_endpoint"), "https://gateway.example/v1")
	typed(zh("provider_api_key"), "sk-typed")
	click(ui.RoleRadio, "Chat Completions")
	field := func(label string) string {
		x, _ := tester.AccessByRole(ui.RoleTextField, label)
		return x.Value
	}
	if field(zh("provider_endpoint")) != "https://gateway.example/v1" || field(zh("settings_name")) != "OpenAI Compatible" {
		t.Errorf("the switch left endpoint %q and name %q", field(zh("provider_endpoint")), field(zh("settings_name")))
	}
	shot("provider-form-compatible")
	// Back from a method of several returns to the methods.
	click(ui.RoleButton, zh("provider_step_back"))
	if !tester.HasText(zh("provider_choose_method")) {
		t.Fatalf("Back from the form went to %q", tester.Texts())
	}
	if err := tester.Click("OpenAI Compatible"); err != nil {
		t.Fatal(err)
	}
	settle()
	typed(zh("provider_endpoint"), "https://gateway.example/v1")
	typed(zh("provider_api_key"), "sk-typed")
	click(ui.RoleRadio, "Chat Completions")
	click(ui.RoleButton, zh("provider_add_model"))
	typed(zh("provider_model_id"), "gateway-model")
	click(ui.RoleButton, "128K")
	shot("provider-form-compatible-model")
	click(ui.RoleButton, zh("provider_save"))
	sent := func() (n int) {
		transport.mu.Lock()
		defer transport.mu.Unlock()
		return len(saves)
	}
	await("the save", func() bool { return sent() == 1 })
	transport.mu.Lock()
	p := saves[0].Provider
	secret := saves[0].Secret
	transport.mu.Unlock()
	if p.Api != protocol.ConversationModelApiChatCompletions || p.Endpoint != "https://gateway.example/v1" || p.Authentication != protocol.AuthenticationApiKey || secret == nil ||
		p.Preset == nil || p.Preset.ID != "compatible-responses" || p.Preset.Route == nil || p.Preset.Route.Protocol != "openai-chat-completions" ||
		len(p.Models) != 1 || p.Models[0].ID != "gateway-model" {
		t.Errorf("the save sent %+v with preset %+v", p, p.Preset)
	}
	await("the editor to close", func() bool { _, open := tester.AccessByRole(ui.RoleButton, zh("provider_save")); return !open })

	// A sign-in opens its form: no key, and the action signs in.
	click(ui.RoleButton, zh("settings_add"))
	click(ui.RoleButton, "ChatGPT")
	if _, ok := tester.AccessByRole(ui.RoleTextField, zh("provider_api_key")); ok {
		t.Error("a sign-in asks for an API key")
	}
	if _, ok := tester.AccessByRole(ui.RoleButton, zh("provider_save_login")); !ok {
		t.Errorf("a sign-in has no sign-in action: %q", named(ui.RoleButton))
	}
	shot("provider-form-sign-in")
	click(ui.RoleButton, zh("provider_step_back"))
	if !tester.HasText(zh("provider_add_title")) {
		t.Errorf("Back from a sign-in went to %q", tester.Texts())
	}
}

// channelIcon is the mark a channel's tile draws: its own, else its brand's.
func channelIcon(icon, brand string) *ui.SVG {
	if mark := icons.Brand(icon); mark != nil {
		return mark
	}
	return icons.Brand(brand)
}
