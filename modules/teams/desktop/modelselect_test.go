package desktop

import (
	"image/png"
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
)

func TestRoleModelField(t *testing.T) {
	type window struct {
		*ui.Tester
		t *testing.T
	}
	show := func(t *testing.T, view func(c *ui.Context)) window {
		tt := ui.NewTesterWith(func(c *ui.Context) {
			i18n.Bind(c, testCatalog)
			view(c)
		}, 900, 700, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true}})
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
	channel := func(t *testing.T, m *page) (*viewmodel.Channel, viewmodel.ChannelModel) {
		for i := range m.TeamChannels {
			for _, x := range m.TeamChannels[i].Models {
				if m.TeamChannels[i].Enabled && len(x.Efforts) > 1 && x.Name != "" && x.Name != x.ID {
					return &m.TeamChannels[i], x
				}
			}
		}
		t.Fatal("the preview has no enabled channel whose named model has efforts")
		return nil, viewmodel.ChannelModel{}
	}

	m := pageFor(settings.New().Model)
	m.Channels = nil // Provider settings are unmounted while editing a role.
	ch, x := channel(t, m)
	id := ch.ID + "/" + x.ID
	model, effort := "", ""
	features := map[string]string{"speed": "fast"}
	w := show(t, func(c *ui.Context) {
		modelField(c, m, "model", "Model", "role_inherit", &model, &effort, &features)
	})
	follow, auto := testCatalog.In("en", "role_inherit"), testCatalog.In("en", "role_effort_inherit")
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

	if os.Getenv("SAILRY_SNAPSHOTS") == "" {
		return
	}
	shots := filepath.Join("..", "..", "..", ".runtime", "ui-shots", "model-select")
	if err := os.MkdirAll(shots, 0o755); err != nil {
		t.Fatal(err)
	}
	for _, mode := range []theme.Mode{theme.Dark, theme.Light} {
		suffix := map[theme.Mode]string{theme.Light: "light", theme.Dark: "dark"}[mode]
		model, effort := id, x.Efforts[len(x.Efforts)-1]
		var features map[string]string
		tt := ui.NewTesterWith(func(c *ui.Context) {
			i18n.Bind(c, testCatalog)
			theme.Apply(c, "", mode)
			ui.Column(c).Fill().Padding(24).Background(theme.Of(c).Background).Children(func() {
				modelField(c, m, "model", "Model", "role_inherit", &model, &effort, &features)
			})
		}, 760, 560, ui.TesterOptions{Dark: mode == theme.Dark, Preferences: ui.Preferences{ReduceTransparency: true}})
		tt.SetLocale("en")
		tt.SetScale(2)
		tt.Frame()
		click(window{tt, t}, ui.RoleButton, "Model")
		tt.Frame()
		out, err := os.Create(filepath.Join(shots, "settings-role-"+suffix+".png"))
		if err != nil {
			t.Fatal(err)
		}
		if err := png.Encode(out, tt.Image()); err != nil {
			t.Fatal(err)
		}
		out.Close()
	}
}
