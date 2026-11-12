package shell

import (
	"slices"
	"strings"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
)

// TestDemoActions covers the demo's conversation, panel and notice actions:
// each does in memory what its control says, and none ends in the preview's
// notice, which the demo does not show.
func TestDemoActions(t *testing.T) {
	en := func(key string, args ...any) string { return i18n.In("en", key, args...) }
	const flaky = "harbor-api/flaky-test"
	// open shows the demo on id; act runs fn on its sample actions inside a
	// frame, as a control's click does.
	open := func(t *testing.T, id string) (*App, *ui.Tester, func(func(*actions))) {
		t.Helper()
		a := New(preview.Load())
		a.Settings.Model.DemoMode = true
		var run func(*ui.Context)
		tt := ui.NewTesterWith(func(c *ui.Context) {
			if run != nil {
				run(c)
				run = nil
			}
			a.View(c)
		}, 1280, 820, opaque)
		tt.SetLocale("en")
		tt.Frame()
		a.Sidebar.Selected = id
		settle(tt)
		act := func(fn func(*actions)) {
			t.Helper()
			run = func(c *ui.Context) {
				cv := a.Data.Find(a.Sidebar.Selected)
				fn(&actions{a: a, c: c, cv: cv, comp: a.Composer(cv.ID)})
			}
			tt.Frame()
			tt.Frame()
			for _, key := range []string{"preview_notice", "unavailable"} {
				if tt.HasText(en(key)) {
					t.Fatalf("the action says %q", en(key))
				}
			}
		}
		return a, tt, act
	}
	play := func(tt *ui.Tester) {
		for range 40 {
			tt.Advance(time.Second)
			tt.Frame()
		}
	}

	t.Run("Retry", func(t *testing.T) {
		a, tt, act := open(t, flaky)
		cv := a.Data.Find(flaky)
		last := len(cv.Turns) - 1
		act(func(x *actions) { x.Retry(cv.Turns[last].ID) })
		if !cv.Running() || cv.Turns[last].Reply.Status == conversation.Completed {
			t.Fatalf("Retry did not run the turn: %d", cv.Turns[last].Reply.Status)
		}
		// Stop ends the run that plays.
		act(func(x *actions) { x.Stop() })
		if cv.Running() || cv.Turns[last].Reply.Status != conversation.Cancelled {
			t.Fatalf("Stop left the reply %d", cv.Turns[last].Reply.Status)
		}
		act(func(x *actions) { x.Retry(cv.Turns[last].ID) })
		play(tt)
		if r := cv.Turns[last].Reply; r.Status != conversation.Completed || cv.Running() {
			t.Fatalf("the retried reply ended %d", r.Status)
		}
	})

	// Enter sends as the button does: the reply plays from the frame's time,
	// not at once, though a key's handler runs between frames.
	t.Run("EnterSends", func(t *testing.T) {
		a, tt, _ := open(t, flaky)
		cv := a.Data.Find(flaky)
		n := len(cv.Turns)
		click(t, tt, en("composer"))
		tt.Type("Run it once more")
		tt.Key(0, ui.KeyEnter)
		tt.Frame()
		if len(cv.Turns) != n+1 || !cv.Running() || cv.Turns[n].Reply.Status == conversation.Completed {
			t.Fatalf("Enter: %d turns, running %v, status %d", len(cv.Turns), cv.Running(), cv.Turns[n].Reply.Status)
		}
		play(tt)
		if cv.Turns[n].Reply.Status != conversation.Completed {
			t.Fatalf("the reply ended %d", cv.Turns[n].Reply.Status)
		}
	})

	t.Run("EditAndResend", func(t *testing.T) {
		a, tt, act := open(t, flaky)
		cv := a.Data.Find(flaky)
		first := cv.Turns[0].ID
		act(func(x *actions) { x.Edit(first) })
		act(func(x *actions) { x.SaveEdit(first, "Fix the other test") })
		if len(cv.Turns) != 1 || cv.Turns[0].User != "Fix the other test" || !cv.Running() {
			t.Fatalf("the edit did not resend: %d turns, %q, running %v", len(cv.Turns), cv.Turns[0].User, cv.Running())
		}
		play(tt)
		if cv.Turns[0].Reply.Status != conversation.Completed {
			t.Fatalf("the resent reply ended %d", cv.Turns[0].Reply.Status)
		}
	})

	t.Run("Fork", func(t *testing.T) {
		a, _, act := open(t, flaky)
		cv := a.Data.Find(flaky)
		pr := a.Data.ProjectOf(flaky)
		n, trees := len(pr.Conversations), len(pr.Worktrees)
		act(func(x *actions) { x.Fork(cv.Turns[0].ID) })
		fork := a.Data.Find(a.Sidebar.Selected)
		if a.Sidebar.Selected == flaky || len(pr.Conversations) != n+1 || len(fork.Turns) != 1 || fork.Turns[0].User != cv.Turns[0].User || len(cv.Turns) < 2 {
			t.Fatalf("Fork: selected %q with %d turns", a.Sidebar.Selected, len(fork.Turns))
		}
		a.Sidebar.Selected = flaky
		act(func(x *actions) { x.Worktree(cv.Turns[0].ID) })
		tree := a.Data.Find(a.Sidebar.Selected)
		if len(pr.Worktrees) <= trees || tree.Worktree == "" || tree.Worktree != pr.Worktrees[len(pr.Worktrees)-1].Branch {
			t.Fatalf("Create worktree: %d worktrees, the copy in %q", len(pr.Worktrees), tree.Worktree)
		}
		// A chat forks beside itself and has no worktree to make.
		chat := a.Data.Unassigned[0]
		a.Sidebar.Selected = chat.ID
		chats := len(a.Data.Unassigned)
		act(func(x *actions) { x.Fork(chat.Turns[0].ID) })
		if len(a.Data.Unassigned) != chats+1 || a.Sidebar.Selected == chat.ID {
			t.Fatalf("a chat's fork: %d chats, selected %q", len(a.Data.Unassigned), a.Sidebar.Selected)
		}
		if !a.Data.Find(a.Sidebar.Selected).Turns[0].Controls.NoWorktree {
			t.Fatal("a chat offers Create worktree")
		}
	})

	t.Run("Rewind", func(t *testing.T) {
		a, tt, _ := open(t, flaky)
		cv := a.Data.Find(flaky)
		n := len(cv.Turns)
		a.Messages.AskRewind(cv.Turns[0].ID)
		settle(tt)
		if !tt.HasText(en("chat_rewind_confirm")) || len(cv.Turns) != n {
			t.Fatalf("Rewind did not ask first: %q", tt.Texts())
		}
		clickButton(t, tt, en("chat_rewind_action"))
		settle(tt)
		if len(cv.Turns) != 1 {
			t.Fatalf("Rewind left %d of %d turns", len(cv.Turns), n)
		}
	})

	t.Run("Compact", func(t *testing.T) {
		a, tt, act := open(t, flaky)
		cv := a.Data.Find(flaky)
		n := len(cv.Turns)
		act(func(x *actions) { x.Compact() })
		if len(cv.Turns) != n+1 || cv.Turns[n].Compaction == nil || cv.Turns[n].Compaction.State != conversation.CompactionRunning {
			t.Fatalf("Compact added no running compaction")
		}
		play(tt)
		if k := cv.Turns[n].Compaction; k.State != conversation.Compacted || k.Summary == "" || cv.Running() {
			t.Fatalf("the compaction ended as %+v", k)
		}
	})

	t.Run("UndoAndReview", func(t *testing.T) {
		a, tt, act := open(t, flaky)
		cv := a.Data.Find(flaky)
		at := slices.IndexFunc(cv.Turns, func(t conversation.Turn) bool { return t.Reply.Changes != nil })
		ch, turn := cv.Turns[at].Reply.Changes, cv.Turns[at].ID
		act(func(x *actions) { x.Review(turn, "") })
		if !a.PanelOpen || a.Panel.Active.Kind != panel.Changes {
			t.Fatalf("Review opened %+v", a.Panel.Active)
		}
		// Undo asks over the History tab, as the application does.
		act(func(x *actions) { x.Undo(turn, "") })
		settle(tt)
		if !a.PanelOpen || a.Panel.Active.Kind != panel.History || ch.Undone[""] {
			t.Fatalf("Undo: tab %+v, undone %v", a.Panel.Active, ch.Undone)
		}
		if !tt.HasText(en("workspace_restore_confirm")) {
			t.Fatalf("Undo did not ask: %q", tt.Texts())
		}
		clickButton(t, tt, en("turn_changes_undo_all"))
		settle(tt)
		if !ch.Undone[""] {
			t.Fatalf("the confirmed Undo left %v", ch.Undone)
		}
		// The History tab's own Undo all asks, then drops the checkpoint.
		checkpoints := len(a.Data.Workspace.Checkpoints)
		var undo ui.AccessInfo
		for _, info := range tt.AccessAll() {
			if info.Role == ui.RoleButton && info.Name == en("turn_changes_undo_all") && info.Bounds.X > 640 {
				undo = info
				break
			}
		}
		tt.ClickAt(undo.Bounds.X+undo.Bounds.W/2, undo.Bounds.Y+undo.Bounds.H/2)
		settle(tt)
		if len(a.Data.Workspace.Checkpoints) != checkpoints || !tt.HasText(en("workspace_restore_confirm")) {
			t.Fatalf("the checkpoint's Undo all did not ask: %q", tt.Texts())
		}
	})

	t.Run("Composer", func(t *testing.T) {
		a, _, act := open(t, flaky)
		cv := a.Data.Find(flaky)
		pr := a.Data.ProjectOf(flaky)
		trees := len(pr.Worktrees)
		act(func(x *actions) { x.CreateWorktree() })
		if len(pr.Worktrees) <= trees || cv.Worktree == "" || a.Composer(flaky).Worktree != cv.Worktree {
			t.Fatalf("New worktree: %d worktrees, the session in %q", len(pr.Worktrees), cv.Worktree)
		}
		act(func(x *actions) { x.AddProject() })
		if !a.Sidebar.Draft().Open {
			t.Fatal("Add project opened no editor")
		}
		cv.Commands = []composer.Command{{ID: "k1", Command: "make watch"}, {ID: "k2", Command: "make serve"}}
		act(func(x *actions) { x.StopCommand("k1") })
		if len(cv.Commands) != 1 || cv.Commands[0].ID != "k2" {
			t.Fatalf("Stop command left %+v", cv.Commands)
		}
		act(func(x *actions) { x.OpenService(8080) })
		if !a.PanelOpen || a.Panel.Active.Kind != panel.Browser {
			t.Fatalf("Open service showed %+v", a.Panel.Active)
		}
		act(func(x *actions) { x.ManageTeams() })
		if a.Page != SettingsPage || a.Settings.Section != settings.Teams {
			t.Fatalf("Manage teams showed page %d", a.Page)
		}
	})

	t.Run("Goal", func(t *testing.T) {
		a, _, act := open(t, "lumen-web/pursuit")
		cv := a.Data.Find("lumen-web/pursuit")
		id := cv.Badges[0].ID
		act(func(x *actions) { x.Badge(id, "goal-pause") })
		if cv.Badges[0].Panel.Buttons[0].ID != "goal-resume" {
			t.Fatalf("Pause: %+v", cv.Badges[0].Panel.Buttons)
		}
		act(func(x *actions) { x.Badge(id, "goal-end") })
		if len(cv.Badges) != 0 {
			t.Fatal("End kept the goal")
		}
	})

	// Voice input plays on the clock and ends with the demo's words in the
	// draft: no microphone opens and nothing is said of it.
	t.Run("Dictation", func(t *testing.T) {
		a, tt, act := open(t, flaky)
		comp := a.Composer(flaky)
		act(func(x *actions) { x.Dictate(&comp.Dictation, composer.StartDictation) })
		play(tt)
		if comp.Dictation.Phase != composer.Recording {
			t.Fatalf("the dictation is in phase %d", comp.Dictation.Phase)
		}
		act(func(x *actions) { x.Dictate(&comp.Dictation, composer.FinishDictation) })
		play(tt)
		heard := preview.DemoCatalog("en")["dictation.text"]
		if comp.Dictation.Phase != composer.Idle || heard == "" || !strings.HasSuffix(comp.Draft, heard) {
			t.Fatalf("the dictation ended in phase %d with the draft %q", comp.Dictation.Phase, comp.Draft)
		}
		if tt.HasText(en("preview_notice")) {
			t.Fatal("the dictation tells the preview's notice")
		}
	})

	// A cited page of the session overview opens in the panel's browser, and
	// a file without a native preview opens a tab that names it: neither
	// leaves the window.
	t.Run("SourcesAndFiles", func(t *testing.T) {
		const metrics = "lumen-web/metrics"
		a, tt, act := open(t, metrics)
		cv := a.Data.Find(metrics)
		cited := 0
		act(func(x *actions) {
			o := a.sessionOverview(x.c, cv)
			if cited = len(o.Citations); cited > 0 {
				o.OpenSource(o.Citations[0].URI)
			}
		})
		if cited == 0 {
			t.Fatal("the demo's metrics conversation cites no page")
		}
		if !a.PanelOpen || a.Panel.Active.Kind != panel.Browser {
			t.Fatalf("the cited page opened %+v", a.Panel.Active)
		}
		var pdf *conversation.File
		for i := range cv.Turns {
			for _, b := range cv.Turns[i].Reply.Blocks {
				for j := range b.Files {
					if b.Files[j].Kind == conversation.OtherFile {
						pdf = &b.Files[j]
					}
				}
			}
		}
		if pdf == nil {
			t.Fatal("the demo has no file without a native preview")
		}
		for _, open := range []func(x *actions){
			func(x *actions) { x.OpenFile(*pdf) },
			func(x *actions) { x.FileAction(*pdf, conversation.OpenExternal) },
		} {
			a.Panel.Close(a.Panel.Active)
			act(open)
			settle(tt)
			if a.Panel.Active != (panel.Tab{Kind: panel.Artifact, ID: pdf.ID}) || tt.HasText(en("files_open_failed")) || len(tt.OpenedURLs()) != 0 {
				t.Fatalf("the file opened %+v, urls %q: %q", a.Panel.Active, tt.OpenedURLs(), tt.Texts())
			}
			if !slices.Contains(tt.Texts(), pdf.Name()) || tt.HasText(en("artifact_preview_failed")) {
				t.Fatalf("the file's tab does not name it: %q", tt.Texts())
			}
		}
	})

	// Open folder lists the sample catalog's directories and adds the one
	// chosen to the demo; the selection dialog offers no Read aloud where no
	// voice would read.
	t.Run("OpenFolderAndSelection", func(t *testing.T) {
		a, tt, _ := open(t, flaky)
		n := len(a.Data.Projects)
		a.Do("folder.open")
		settle(tt)
		if !a.Sidebar.Picker.Showing() || tt.HasText(en("directory_preview")) {
			t.Fatalf("Open folder: picker %v: %q", a.Sidebar.Picker.Showing(), tt.Texts())
		}
		click(t, tt, "sailry")
		settle(tt)
		all := tt.AccessAll()
		for i := len(all) - 1; i >= 0; i-- {
			if all[i].Role == ui.RoleButton && all[i].Name == en("directory_open") {
				tt.ClickAt(all[i].Bounds.X+all[i].Bounds.W/2, all[i].Bounds.Y+all[i].Bounds.H/2)
				break
			}
		}
		settle(tt)
		if len(a.Data.Projects) != n+1 || a.Data.Projects[n].Name != "sailry" || a.Sidebar.Picker.Showing() {
			t.Fatalf("Open folder added %d projects", len(a.Data.Projects)-n)
		}
		// The dialog is modal: what is named while it shows is its own.
		a.Page, a.Sidebar.Selected = ConversationPage, flaky
		settle(tt)
		a.captureSelection()
		settle(tt)
		if _, quote := tt.AccessByRole(ui.RoleButton, en("message_quote")); !a.selected.open || !quote {
			t.Fatalf("the selection dialog: open %v, Quote %v", a.selected.open, quote)
		}
		for _, info := range tt.AccessAll() {
			if info.Name == en("reader_read") {
				t.Fatalf("the demo's selection dialog offers Read aloud: %+v", info.Bounds)
			}
		}
	})

	// What needs the system or a file is not offered: the transcript has no
	// export menu, and the image viewer no Copy, Save or Open.
	t.Run("Absent", func(t *testing.T) {
		_, tt, _ := open(t, flaky)
		for _, info := range tt.AccessAll() {
			if info.Name == en("conversation_export") {
				t.Fatalf("the demo offers Export: %+v", info)
			}
		}
	})

	t.Run("Notices", func(t *testing.T) {
		a, tt, _ := open(t, flaky)
		// A live window's hooks would tell its Node: the demo's notices are
		// its own.
		told := 0
		a.Hooks.ReadNotice = func(string) { told++ }
		a.Hooks.ReadNotices = func([]string) { told++ }
		a.Hooks.DismissRead = func([]string) { told++ }
		a.Hooks.DismissNotices = func([]string) { told++ }
		list := a.Data.Workspace.Notices
		if len(list) < 3 || len(a.noticeIDs(false)) < 2 || len(a.noticeIDs(true)) < 1 {
			t.Fatalf("the demo has %d notices", len(list))
		}
		first := list[0]
		a.readNotice(first)
		if !first.Read {
			t.Fatal("a notice stays unread")
		}
		a.dismissNotice(first)
		if slices.Contains(a.Data.Workspace.Notices, first) {
			t.Fatal("a dismissed notice stays")
		}
		a.Navigate(NotificationsPage)
		settle(tt)
		if !tt.HasText(a.Data.Workspace.Notices[0].Title) {
			t.Fatalf("the notification centre is empty: %q", tt.Texts())
		}
		clickButton(t, tt, en("notifications_read_all"))
		settle(tt)
		if len(a.noticeIDs(false)) != 0 {
			t.Fatal("Mark all read left unread notices")
		}
		clickButton(t, tt, en("notification_delete_read"))
		settle(tt)
		if len(a.Data.Workspace.Notices) != 0 || told != 0 {
			t.Fatalf("Delete read left %d notices; the Node was told %d times", len(a.Data.Workspace.Notices), told)
		}
	})
}
