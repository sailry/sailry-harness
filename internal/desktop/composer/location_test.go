package composer

import (
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

func TestDraftLocationMenu(t *testing.T) {
	state := New("local/model")
	state.Host, state.Draft, state.Team = "local", "Keep my draft", "local-team"
	ctx := &Context{Hosts: []Host{{ID: "local", Name: "This device"}, {ID: "remote", Name: "Debian"}},
		Projects: []Project{{ID: "same", Host: "local", Name: "Local folder"}, {ID: "same", Host: "remote", Name: "Remote folder"}}}
	tester := ui.NewTester(func(c *ui.Context) {
		theme.Apply(c, "", theme.Light)
		ui.Column(c).Fill().Justify(ui.End).Padding(20).Children(func() { state.contextBar(c, ctx, nil) })
	}, 600, 480)
	tester.SetLocale("en")
	tester.Frame()
	project, projectFound := tester.Access(i18n.In("en", "composer_no_project"))
	host, hostFound := tester.Access("This device")
	if !projectFound || !hostFound || host.Bounds.X < project.Bounds.X+project.Bounds.W || host.Bounds.Y != project.Bounds.Y {
		t.Fatalf("host choice must sit beside the project choice: project=%+v host=%+v", project, host)
	}
	click := func(name string) {
		t.Helper()
		if err := tester.Click(name); err != nil {
			t.Fatalf("click %q: %v; texts=%v", name, err, tester.Texts())
		}
		tester.Advance(300 * time.Millisecond)
		tester.Frame()
	}
	click("This device")
	click("Debian")
	if state.Host != "remote" || state.Project != "" || state.Model != "" || state.Team != "" || state.Draft != "Keep my draft" {
		t.Fatalf("host choice lost draft or retained another host's configuration: %+v", state)
	}
	click(i18n.In("en", "composer_no_project"))
	if tester.HasText(i18n.In("en", "composer_execution_host")) {
		t.Fatal("project menu still contains an execution-host submenu")
	}
	click("Local folder")
	if state.Host != "local" || state.Project != "same" || state.Worktree != "" || state.project(ctx).Name != "Local folder" {
		t.Fatalf("plain project did not supply its own host: %+v", state)
	}
	if _, found := tester.Access("This device"); found {
		t.Fatal("project host can be selected independently of its project")
	}
	click("Local folder")
	click("Remote folder")
	if state.Host != "remote" || state.project(ctx).Name != "Remote folder" {
		t.Fatalf("same-ID project was resolved on another host: %+v", state)
	}
	ctx.Projects[1].Worktrees = []Worktree{{Branch: "main", Main: true}, {Branch: "feature"}}
	state.Worktree = "feature"
	click("Remote folder")
	click("Remote folder")
	if state.Worktree != "feature" {
		t.Fatal("reselecting the current project reset the draft's checkout")
	}
}

func TestCapturedLocationCannotChange(t *testing.T) {
	for _, reason := range []string{"readonly", "admitted", "attachments"} {
		t.Run(reason, func(t *testing.T) {
			state := New("original-model")
			state.Host, state.Project, state.Worktree, state.Draft = "local", "project", "tree", "Retained"
			ctx := &Context{Readonly: reason == "readonly", LocationLocked: reason == "admitted"}
			if reason == "attachments" {
				state.Attachments = []*Attachment{{ID: "upload", Status: Preparing}}
			}
			state.chooseLocation(ctx, "remote", "", "")
			if state.Host != "local" || state.Project != "project" || state.Worktree != "tree" || state.Model != "original-model" || state.Draft != "Retained" {
				t.Fatalf("captured location changed: %+v", state)
			}
		})
	}
}
