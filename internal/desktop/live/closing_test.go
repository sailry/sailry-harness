package live

import (
	"slices"
	"testing"
	"testing/synctest"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/preferences"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// closeTestWindow drains the runtime's queue in frames that also draw the
// window's toasts.
func closeTestWindow(root *Runtime) *ui.Tester {
	tester := ui.NewTester(func(view *ui.Context) {
		theme.Apply(view, "", theme.Light)
		root.mu.Lock()
		queue := root.queue
		root.queue = nil
		root.mu.Unlock()
		for _, apply := range queue {
			apply(view)
		}
		kit.ToastHost(view)
	}, 800, 600)
	tester.SetLocale("en")
	return tester
}

// closeTestDraft opens a.go in the fixture's main worktree with an unsaved
// draft over a loaded revision.
func closeTestDraft(fixture pluginLocationTestFixture) *workspaceProject {
	root := fixture.root
	root.busy, root.uncertain = map[string]bool{}, map[string]protocol.Request{}
	project := root.workspaceFor(fixture.main)
	revision := "r1"
	project.files["a.go"] = protocol.FileContent{Path: "a.go", Text: "one\n", Revision: &revision}
	project.panel.SetFile("a.go", "one\n")
	project.panel.SetFileWritable("a.go", true)
	project.panel.FileChanged("a.go", "two")
	return project
}

func TestCloseGuard(t *testing.T) {
	t.Run("Save all writes every draft and clears the unsaved list", func(t *testing.T) {
		synctest.Test(t, func(t *testing.T) {
			fixture := newPluginLocationTestFixture(t)
			root := fixture.root
			project := closeTestDraft(fixture)
			if names := root.UnsavedDocuments(); !slices.Equal(names, []string{"fixture/a.go"}) || root.SavingDocuments() {
				t.Fatalf("unsaved documents = %v", names)
			}
			var written []protocol.CommandWriteFilePayload
			fixture.source.dispatch = func(request protocol.Request) (link.Admission, error) {
				payload, _ := detail[protocol.CommandWriteFilePayload](request.Command.Data)
				written = append(written, payload)
				completion := make(chan link.Response, 1)
				completion <- link.Response{Output: protocol.Output{Kind: "file_written", Data: protocol.FileWritten{Path: payload.Path, Revision: "r2", Size: uint64(len(payload.Text))}}}
				return link.Admission{Receipt: protocol.Receipt{ID: request.ID}, Completion: completion}, nil
			}
			window := closeTestWindow(root)
			root.SaveDocuments()
			if !root.SavingDocuments() {
				t.Fatal("a requested save is not reported before its frame")
			}
			window.Frame()
			if !root.SavingDocuments() {
				t.Fatal("a save under way is not reported")
			}
			synctest.Wait()
			window.Frame()
			if len(written) != 1 || written[0].Path != "a.go" || written[0].Text != "two\n" || written[0].ExpectedRevision == nil || *written[0].ExpectedRevision != "r1" {
				t.Fatalf("written = %+v", written)
			}
			if names := root.UnsavedDocuments(); len(names) != 0 || root.SavingDocuments() {
				t.Fatalf("after the save: unsaved %v, saving %v", names, root.SavingDocuments())
			}
			if _, dirty := project.panel.FileDraft("a.go"); dirty {
				t.Fatal("the saved draft stayed dirty")
			}
		})
	})
	t.Run("a failed save keeps the draft and tells a toast", func(t *testing.T) {
		synctest.Test(t, func(t *testing.T) {
			fixture := newPluginLocationTestFixture(t)
			root := fixture.root
			project := closeTestDraft(fixture)
			revision := "r1"
			project.files["b.go"] = protocol.FileContent{Path: "b.go", Text: "one\n", Revision: &revision}
			project.panel.SetFile("b.go", "one\n")
			project.panel.SetFileWritable("b.go", true)
			project.panel.FileChanged("b.go", "three")
			// Both files changed on disk: the writes are refused and the
			// reads that follow return the other content.
			fixture.source.dispatch = func(request protocol.Request) (link.Admission, error) {
				if request.Command.Kind == "write_file" {
					return link.Admission{}, &link.FaultError{Fault: protocol.Fault{Code: protocol.ErrorCodeRevisionConflict, Message: "changed on disk"}}
				}
				payload, _ := detail[protocol.CommandReadFilePayload](request.Command.Data)
				other := "r9"
				completion := make(chan link.Response, 1)
				completion <- link.Response{Output: protocol.Output{Kind: "file", Data: protocol.FileContent{Path: payload.Path, Text: "other\n", Revision: &other}}}
				return link.Admission{Receipt: protocol.Receipt{ID: request.ID}, Completion: completion}, nil
			}
			window := closeTestWindow(root)
			root.SaveDocuments()
			window.Frame()
			synctest.Wait()
			window.Frame()
			window.Frame()
			if names := root.UnsavedDocuments(); !slices.Equal(names, []string{"fixture/a.go", "fixture/b.go"}) || root.SavingDocuments() {
				t.Fatalf("after the failure: unsaved %v, saving %v", names, root.SavingDocuments())
			}
			if text, dirty := project.panel.FileDraft("a.go"); text != "two\n" || !dirty {
				t.Fatalf("the draft changed: %q dirty=%v", text, dirty)
			}
			if text, dirty := project.panel.FileDraft("b.go"); text != "three\n" || !dirty {
				t.Fatalf("the second draft changed: %q dirty=%v", text, dirty)
			}
			// One toast names both documents.
			told := 0
			for _, text := range window.Texts() {
				if text == i18n.In("en", "files_changed_on_disk") {
					told++
				}
			}
			if told != 1 || !window.HasText("fixture/a.go\nfixture/b.go") {
				t.Fatalf("no single conflict toast naming the documents: %q", window.Texts())
			}
			// Each draft is left marked as changed on disk for its editor.
			synctest.Wait()
			window.Frame()
			for _, name := range []string{"a.go", "b.go"} {
				if disk, changed := project.changedFiles[name]; !changed || disk.content.Revision == nil || *disk.content.Revision != "r9" {
					t.Fatalf("%s is not marked as changed on disk: %+v", name, project.changedFiles)
				}
				if _, dirty := project.panel.FileDraft(name); !dirty {
					t.Fatalf("%s lost its draft to the conflict", name)
				}
			}
		})
	})
	t.Run("a draft that cannot be saved fails without a command", func(t *testing.T) {
		synctest.Test(t, func(t *testing.T) {
			fixture := newPluginLocationTestFixture(t)
			root := fixture.root
			project := closeTestDraft(fixture)
			project.panel.SetFileWritable("a.go", false)
			fixture.source.dispatch = func(protocol.Request) (link.Admission, error) {
				t.Fatal("a read-only draft was written")
				return link.Admission{}, nil
			}
			window := closeTestWindow(root)
			root.SaveDocuments()
			window.Frame()
			window.Frame()
			if len(root.UnsavedDocuments()) != 1 || root.SavingDocuments() || !window.HasText(i18n.In("en", "workspace_action_failed")) {
				t.Fatalf("unsaved %v, saving %v, texts %q", root.UnsavedDocuments(), root.SavingDocuments(), window.Texts())
			}
		})
	})
	t.Run("only running local work asks before a quit", func(t *testing.T) {
		idle := protocol.DefaultSnapshot()
		if nodeWorks(idle) {
			t.Fatal("an idle Node asks")
		}
		running := protocol.DefaultSnapshot()
		running.Sessions = []protocol.Session{{Activity: protocol.ActivitySummary{Run: &protocol.ConversationRun{Status: protocol.ConversationStatusRunning}}}}
		completed := protocol.DefaultSnapshot()
		completed.Sessions = []protocol.Session{{Activity: protocol.ActivitySummary{Run: &protocol.ConversationRun{Status: protocol.ConversationStatusCompleted}}}}
		terminal := protocol.DefaultSnapshot()
		terminal.Terminals = []protocol.TerminalInfo{{Status: protocol.TerminalStatus{Kind: "running"}}}
		exited := protocol.DefaultSnapshot()
		exited.Terminals = []protocol.TerminalInfo{{Status: protocol.TerminalStatus{Kind: "exited"}}}
		if !nodeWorks(running) || nodeWorks(completed) || !nodeWorks(terminal) || nodeWorks(exited) {
			t.Fatal("running tasks and terminals, and only they, are local work")
		}
	})
}

func TestNoticeChannels(t *testing.T) {
	node := protocol.NodeID{1}
	session := protocol.SessionID{7}
	failed := client.Notice{ID: client.NoticeID{Node: node, Cursor: 1, Target: client.NoticeTarget{Kind: "session", Session: session}}, Kind: "failed", Session: &session}
	approval := client.Notice{ID: client.NoticeID{Node: node, Cursor: 2, Target: client.NoticeTarget{Kind: "session", Session: session}}, Kind: "approval", Session: &session}
	plugin := client.Notice{ID: client.NoticeID{Node: node, Cursor: 3, Target: client.NoticeTarget{Kind: "notification", Notification: protocol.NotificationID{3}}}, Kind: "notification", NotificationKind: protocol.NotificationKindInfo, Title: "Index", Message: "Rebuilt"}
	deliver := func(prefs *preferences.Store, focused bool) (toasts, system []string) {
		n := &Notifier{prefs: prefs, seen: map[client.NoticeID]bool{}, pending: []client.Notice{failed, approval, plugin}}
		n.SetNotify(func(id, _, _, _ string) { system = append(system, id) })
		n.SetPresent(func(notice *viewmodel.Notice) bool {
			if focused {
				toasts = append(toasts, notice.ID)
			}
			return focused
		})
		n.SetUpdate(func(f func()) { f() })
		return toasts, system
	}
	all := []string{noticeID(failed), noticeID(approval), noticeID(plugin)}
	t.Run("the active window takes every notice and the system gets none", func(t *testing.T) {
		toasts, system := deliver(&preferences.Store{Data: preferences.Default()}, true)
		if !slices.Equal(toasts, all) || len(system) != 0 {
			t.Fatalf("toasts %v, system %v", toasts, system)
		}
	})
	t.Run("without an active window each notice is one system notification", func(t *testing.T) {
		toasts, system := deliver(&preferences.Store{Data: preferences.Default()}, false)
		if len(toasts) != 0 || !slices.Equal(system, all) {
			t.Fatalf("toasts %v, system %v", toasts, system)
		}
	})
	t.Run("the system switch silences only system notifications", func(t *testing.T) {
		prefs := &preferences.Store{Data: preferences.Default()}
		prefs.Data.Notifications[0] = false
		if toasts, system := deliver(prefs, true); !slices.Equal(toasts, all) || len(system) != 0 {
			t.Fatalf("active: toasts %v, system %v", toasts, system)
		}
		if toasts, system := deliver(prefs, false); len(toasts) != 0 || len(system) != 0 {
			t.Fatalf("background: toasts %v, system %v", toasts, system)
		}
	})
	t.Run("a category switched off reaches neither channel", func(t *testing.T) {
		prefs := &preferences.Store{Data: preferences.Default()}
		prefs.Data.Notifications[3] = false
		rest := []string{noticeID(approval), noticeID(plugin)}
		if toasts, _ := deliver(prefs, true); !slices.Equal(toasts, rest) {
			t.Fatalf("toasts %v", toasts)
		}
		if _, system := deliver(prefs, false); !slices.Equal(system, rest) {
			t.Fatalf("system %v", system)
		}
	})
	t.Run("the toast's Details opens the conversation or the notification centre", func(t *testing.T) {
		synctest.Test(t, func(t *testing.T) {
			fixture := newPluginLocationTestFixture(t)
			root := fixture.root
			window := closeTestWindow(root)
			details := i18n.In("en", "notification_details")
			task := &viewmodel.Notice{ID: "task", Kind: viewmodel.NoticeFailed, TitleKey: "activity_failed", Message: "Build the index", Session: "draft/first"}
			root.App.Data.Notices = []*viewmodel.Notice{task}
			root.PresentNotice(task)
			window.Frame()
			window.Advance(500 * time.Millisecond)
			if !window.HasText(i18n.In("en", "activity_failed")) || !window.HasText("Build the index") || !window.HasText(details) {
				t.Fatalf("no notice toast: %q", window.Texts())
			}
			if err := window.Click(details); err != nil {
				t.Fatal(err)
			}
			window.Frame()
			if root.App.Sidebar.Selected != "draft/first" || root.App.Page != shell.ConversationPage {
				t.Fatalf("Details selected %q on page %v", root.App.Sidebar.Selected, root.App.Page)
			}
			window.Advance(500 * time.Millisecond)
			if window.HasText("Build the index") {
				t.Fatal("the action left its toast open")
			}
			notice := &viewmodel.Notice{ID: "plugin", Kind: viewmodel.NoticeInfo, Title: "Index", Message: "Rebuilt"}
			root.PresentNotice(notice)
			window.Frame()
			window.Advance(500 * time.Millisecond)
			if err := window.Click(details); err != nil {
				t.Fatalf("%v: %q", err, window.Texts())
			}
			if root.App.Page != shell.NotificationsPage {
				t.Fatalf("a notice without a conversation opened page %v", root.App.Page)
			}
		})
	})
	t.Run("a notice of the conversation on screen has no toast", func(t *testing.T) {
		synctest.Test(t, func(t *testing.T) {
			fixture := newPluginLocationTestFixture(t)
			root := fixture.root
			root.conversationSeen = map[protocol.SessionID]bool{session: true}
			window := closeTestWindow(root)
			root.PresentNotice(&viewmodel.Notice{ID: "seen", Kind: viewmodel.NoticeCompleted, Title: "Shown already", Session: session.String()})
			root.PresentNotice(&viewmodel.Notice{ID: "other", Kind: viewmodel.NoticeCompleted, Title: "Elsewhere", Session: protocol.SessionID{8}.String()})
			window.Frame()
			window.Frame()
			if window.HasText("Shown already") || !window.HasText("Elsewhere") {
				t.Fatalf("toasts: %q", window.Texts())
			}
		})
	})
}
