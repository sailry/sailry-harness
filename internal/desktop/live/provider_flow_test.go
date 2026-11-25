package live

import (
	"slices"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/providers"
	"github.com/sailry/sailry-harness/internal/protocol"
	providerdesktop "github.com/sailry/sailry-harness/modules/providers/desktop"
)

func TestBundledProviderProjectionAndSaveContract(t *testing.T) {
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
	snapshot.ModelCatalog.Bundled = true
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
	t.Cleanup(func() { _ = controller.Close() })
	assembly := shell.Assemble(providerdesktop.Register)
	catalog := i18n.New(assembly.Locales...)
	r, settle := mountSettingsPageFixture(t, controller, assembly)
	r.openNodeSettings(settings.Providers)
	settle()
	m := r.App.Settings

	var brands, logins, keyed []string
	methods := map[string][]string{}
	oauth := map[string][4]string{}
	var compatible viewmodel.Method
	for _, b := range m.Brands {
		brands = append(brands, b.Name)
		rows := map[string]bool{}
		api := 0
		for _, x := range b.Methods {
			methods[b.ID] = append(methods[b.ID], x.Name)
			oauth[x.ID] = x.OAuth
			if x.Name == "OpenAI Compatible" {
				compatible = x
			}
			if icons.Brand(x.Icon) == nil {
				t.Errorf("method %q of %q has no mark", x.ID, b.Name)
			}
			if x.Login != "" {
				if x.Login == "provider_access_login" {
					logins = append(logins, x.Name)
				} else {
					logins = append(logins, catalog.In("zh-CN", x.Login))
				}
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
	want := []string{"Amazon Bedrock", "Anthropic", "Baidu", "Cohere", "DeepSeek", "Doubao", "Gemini", "Kimi", "MiniMax", "Mistral", "OpenAI", "OpenCode", "Qwen", "Vertex AI", "xAI", "Zhipu"}
	if listed := slices.Insert(slices.Clone(want), 7, "GitHub Copilot"); !slices.Equal(brands, listed) || !slices.Equal(keyed, want) {
		t.Errorf("brands %q, with API methods %q, want %q", brands, keyed, want)
	}
	if slices.Sort(logins); !slices.Equal(logins, []string{"ChatGPT", "Claude", "Copilot"}) {
		t.Errorf("sign-ins %q", logins)
	}
	for _, entry := range listing.Entries {
		if icons.Brand(entry.Preset.ID) == nil || icons.Brand(presetBrand(entry)) == nil {
			t.Errorf("preset %q of brand %q has no mark", entry.Preset.ID, presetBrand(entry))
		}
	}
	if got := methods["openai"]; !slices.Equal(got, []string{"ChatGPT OAuth", "OpenAI", "OpenAI Compatible"}) {
		t.Errorf("OpenAI's methods %q", got)
	}
	node := snapshot.Node.String()
	for id, want := range map[string][4]string{
		"builtin:chat-gpt-oauth": {providers.DefaultCatalogVersion, "Sailry/0.1"},
		"builtin:claude-oauth":   {1: providers.DefaultClaudeAgent},
		"builtin:copilot":        {1: providers.DefaultCopilotAgent, 2: providers.DefaultCopilotEditor, 3: providers.DefaultCopilotEditorPlugin},
		"builtin:openai":         {},
	} {
		if got, ok := oauth[node+"/"+id]; !ok || got != want {
			t.Errorf("method %q starts its advanced settings on %q (listed %v), want %q", id, got, ok, want)
		}
	}
	if len(m.Channels) != len(snapshot.Providers) {
		t.Fatalf("projected %d channels, want %d", len(m.Channels), len(snapshot.Providers))
	}
	for i, want := range [][3]string{{"openai", "OpenAI", "openai"}, {"anthropic", "Anthropic", "claude-oauth"}, {"gemini", "Gemini", "gemini"}, {"zhipu", "Zhipu", "zhipu"}, {"deepseek", "DeepSeek", "deepseek"}, {"openrouter", "OpenRouter", "openrouter"}, {"openai", "OpenAI", ""}} {
		ch := m.Channels[i]
		if ch.Brand != want[0] || ch.BrandName != want[1] || ch.Icon != want[2] || icons.Brand(ch.Icon) == nil && icons.Brand(ch.Brand) == nil {
			t.Errorf("channel %q is of brand %q (%q) with mark %q", ch.Name, ch.Brand, ch.BrandName, ch.Icon)
		}
	}
	saveChannel := func(draft settings.ProviderDraft) protocol.CommandSaveProviderPayload {
		t.Helper()
		before := len(saves)
		var result *settings.ProviderSaveResult
		m.ProviderActions.Save(draft, func(r settings.ProviderSaveResult) { result = &r })
		settle()
		if result == nil || len(result.Succeeded) != 1 || len(saves) != before+1 {
			t.Fatalf("save %q: result %+v, %d requests", draft.Channel.API, result, len(saves)-before)
		}
		return saves[before]
	}
	chat := string(protocol.ConversationModelApiChatCompletions)
	route := slices.IndexFunc(compatible.Routes, func(route viewmodel.Route) bool { return route.API == chat })
	if route < 0 {
		t.Fatal("OpenAI Compatible did not project a Chat Completions route")
	}
	request := saveChannel(settings.ProviderDraft{Key: "sk-typed", Channel: viewmodel.Channel{
		ID: (protocol.ProviderID{8}).String(), Node: node, Name: "OpenAI Compatible", Method: compatible.Routes[route].ID,
		API: chat, Endpoint: "https://gateway.example/v1", Models: []viewmodel.ChannelModel{{ID: "gateway-model", Context: 128000}},
	}})
	p := request.Provider
	if p.Api != protocol.ConversationModelApiChatCompletions || p.Endpoint != "https://gateway.example/v1" || p.Authentication != protocol.AuthenticationApiKey || request.Secret == nil ||
		p.Preset == nil || p.Preset.ID != "compatible-responses" || p.Preset.Route == nil || p.Preset.Route.Protocol != "openai-chat-completions" ||
		len(p.Models) != 1 || p.Models[0].ID != "gateway-model" {
		t.Errorf("the save sent %+v with preset %+v", p, p.Preset)
	}

	azure := viewmodel.Channel{ID: (protocol.ProviderID{9}).String(), Name: "Azure", Node: node, Method: node + "/builtin:azure-openai/0", API: string(protocol.ConversationModelApiAzureOpenAi),
		Endpoint: "https://unit.openai.azure.com", APIVersion: "2025-01-01-preview", Models: []viewmodel.ChannelModel{{ID: "deployment", Context: 128000}}}
	p = saveChannel(settings.ProviderDraft{Channel: azure, Key: "key"}).Provider
	if p.Api != protocol.ConversationModelApiAzureOpenAi || p.Options == nil || p.Options.Kind != "azure_open_ai" || p.Preset == nil || p.Preset.ID != "azure-openai" {
		t.Fatalf("the Azure save sent %+v with options %+v", p, p.Options)
	}
	if err := providers.ValidateConfiguration(p); err != nil {
		t.Errorf("the Node refuses the Azure provider: %v", err)
	}
	if got := azureVersion(p); got != "2025-01-01-preview" {
		t.Errorf("a saved Azure provider shows version %q", got)
	}
	for i, cloud := range []viewmodel.Channel{
		{Method: "/builtin:bedrock/0", API: string(protocol.ConversationModelApiBedrock), Endpoint: "https://bedrock-runtime.eu-west-1.amazonaws.com", Region: "eu-west-1",
			Models: []viewmodel.ChannelModel{{ID: "eu.anthropic.claude-sonnet-4-6", Context: 1000000, Output: 64000}}},
		{Method: "/builtin:vertex-ai/0", API: string(protocol.ConversationModelApiVertexAi), Endpoint: "https://aiplatform.googleapis.com", Project: "proj-123", Location: "global",
			Models: []viewmodel.ChannelModel{{ID: "gemini-3.5-flash", Context: 1048576, Output: 65536}}},
	} {
		cloud.ID, cloud.Name, cloud.Node = protocol.ProviderID{byte(10 + i)}.String(), "Cloud", node
		cloud.Method = node + cloud.Method
		p = saveChannel(settings.ProviderDraft{Channel: cloud, Key: "credential"}).Provider
		bedrock, isBedrock := p.Options.Bedrock()
		vertex, isVertex := p.Options.VertexAi()
		if string(p.Api) != cloud.API || p.Preset == nil || p.Endpoint != cloud.Endpoint ||
			isBedrock && bedrock.Region != cloud.Region || isVertex && (vertex.Project != cloud.Project || vertex.Location != cloud.Location) || isBedrock == isVertex {
			t.Errorf("the %s save sent %+v with options %+v", cloud.API, p, p.Options)
		}
		if err := providers.ValidateConfiguration(p); err != nil {
			t.Errorf("the Node refuses the %s provider: %v", cloud.API, err)
		}
	}
}
