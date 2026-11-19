package settings

import (
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
)

func TestSkillDetailsShowFailureAndRetry(t *testing.T) {
	s := NewLive()
	s.MarketSkills = []preview.Skill{{ID: "office", Name: "Office MCP", Description: "Work with documents", Origin: "example/skills"}}
	var finishes []func(string)
	s.PackageActions = &PackageActions{LoadDetails: func(kind DetailKind, id string, done func(string)) {
		if kind != DetailSkill || id != "office" {
			t.Fatalf("unexpected details request %v for %q", kind, id)
		}
		finishes = append(finishes, done)
	}}
	s.ShowDetails(DetailSkill, "office")
	s.ShowDetails(DetailSkill, "office")
	const reason = "The skill repository is unavailable"
	finishes[1](reason)
	finishes[0]("The old request failed")
	tt := ui.NewTesterWith(func(c *ui.Context) { DetailsPanel(c, s) }, 520, 800, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true}})
	tt.SetLocale("en")
	tt.Frame()
	for _, want := range []string{"Office MCP", "Work with documents", "example/skills", reason} {
		if !tt.HasText(want) {
			t.Fatalf("the failed details lack %q: %q", want, tt.Texts())
		}
	}
	if tt.HasText("The old request failed") || s.Details.Load.Loaded {
		t.Fatalf("the failed details settled incorrectly: %+v", s.Details)
	}
	retry := i18n.In("en", "settings_retry")
	clickButton(t, tt, retry)
	if len(finishes) != 3 || !s.Details.Load.Loading || tt.HasText(reason) {
		t.Fatalf("retry did not load the skill again: %d requests, %+v", len(finishes), s.Details)
	}
	s.MarketSkills[0].Content = "The document is now available"
	finishes[2]("")
	tt.Frame()
	if !s.Details.Load.Loaded || !tt.HasText(s.MarketSkills[0].Content) || tt.HasText(reason) {
		t.Fatalf("retry did not reveal the document: %q", tt.Texts())
	}
}
