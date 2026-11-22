package desktop

import (
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
)

var testCatalog = i18n.New(Locales)

func TestSkillPageAndSource(t *testing.T) {
	e := skillInstall{source: 1, link: "https://example.com/x"}
	if e.Check() != "skill_source_link_invalid" {
		t.Error("skills.sh link passed")
	}
	s := settings.New()
	s.SetNativePages([]settings.NativePage{Page()})
	s.ShowDetails(settings.DetailPlugin, "git")
	s.Open(settings.Skills)
	if s.Section != settings.Skills || s.Details != nil {
		t.Fatal("opening Skills did not replace the current page and close details")
	}
}

func TestSkillInstallCompletionAfterNodeChange(t *testing.T) {
	s := settings.NewLive()
	s.SetNativePages([]settings.NativePage{Page()})
	s.Open(settings.Skills)
	var complete func(bool)
	var draft settings.SkillSourceDraft
	s.PackageActions = &settings.PackageActions{InstallSkill: func(value settings.SkillSourceDraft, done func(bool)) {
		draft, complete = value, done
	}}
	tester := ui.NewTesterWith(func(c *ui.Context) {
		i18n.Bind(c, testCatalog)
		settings.Page(c, s, nil, settings.Look{})
	}, 960, 800, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true}})
	tester.SetLocale("en")
	press := func(key string) {
		t.Helper()
		if err := tester.Click(testCatalog.In("en", key)); err != nil {
			t.Fatal(err)
		}
		tester.Frame()
		tester.Advance(time.Second)
	}
	tester.Frame()
	p := pageFor(s.Model)
	press("skills_install")
	p.install.repository = "https://example.test/skills.git"
	tester.Frame()
	press("plugins_install")
	if complete == nil || draft.Repository != p.install.repository || !p.install.pending {
		t.Fatal("the Skills dialog did not submit its repository")
	}
	s.CloseManagedPackageDialogs()
	tester.Frame()
	tester.Advance(time.Second)
	press("skills_install")
	complete(true)
	if !p.install.open || p.install.repository != "" {
		t.Fatal("the previous Node's install affected the new dialog")
	}
}
