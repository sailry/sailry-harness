package shell

import (
	"fmt"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
)

// The working conversation's activity strip shows every capsule, their
// popovers open, and actions reaching the host only say so; an idle one
// shows only the Git badge, and a strip without capsules keeps its row.
func TestActivityStrip(t *testing.T) {
	a, tt := newTester(t)
	added, removed := 0, 0
	for _, f := range a.Data.Workspace.Changes {
		added += f.Added
		removed += f.Removed
	}
	git := []string{fmt.Sprintf("+%d", added), fmt.Sprintf("-%d", removed)}
	running := ": " + i18n.In("en", "chat_running")
	terminals := i18n.In("en", "background_count", "count", 2)
	want := append([]string{"Goal", "Running", i18n.In("en", "composer_todo_count", "completed", 1, "total", 3),
		terminals, "5173", i18n.In("en", "composer_subagent_one")}, git...)
	for _, s := range want {
		if !tt.HasText(s) {
			t.Fatalf("the strip does not show %q: %q", s, tt.Texts())
		}
	}
	snapshot(t, tt, "composer-activity")

	open := func(name, shows, shot string) {
		t.Helper()
		clickButton(t, tt, name)
		tt.Advance(time.Second) // the popover's entrance
		tt.Frame()
		if !tt.HasText(shows) {
			t.Fatalf("the %q popover does not show %q: %q", name, shows, tt.Texts())
		}
		snapshot(t, tt, shot)
		tt.Frame()
	}
	open(terminals+running, "npx vite --port 5173", "composer-activity-commands")
	clickButton(t, tt, "npx vite --port 5173") // its output opens
	tt.Frame()
	if !tt.HasText("➜  Local:   http://localhost:5173/") {
		t.Errorf("the command's output does not show: %q", tt.Texts())
	}
	tt.Advance(2 * time.Second) // the toast before is gone
	// The row's Stop, not the composer's of the same name.
	row, _ := tt.AccessByRole(ui.RoleButton, "npx vite --port 5173")
	stopped := false
	for _, e := range tt.AccessAll() {
		b := e.Bounds
		if e.Role == ui.RoleButton && e.Name == i18n.In("en", "background_end") && b.Y < row.Bounds.Y+row.Bounds.H && b.Y+b.H > row.Bounds.Y {
			tt.ClickAt(b.X+b.W/2, b.Y+b.H/2)
			stopped = true
		}
	}
	if !stopped {
		t.Fatalf("the command row has no Stop")
	}
	if !tt.HasText(i18n.In("en", "preview_notice")) {
		t.Errorf("stopping a command does not say it is a preview: %q", tt.Texts())
	}
	open("Goal", "Port the composer's activity strip and keep the benchmarks within budget", "composer-activity-goal")
	open(i18n.In("en", "composer_subagent_one")+running, "Measure scroll frames", "composer-activity-subagents")
	open(i18n.In("en", "task_progress"), "Run the benchmarks", "composer-activity-progress")

	// An idle conversation in a repository: the Git badge alone.
	click(t, tt, "Port the neutral theme tokens")
	tt.Frame()
	if !tt.HasText(git[0]) || tt.HasText("Goal") || tt.HasText(terminals) {
		t.Fatalf("the idle conversation's strip: %q", tt.Texts())
	}
	badge, _ := tt.Find(git[0]) // the rail has a Git button too
	// The badge opens its totals and the files changed last, not the
	// panel; a file opens its diff.
	tt.ClickAt(badge.X+badge.W/2, badge.Y+badge.H/2)
	tt.Advance(time.Second)
	tt.Frame()
	if a.PanelOpen && a.Panel.Active.Kind == "git" {
		t.Errorf("the Git badge opens the Git panel")
	}
	files := i18n.In("en", "directory_count_files", "count", len(a.Data.Workspace.Changes))
	if !tt.HasText(files) {
		t.Fatalf("the Git badge's popover does not show %q: %q", files, tt.Texts())
	}
	first := a.Data.Workspace.Changes[0].Path
	snapshot(t, tt, "composer-activity-git")
	clickButton(t, tt, first)
	if !a.PanelOpen || a.Panel.Active.Kind != "git" || a.Panel.Active.ID != first {
		t.Errorf("a changed file opens %+v, not its diff", a.Panel.Active)
	}
	draft, _ := tt.AccessByRole(ui.RoleTextField, i18n.In("en", "composer"))
	// A chat outside a project has no capsule, but the row stays.
	click(t, tt, "Regex for semantic versions")
	tt.Frame()
	if tt.HasText(git[0]) {
		t.Fatalf("a chat outside a project shows Git changes")
	}
	if empty, _ := tt.AccessByRole(ui.RoleTextField, i18n.In("en", "composer")); empty.Bounds.Y != draft.Bounds.Y {
		t.Errorf("the empty strip moves the composer: %v then %v", draft.Bounds.Y, empty.Bounds.Y)
	}
}
