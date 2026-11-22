package desktop

import (
	"bytes"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
)

var testCatalog = i18n.New(Locales)

// Team and role checks.
func TestTeams(t *testing.T) {
	m := pageFor(settings.New().Model)
	if got := m.saveTeam(viewmodel.Team{Name: " "}); got != "team_invalid" {
		t.Errorf("blank team: %q", got)
	}
	if got := m.saveTeam(viewmodel.Team{Name: "study"}); got != "team_name_taken" {
		t.Errorf("taken name: %q", got)
	}
	n := len(m.Teams)
	if got := m.saveTeam(viewmodel.Team{Name: "Docs"}); got != "" || len(m.Teams) != n+1 {
		t.Errorf("new team: %q", got)
	}
	if got := m.saveRole(viewmodel.Role{Name: "X", Key: "Bad Key"}); got != "teams_role_invalid" {
		t.Errorf("bad key: %q", got)
	}
	if got := m.saveRole(viewmodel.Role{Name: "X", Key: "tester"}); got != "role_duplicate" {
		t.Errorf("duplicate key: %q", got)
	}
	// A model's features each have a select after the effort: Auto follows,
	// Off sets none, an option sets it; another model starts from Auto.
	t.Run("model features", func(t *testing.T) {
		m := pageFor(settings.New().Model)
		var ch *viewmodel.Channel
		for i := range m.Channels {
			if m.Channels[i].Enabled && len(m.Channels[i].Models) > 1 {
				ch = &m.Channels[i]
				break
			}
		}
		if ch == nil {
			t.Fatal("the preview has no enabled channel with two models")
		}
		ch.Models[0].Features = []viewmodel.ModelFeature{{ID: "speed", Name: "Speed", Options: []viewmodel.FeatureOption{{ID: "fast", Name: "fast"}}}}
		model, effort := ch.ID+"/"+ch.Models[0].ID, ""
		var features map[string]string
		tt := ui.NewTesterWith(func(c *ui.Context) {
			i18n.Bind(c, testCatalog)
			modelField(c, m, "model", "Model", "role_inherit", &model, &effort, &features)
		}, 900, 600, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true}})
		tt.SetLocale("en")
		choose := func(option string) {
			t.Helper()
			tt.Frame()
			var trigger ui.AccessInfo
			for _, x := range tt.AccessAll() {
				if x.Name == "Speed" && x.Bounds.H > trigger.Bounds.H {
					trigger = x
				}
			}
			tt.ClickAt(trigger.Bounds.X+trigger.Bounds.W/2, trigger.Bounds.Y+trigger.Bounds.H/2)
			tt.Frame()
			tt.Advance(time.Second)
			for _, x := range tt.AccessAll() {
				if x.Name == option && x.Bounds.Y > trigger.Bounds.Y+trigger.Bounds.H/2 {
					tt.ClickAt(x.Bounds.X+x.Bounds.W/2, x.Bounds.Y+x.Bounds.H/2)
					tt.Frame()
					tt.Advance(time.Second)
					return
				}
			}
			t.Fatalf("no %q option: %q", option, tt.Texts())
		}
		choose(testCatalog.In("en", "composer_feature_fast"))
		if features["speed"] != "fast" {
			t.Fatalf("Fast set %v", features)
		}
		choose(testCatalog.In("en", "composer_feature_off"))
		if features == nil || len(features) != 0 {
			t.Fatalf("Off set %v", features)
		}
		choose(testCatalog.In("en", "role_effort_inherit"))
		if features != nil {
			t.Fatalf("Auto set %v", features)
		}
		features = map[string]string{"speed": "fast"}
		model = ch.ID + "/" + ch.Models[1].ID
		tt.Frame()
		for _, x := range tt.AccessAll() {
			if x.Name == "Speed" {
				t.Error("a model without features offers one")
			}
		}
	})
	// A team's deletion closes the editor it was deleted from, not one
	// opened since for another team.
	t.Run("late removal", func(t *testing.T) {
		m := pageFor(settings.New().Model)
		m.Live = true
		var done func(kit.Outcome)
		m.TeamActions = &settings.TeamActions{RemoveTeam: func(_ viewmodel.Team, completed func(kit.Outcome)) { done = completed }}
		removed, other := m.Teams[0], m.Teams[1]
		m.teamEdit, m.teamRemoving = teamEditor{open: true, team: removed}, removed.ID
		tt := ui.NewTesterWith(func(c *ui.Context) {
			i18n.Bind(c, testCatalog)
			m.render(c)
		}, 1100, 900, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true}})
		tt.SetLocale("en")
		tt.Frame()
		tt.Advance(time.Second)
		var confirms []ui.Rect
		for _, x := range tt.AccessAll() {
			if x.Role == ui.RoleButton && x.Name == testCatalog.In("en", "settings_delete") {
				confirms = append(confirms, x.Bounds)
			}
		}
		if len(confirms) == 0 {
			t.Fatalf("no confirmation: %q", tt.Texts())
		}
		at := confirms[len(confirms)-1]
		tt.ClickAt(at.X+at.W/2, at.Y+at.H/2)
		tt.Frame()
		if done == nil {
			t.Fatal("the confirmation removed nothing")
		}
		m.teamEdit, m.teamRemoving = teamEditor{open: true, team: other}, other.ID
		if done(kit.Done); !m.teamEdit.open || m.teamRemoving != other.ID {
			t.Errorf("another team's editor %v, confirmation %q", m.teamEdit.open, m.teamRemoving)
		}
		m.teamEdit = teamEditor{open: true, team: removed}
		if done(kit.Done); m.teamEdit.open {
			t.Error("the deleted team's editor stayed open")
		}
		m.tab = 1
		m.CloseManagedNodeDialogs()
		if m.tab != 1 || m.teamRemoving != "" {
			t.Errorf("switching Nodes changed the tab to %d or left %q open", m.tab, m.teamRemoving)
		}
		m.teamEdit, m.teamRemoving = teamEditor{open: true, team: removed}, removed.ID
		if done(kit.Done); !m.teamEdit.open || m.teamRemoving != removed.ID {
			t.Errorf("old Node's removal affected the new Node's editor %v, confirmation %q", m.teamEdit.open, m.teamRemoving)
		}
	})
}

func TestDefaultMemberRowIsStatic(t *testing.T) {
	m := pageFor(settings.New().Model)
	role := m.Roles[0]
	m.teamEdit = teamEditor{open: true, def: true, team: viewmodel.Team{Name: "Default", Members: []string{role.ID}}}
	tt := ui.NewTesterWith(func(c *ui.Context) {
		i18n.Bind(c, testCatalog)
		teamDialog(c, m)
	}, 1100, 900, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true}})
	tt.SetLocale("en")
	tt.Frame()
	tt.Advance(time.Second)
	for _, x := range tt.AccessAll() {
		if x.Role == ui.RoleButton && x.Name == role.Name {
			t.Errorf("the Default team's member row is a button: %+v", x)
		}
	}
	box, ok := tt.Find(role.Name)
	if !ok {
		t.Fatalf("no member row: %q", tt.Texts())
	}
	tt.Move(2, 890)
	tt.Frame()
	idle := bytes.Clone(tt.Image().Pix)
	tt.Move(box.X+box.W+40, box.Y+box.H/2)
	tt.Frame()
	tt.Advance(time.Second)
	if !bytes.Equal(idle, tt.Image().Pix) {
		t.Error("a member row with no action changed under the pointer")
	}
}
