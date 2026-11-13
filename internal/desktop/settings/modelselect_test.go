package settings

import (
	"image/png"
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
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

	t.Run("role model", func(t *testing.T) {
		m := NewModel(preview.SettingsData())
		ch, x := channel(t, m)
		id := ch.ID + "/" + x.ID
		model, effort := "", ""
		features := map[string]string{"speed": "fast"}
		w := show(t, func(c *ui.Context) {
			modelField(c, m, "model", "Model", "role_inherit", &model, &effort, &features)
		})
		follow, auto := i18n.In("en", "role_inherit"), i18n.In("en", "role_effort_inherit")
		if trigger, _ := w.AccessByRole(ui.RoleButton, "Model"); trigger.Value != follow {
			t.Fatalf("no model reads %q, want %q", trigger.Value, follow)
		}
		click(w, ui.RoleButton, "Model")
		if w.HasText(id) || w.HasText(ch.ID) || !w.HasText(ch.Name) {
			t.Errorf("the popover names ids or no channel: %q", w.Texts())
		}
		// Another model starts from Auto and without the old one's features.
		click(w, ui.RoleOption, x.Name)
		if model != id || effort != "" || features != nil {
			t.Fatalf("chose %q at %q with %v, want %q at Auto", model, effort, features, id)
		}
		level := x.Efforts[len(x.Efforts)-1]
		click(w, ui.RoleOption, level)
		if model != id || effort != level {
			t.Fatalf("chose %q at %q, want %q at %q", model, effort, id, level)
		}
		if trigger, _ := w.AccessByRole(ui.RoleButton, "Model"); trigger.Value != x.Name+" · "+level {
			t.Errorf("the field reads %q", trigger.Value)
		}
		// An effort of the same model keeps its features.
		features = map[string]string{"speed": "fast"}
		click(w, ui.RoleButton, "Model")
		click(w, ui.RoleOption, auto)
		if model != id || effort != "" || features["speed"] != "fast" {
			t.Errorf("Auto set %q at %q with %v", model, effort, features)
		}
		click(w, ui.RoleButton, "Model")
		click(w, ui.RoleOption, follow)
		if model != "" || effort != "" || features != nil {
			t.Errorf("following set %q at %q with %v", model, effort, features)
		}
	})

	t.Run("local model", func(t *testing.T) {
		m := NewModel(preview.SettingsData())
		lm := &LocalModels{Model: -1, Models: []preview.LocalModel{
			{ID: "a", Name: "Alpha", Status: preview.ModelInstalled},
			{ID: "x", Name: "Not installed"},
			{ID: "b", Name: "Beta", Status: preview.ModelInstalled},
		}}
		lm.Enabled.Draft = true
		w := show(t, func(c *ui.Context) {
			localModels(c, m, lm, "runtime_ocr", "runtime_ocr_model", "runtime_ocr_enable", "search_driver_auto")
		})
		label, none := i18n.In("en", "runtime_ocr_model"), i18n.In("en", "search_driver_auto")
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

	// For review, with SAILRY_SNAPSHOTS set: the Media models page with a
	// row's models open, and a role's model with its efforts, at 2x under
	// .runtime/ui-shots/model-select.
	if os.Getenv("SAILRY_SNAPSHOTS") == "" {
		return
	}
	shots := filepath.Join("..", "..", "..", ".runtime", "ui-shots", "model-select")
	if err := os.MkdirAll(shots, 0o755); err != nil {
		t.Fatal(err)
	}
	for _, mode := range []theme.Mode{theme.Dark, theme.Light} {
		suffix := map[theme.Mode]string{theme.Light: "light", theme.Dark: "dark"}[mode]
		m := NewModel(preview.SettingsData())
		ch, x := channel(t, m)
		model, effort := ch.ID+"/"+x.ID, x.Efforts[len(x.Efforts)-1]
		var features map[string]string
		for name, shot := range map[string]struct {
			view func(c *ui.Context)
			open string
		}{
			"settings-media": {func(c *ui.Context) { mediaModels(c, m) }, i18n.In("en", "media_image")},
			"settings-role":  {func(c *ui.Context) { modelField(c, m, "model", "Model", "role_inherit", &model, &effort, &features) }, "Model"},
		} {
			tt := ui.NewTesterWith(func(c *ui.Context) {
				theme.Apply(c, "", mode)
				ui.Column(c).Fill().Padding(24).Background(theme.Of(c).Background).Children(func() { shot.view(c) })
			}, 760, 560, ui.TesterOptions{Dark: mode == theme.Dark, Preferences: ui.Preferences{ReduceTransparency: true}})
			tt.SetLocale("en")
			tt.SetScale(2)
			tt.Frame()
			click(window{tt, t}, ui.RoleButton, shot.open)
			tt.Frame()
			out, err := os.Create(filepath.Join(shots, name+"-"+suffix+".png"))
			if err != nil {
				t.Fatal(err)
			}
			if err := png.Encode(out, tt.Image()); err != nil {
				t.Fatal(err)
			}
			out.Close()
		}
	}
}
