package desktop

import (
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
)

func TestMemoryEditorUsesLoadedRevisionAndIgnoresRetiredCallbacks(t *testing.T) {
	assembly := shell.Assemble(Register)
	catalog := i18n.New(assembly.Locales...)
	en := func(key string) string { return catalog.In("en", key) }
	s := settings.NewLive()
	s.SetNativePages(assembly.SettingsPages)
	s.Open(settings.Memory)
	entry := settings.MemoryEntryItem{ID: "entry", Revision: 3, Title: "Initial", Kind: "user", Project: "p"}
	s.MemorySettings = settings.MemorySettingsState{Loaded: true, Connected: true, Entries: []settings.MemoryEntryItem{entry},
		Projects: []settings.MemoryProjectOption{{ID: "p", Name: "Project"}}}
	var loads []func(settings.MemoryEntryItem, string, string)
	var drafts []settings.MemorySaveDraft
	var finish func(bool, bool, string)
	s.MemoryActions = &settings.MemoryActions{
		LoadEntry: func(id string, done func(settings.MemoryEntryItem, string, string)) {
			if id != entry.ID {
				t.Fatalf("loaded the wrong entry: %q", id)
			}
			loads = append(loads, done)
		},
		SaveEntry: func(draft settings.MemorySaveDraft, done func(bool, bool, string)) {
			drafts, finish = append(drafts, draft), done
		},
	}
	view := ui.NewTesterWith(func(c *ui.Context) {
		i18n.Bind(c, catalog)
		settings.Page(c, s, nil, settings.Look{})
	}, 960, 850, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true, ReduceMotion: true}})
	view.SetLocale("en")
	view.Frame()
	click := func(label string) {
		t.Helper()
		x, ok := view.AccessByRole(ui.RoleButton, en(label))
		if !ok || x.Disabled {
			t.Fatalf("no enabled %q button: %q", label, view.Texts())
		}
		view.ClickAt(x.Bounds.X+x.Bounds.W/2, x.Bounds.Y+x.Bounds.H/2)
		view.Frame()
	}
	click("settings_edit")
	if len(loads) != 1 {
		t.Fatalf("the editor started %d reads", len(loads))
	}
	click("settings_cancel")
	click("settings_edit")
	if len(loads) != 2 {
		t.Fatalf("reopening the editor started %d reads", len(loads))
	}
	loaded := entry
	loaded.Revision, loaded.Title = 4, "Current"
	loads[1](loaded, "Current body", "")
	loads[0](entry, "Outdated body", "")
	view.Frame()
	click("settings_save")
	if len(drafts) != 1 || drafts[0].ID != loaded.ID || drafts[0].ExpectedRevision != loaded.Revision || drafts[0].Project != "p" ||
		drafts[0].Title != loaded.Title || drafts[0].Body != "Current body" {
		t.Fatalf("the form saved a stale entry: %+v", drafts)
	}
	finish(false, false, "memory_failed")
	view.Frame()
	click("settings_save")
	if len(drafts) != 2 || drafts[1].Title != drafts[0].Title || drafts[1].Body != drafts[0].Body || drafts[1].ExpectedRevision != drafts[0].ExpectedRevision {
		t.Fatalf("a failed save discarded the draft: %+v", drafts)
	}
	finish(true, false, "")
	view.Frame()
	if _, open := view.AccessByRole(ui.RoleButton, en("settings_save")); open {
		t.Fatal("successful save did not close the editor")
	}
}
