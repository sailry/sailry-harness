package settings

import (
	"os"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
)

// The settings pages' model fields are kit.ModelSelect: they name a model
// as its channel does, and store what their selects stored.
func TestModelFields(t *testing.T) {
	type window struct {
		*ui.Tester
		t *testing.T
	}
	show := func(t *testing.T, view func(c *ui.Context)) window {
		tt := ui.NewTesterWith(view, 900, 700, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true}})
		tt.SetLocale("en")
		tt.Frame()
		return window{tt, t}
	}
	click := func(w window, role ui.Role, name string) {
		w.t.Helper()
		var found []ui.AccessInfo
		for _, x := range w.AccessAll() {
			if x.Role == role && x.Name == name && x.Bounds.W > 0 {
				found = append(found, x)
			}
		}
		if len(found) != 1 {
			w.t.Fatalf("%d of %q: %q", len(found), name, w.Texts())
		}
		w.ClickAt(found[0].Bounds.X+found[0].Bounds.W/2, found[0].Bounds.Y+found[0].Bounds.H/2)
		w.Frame()
		w.Advance(time.Second)
	}
	// channel is the preview's first enabled channel having a model with
	// efforts, and that model.
	channel := func(t *testing.T, m *Model) (*preview.Channel, preview.ChannelModel) {
		for i := range m.Channels {
			for _, x := range m.Channels[i].Models {
				if m.Channels[i].Enabled && len(x.Efforts) > 1 && x.Name != "" && x.Name != x.ID {
					return &m.Channels[i], x
				}
			}
		}
		t.Fatal("the preview has no enabled channel whose named model has efforts")
		return nil, preview.ChannelModel{}
	}

	t.Run("local model", func(t *testing.T) {
		m := NewModel(preview.SettingsData())
		lm := &LocalModels{Model: -1, Models: []preview.LocalModel{
			{ID: "a", Name: "Alpha", Status: preview.ModelInstalled},
			{ID: "x", Name: "Not installed"},
			{ID: "b", Name: "Beta", Status: preview.ModelInstalled},
		}}
		lm.Enabled.Draft = true
		catalog := i18n.New(os.DirFS("../../../modules/ocr/desktop"))
		w := show(t, func(c *ui.Context) {
			i18n.Bind(c, catalog)
			LocalModelLibrary(c, m, lm, "runtime_ocr", "runtime_ocr_model", "runtime_ocr_enable", "search_driver_auto")
		})
		label, none := catalog.In("en", "runtime_ocr_model"), catalog.In("en", "search_driver_auto")
		if trigger, _ := w.AccessByRole(ui.RoleButton, label); trigger.Value != none {
			t.Fatalf("no model reads %q, want %q", trigger.Value, none)
		}
		click(w, ui.RoleButton, label)
		if _, listed := w.AccessByRole(ui.RoleOption, "Not installed"); listed {
			t.Error("a model that is not installed is offered")
		}
		// The place among the installed models is what the page stores.
		click(w, ui.RoleOption, "Beta")
		if lm.Model != 1 {
			t.Fatalf("Beta stored %d, want 1", lm.Model)
		}
		if trigger, _ := w.AccessByRole(ui.RoleButton, label); trigger.Value != "Beta" {
			t.Errorf("the field reads %q", trigger.Value)
		}
		click(w, ui.RoleButton, label)
		click(w, ui.RoleOption, none)
		if lm.Model != -1 {
			t.Errorf("%s stored %d, want -1", none, lm.Model)
		}
		// A chosen model the host lost stays in the field under its name.
		lm.Model, lm.Missing = 2, "Gamma"
		w.Frame()
		if trigger, _ := w.AccessByRole(ui.RoleButton, label); trigger.Value != "Gamma" {
			t.Errorf("the missing model reads %q", trigger.Value)
		}
		lm.Enabled.Draft = false
		w.Frame()
		if trigger, _ := w.AccessByRole(ui.RoleButton, label); !trigger.Disabled {
			t.Error("the select of a disabled kind is enabled")
		}
	})

	t.Run("package model", func(t *testing.T) {
		m := NewModel(preview.SettingsData())
		ch, x := channel(t, m)
		id, unknown := ch.ID+"/"+x.ID, "0d6f1c1e-7a52-4f0e-9a57-3f1f3c1f0a11/custom-model"
		m.openConfig(m.pkg("figma"))
		m.config.fields = []preview.Setting{{Name: "model", Title: "Summary model", Kind: "model", Options: []string{id, unknown}, Required: true}}
		w := show(t, func(c *ui.Context) { PackageConfigurationDialog(c, m) })
		w.Advance(time.Second)
		if !w.HasText(i18n.In("en", "composer_select_model")) || m.config.Check() != "plugins_settings_required" {
			t.Fatalf("an unset model field: %q, check %q", w.Texts(), m.config.Check())
		}
		click(w, ui.RoleButton, "Summary model")
		if w.HasText(id) || w.HasText(unknown) || !w.HasText("custom-model") {
			t.Errorf("the popover names ids: %q", w.Texts())
		}
		for _, level := range x.Efforts {
			if _, listed := w.AccessByRole(ui.RoleOption, level); listed {
				t.Errorf("a field that stores a model offers the effort %q", level)
			}
		}
		click(w, ui.RoleOption, x.Name)
		if field := m.config.fields[0]; field.Value != id || m.config.Check() != "" {
			t.Fatalf("stored %q, check %q, want %q", field.Value, m.config.Check(), id)
		}
		click(w, ui.RoleButton, "Summary model")
		click(w, ui.RoleOption, "custom-model")
		if field := m.config.fields[0]; field.Value != unknown {
			t.Errorf("stored %q, want %q", field.Value, unknown)
		}
	})
}
