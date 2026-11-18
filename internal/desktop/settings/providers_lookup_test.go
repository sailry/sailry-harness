package settings

import (
	"slices"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
)

func TestProviderModelCapabilityLookup(t *testing.T) {
	m := NewModel(preview.SettingsData())
	m.Live, m.Catalog.Bundled = true, true
	e := &m.provider
	e.open, e.owner, e.id, e.revision = true, "model-host", "channel", 7
	e.configure(preview.Method{ID: "compatible", Name: "Compatible", Keyed: true, Routes: []preview.Route{
		{ID: "compatible/0", API: "responses", Endpoint: "https://fixture.example/v1", Custom: true},
	}})
	type call struct {
		draft ProviderDraft
		id    string
		reply func(*viewmodel.ChannelModel)
	}
	var calls []call
	m.ProviderActions.LookupModel = func(draft ProviderDraft, id string, reply func(*viewmodel.ChannelModel)) {
		calls = append(calls, call{draft: draft, id: id, reply: reply})
	}
	tt := ui.NewTesterWith(func(c *ui.Context) {
		theme.Apply(c, "", theme.Light)
		providerEditorDialog(c, m)
	}, 1000, 1100, ui.TesterOptions{Preferences: ui.Preferences{ReduceMotion: true}})
	tt.SetLocale("en")
	tt.Frame()
	tt.Advance(time.Second)
	if err := tt.Click(i18n.In("en", "provider_add_model")); err != nil {
		t.Fatal(err)
	}
	tt.Frame()
	field, ok := tt.AccessByRole(ui.RoleTextField, i18n.In("en", "provider_model_id"))
	if !ok || len(e.models) != 1 {
		t.Fatal("new model did not open its ID field")
	}
	row := &e.models[0]
	row.context, row.efforts = "172000", []string{"high"}
	tt.ClickAt(field.Bounds.X+field.Bounds.W/2, field.Bounds.Y+field.Bounds.H/2)
	tt.Type("fixture-")
	tt.Frame()
	tt.Advance(modelCapabilityLookupDelay / 2)
	tt.Type("model")
	tt.Frame()
	tt.Advance(modelCapabilityLookupDelay / 2)
	if len(calls) != 0 {
		t.Fatal("lookup used an ID still being typed")
	}
	settle := func() {
		tt.Frame()
		tt.Advance(modelCapabilityLookupDelay + time.Millisecond)
		tt.Frame()
	}
	settle()
	if len(calls) != 1 || calls[0].id != "fixture-model" {
		t.Fatalf("lookup calls = %+v", calls)
	}
	request := calls[0].draft
	if request.Channel.Node != "model-host" || request.Channel.ID != "channel" ||
		request.Channel.Method != "compatible/0" || request.Channel.API != "responses" || request.Channel.Endpoint != "https://fixture.example/v1" ||
		request.ExpectedRevision != 7 || request.Key != "" {
		t.Fatalf("lookup lost the channel's scope: %+v", request)
	}
	calls[0].reply(&viewmodel.ChannelModel{ID: "fixture-model", Reasoning: true, Context: 32000, Efforts: []string{"low"}, Generates: []string{viewmodel.GenerateImageTool}})
	settle()
	if len(calls) != 1 || row.model.Vision || row.model.Tools || !row.model.Reasoning || row.model.CustomCapabilities ||
		!row.hosted || row.catalog == nil || !slices.Equal(row.model.Generates, []string{viewmodel.GenerateImageTool}) ||
		row.context != "172000" || !slices.Equal(row.efforts, []string{"high"}) {
		t.Fatalf("automatic lookup changed choices beyond capability tags: %+v", row)
	}
	row.id = "old"
	settle()
	if len(calls) != 2 || calls[1].id != "old" {
		t.Fatalf("lookup did not follow the changed ID: %+v", calls)
	}
	row.id = "replacement"
	calls[1].reply(&viewmodel.ChannelModel{ID: "old", Vision: false})
	if !row.model.Vision || row.catalog != nil {
		t.Fatal("an old reply changed the replacement model")
	}
	settle()
	if len(calls) != 3 || calls[2].id != "replacement" {
		t.Fatalf("lookup did not follow the replacement ID: %+v", calls)
	}
	box, ok := tt.AccessByRole(ui.RoleCheckBox, i18n.In("en", "provider_cap_understands")+" "+i18n.In("en", "media_image"))
	if !ok || !box.Checked {
		t.Fatal("model image control was unavailable before a manual override")
	}
	tt.ClickAt(box.Bounds.X+box.Bounds.H/2, box.Bounds.Y+box.Bounds.H/2)
	tt.Frame()
	calls[2].reply(&viewmodel.ChannelModel{ID: "replacement", Vision: true, Generates: []string{viewmodel.GenerateSpeech}})
	settle()
	if !row.model.CustomCapabilities || row.model.Vision || len(row.model.Generates) != 0 || len(calls) != 3 {
		t.Fatal("an automatic result replaced manually selected capability tags")
	}
}

func TestProviderModelCapabilityLookupRestoresSavedID(t *testing.T) {
	m := NewModel(preview.SettingsData())
	m.Live = true
	e := &m.provider
	e.open, e.step = true, stepConfigure
	e.models = draftsOf([]viewmodel.ChannelModel{{ID: "saved", Reasoning: true, Generates: []string{viewmodel.GenerateVideo}}})
	var reply func(*viewmodel.ChannelModel)
	m.ProviderActions.LookupModel = func(_ ProviderDraft, id string, done func(*viewmodel.ChannelModel)) {
		if id != "replacement" || reply != nil {
			t.Fatalf("unexpected lookup for %q", id)
		}
		reply = done
	}
	row := &e.models[0]
	tt := ui.NewTester(func(c *ui.Context) { e.lookupModelCapabilities(c, m, row) }, 400, 300)
	tt.Frame()
	row.id = "replacement"
	tt.Frame()
	tt.Advance(modelCapabilityLookupDelay + time.Millisecond)
	tt.Frame()
	if reply == nil {
		t.Fatal("replacement ID did not look up its capabilities")
	}
	reply(&viewmodel.ChannelModel{ID: "replacement", Vision: true, Tools: true})
	if !row.model.Vision || row.model.Reasoning {
		t.Fatal("replacement model did not apply its own capabilities")
	}
	row.id = "saved"
	tt.Frame()
	if row.lookup != nil || row.model.Vision || row.model.Tools || !row.model.Reasoning || !slices.Equal(row.model.Generates, []string{viewmodel.GenerateVideo}) {
		t.Fatal("the saved ID did not recover its original capabilities")
	}
}
