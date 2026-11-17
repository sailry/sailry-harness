package settings

import (
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
)

func TestAddProviderRequiresUsableMetadata(t *testing.T) {
	for _, test := range []struct {
		name    string
		catalog viewmodel.Catalog
		allowed bool
	}{
		{"missing metadata", viewmodel.Catalog{}, false},
		{"scores only", viewmodel.Catalog{Scores: true}, false},
		{"bundled metadata", viewmodel.Catalog{Bundled: true}, true},
		{"downloaded metadata", viewmodel.Catalog{Updated: true}, true},
	} {
		for _, welcome := range []bool{false, true} {
			name := test.name
			if welcome {
				name += " in welcome picker"
			}
			t.Run(name, func(t *testing.T) {
				m := NewModel(preview.SettingsData())
				m.Live, m.Catalog = true, test.catalog
				m.Brands = []preview.Brand{{ID: "fixture", Name: "Fixture", Methods: []preview.Method{{ID: "fixture", Keyed: true, API: "responses"}}}}
				created, chosen := 0, false
				m.ProviderActions.NewID = func() string { created++; return "new-provider" }
				s := &State{Model: m, Section: Providers}
				var shown string
				tester := ui.NewTesterWith(func(c *ui.Context) {
					theme.Apply(c, "", theme.Dark)
					if welcome {
						ProviderPicker(c, s, func() { chosen = true }, nil)
					} else {
						providers(c, s)
					}
					ui.ToastViewportBase(c, func(_ ui.Element, toasts []ui.Toast) {
						if len(toasts) > 0 {
							shown = toasts[len(toasts)-1].Title
						}
					})
				}, 1100, 900, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true, ReduceMotion: true}})
				tester.SetLocale("en")
				tester.Frame()
				label := i18n.In("en", "settings_add")
				if welcome {
					label = "Fixture"
				}
				if err := tester.Click(label); err != nil {
					t.Fatal(err)
				}
				tester.Frame()
				if test.allowed {
					if !m.provider.open || created != 1 || welcome && !chosen || shown != "" {
						t.Fatalf("usable metadata blocked adding a channel: open=%v created=%d chosen=%v toast=%q", m.provider.open, created, chosen, shown)
					}
				} else if m.provider.open || created != 0 || chosen || shown != i18n.In("en", "provider_catalog_required") {
					t.Fatalf("missing metadata did not stop the operation with a prompt: open=%v created=%d chosen=%v toast=%q", m.provider.open, created, chosen, shown)
				}
			})
		}
	}
}

func TestMissingProviderMetadataKeepsEditorDraft(t *testing.T) {
	m := NewModel(preview.SettingsData())
	m.Live, m.Catalog = true, viewmodel.Catalog{}
	m.Brands = []preview.Brand{{ID: "fixture", Methods: []preview.Method{{ID: "fixture", API: "responses", Custom: true, Keyed: true}}}}
	channel := preview.Channel{ID: "saved", Revision: 3, Name: "Saved", Method: "fixture", API: "responses", Auth: preview.AuthKey, KeySet: true,
		Endpoint: "https://fixture.example/v1", Models: []preview.ChannelModel{{ID: "fixture-model", Name: "Fixture model", Context: 128000, Output: 16384, DefaultEffort: "default"}}}
	m.ProviderActions.Discover = func(ProviderDraft, func([]viewmodel.ChannelModel)) {
		t.Fatal("discovery sent a request without usable metadata")
	}
	saved := false
	m.ProviderActions.Save = func(draft ProviderDraft, done func(bool)) { saved = true; done(false) }
	e := &m.provider
	e.edit(m, &channel)
	e.name = "Keep this draft"
	var shown string
	tester := ui.NewTesterWith(func(c *ui.Context) {
		theme.Apply(c, "", theme.Dark)
		providerEditorDialog(c, m)
		ui.ToastViewportBase(c, func(_ ui.Element, toasts []ui.Toast) {
			if len(toasts) > 0 {
				shown = toasts[len(toasts)-1].Title
			}
		})
	}, 1100, 1100, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true, ReduceMotion: true}})
	tester.SetLocale("en")
	tester.Frame()
	tester.Advance(time.Second)
	tester.Frame()
	click := func(key string) {
		t.Helper()
		if err := tester.Click(i18n.In("en", key)); err != nil {
			t.Fatal(err)
		}
		tester.Frame()
	}
	for _, action := range []string{"provider_discover", "provider_add_model"} {
		click(action)
		if shown != i18n.In("en", "provider_catalog_required") || e.probing || e.name != "Keep this draft" || len(e.models) != 1 || e.models[0].id != "fixture-model" {
			t.Fatalf("blocked %s replaced the draft or left it busy: %+v, toast=%q", action, e, shown)
		}
	}
	click("provider_save")
	if !saved || e.name != "Keep this draft" || !e.open {
		t.Fatal("missing metadata prevented an unrelated save or dropped a failed-save draft")
	}
	click("settings_delete")
	if len(e.models) != 0 {
		t.Fatal("missing metadata prevented deleting a model")
	}
}

func TestBundledProviderCatalogLabel(t *testing.T) {
	m := NewModel(preview.SettingsData())
	m.Catalog = viewmodel.Catalog{Bundled: true}
	tester := ui.NewTester(func(c *ui.Context) { catalog(c, m) }, 1000, 600)
	tester.SetLocale("en")
	tester.Frame()
	if !tester.HasText(i18n.In("en", "provider_catalog_bundled")) || tester.HasText(i18n.In("en", "provider_catalog_updated")) || !tester.HasText(i18n.In("en", "provider_catalog_never")) {
		t.Fatalf("bundled data was represented as a network refresh: %q", tester.Texts())
	}
}
