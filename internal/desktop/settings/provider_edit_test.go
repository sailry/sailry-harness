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
	m.ProviderActions.ReadKey = func(_ string, done func(string)) { read = done }
	var saved []ProviderDraft
	m.ProviderActions.Save = func(draft ProviderDraft, done func(bool)) {
		saved = append(saved, draft)
		done(false)
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
	m.ProviderActions.ReadKey = func(_ string, done func(string)) { reads = append(reads, done) }
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
	m.ProviderActions.Save = func(draft ProviderDraft, done func(bool)) {
		if draft.Channel.Shared {
			t.Error("editor retained obsolete host sharing")
		}
		done(false)
	}
	m.provider.edit(m, &ch)
	if problem := m.provider.save(m); problem != "" {
		t.Fatal(problem)
	}
}

func TestFollowedProviderEditorDoesNotReadCopiedKey(t *testing.T) {
	m := NewModel(preview.SettingsData())
	m.Live = true
	m.ProviderActions.ReadKey = func(string, func(string)) { t.Fatal("readonly copy attempted key read") }
	m.provider.edit(m, &preview.Channel{ID: "copy", Auth: preview.AuthKey, ConfigurationReadOnly: true, Shared: true})
}
