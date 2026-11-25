package application

import (
	"context"
	"errors"
	"maps"
	"path/filepath"
	"slices"
	"strings"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/live"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/desktop/preferences"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	desktop "github.com/sailry/sailry-harness/internal/desktop/testassembly"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
	filesdesktop "github.com/sailry/sailry-harness/modules/files/desktop"
	market "github.com/sailry/sailry-harness/modules/market/desktop"
)

const assemblyFile = "notes.go"

type assemblyTestNode struct {
	snapshot protocol.Snapshot
	readFile func() protocol.FileContent
	write    func(protocol.CommandWriteFilePayload) <-chan link.Response
}

func newAssemblyTestClient(t *testing.T, readFile func() protocol.FileContent, write func(protocol.CommandWriteFilePayload) <-chan link.Response) (*client.Client, protocol.Session) {
	t.Helper()
	project := protocol.DefaultProject()
	project.ID, project.Name, project.Path = protocol.ProjectID{1}, "Fixture", "/fixture"
	tree := protocol.Worktree{ID: protocol.WorktreeID{1}, Project: &project.ID, Path: project.Path, Main: true}
	session := protocol.DefaultSession()
	session.ID, session.Project, session.Worktree = protocol.SessionID{1}, &project.ID, tree.ID
	session.Revision, session.Activity.Title = 1, "Fixture conversation"
	snapshot := protocol.DefaultSnapshot()
	snapshot.Node, snapshot.Projects, snapshot.Worktrees, snapshot.Sessions = protocol.NodeID{1}, []protocol.Project{project}, []protocol.Worktree{tree}, []protocol.Session{session}
	node := &assemblyTestNode{snapshot: snapshot, readFile: readFile, write: write}
	controller := client.New(link.NewLocal(snapshot.Node, snapshot.Node, node))
	if err := controller.Start(t.Context()); err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = controller.Close() })
	if !controller.Connected() {
		t.Fatal("fixture Node did not deliver its snapshot")
	}
	return controller, session
}

func (n *assemblyTestNode) Open(context.Context, protocol.NodeID, protocol.StreamID) (link.Stream, error) {
	return nil, errors.New("fixture streams are unavailable")
}

func (n *assemblyTestNode) Dispatch(_ context.Context, _ protocol.NodeID, request protocol.Request) (link.Admission, error) {
	receipt := protocol.Receipt{ID: request.ID, Durable: request.Command.Durable()}
	if write, ok := request.Command.Data.(protocol.CommandWriteFilePayload); ok && n.write != nil {
		return link.Admission{Receipt: receipt, Completion: n.write(write)}, nil
	}
	var response link.Response
	switch request.Command.Kind {
	case "read_session":
		response.Output = protocol.Output{Kind: "session", Data: n.snapshot.Sessions[0]}
	case "list_directory":
		response.Output = protocol.Output{Kind: "directory", Data: protocol.Directory{Entries: []protocol.FileEntry{{Name: assemblyFile, Kind: protocol.EntryKindFile}}}}
	case "inspect_git":
		response.Output = protocol.Output{Kind: "git_status", Data: protocol.GitStatus{Kind: protocol.RepositoryKindDirectory}}
	case "list_artifacts":
		response.Output = protocol.Output{Kind: "artifacts", Data: protocol.DefaultArtifactPage()}
	case "read_file":
		file := protocol.FileContent{Path: assemblyFile, Text: "package fixture\n", Revision: new("r1")}
		if n.readFile != nil {
			file = n.readFile()
		}
		response.Output = protocol.Output{Kind: "file_content", Data: file}
	default:
		response.Fault = &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "fixture command is unavailable"}
	}
	completion := make(chan link.Response, 1)
	completion <- response
	return link.Admission{Receipt: receipt, Completion: completion}, nil
}

func (n *assemblyTestNode) Subscribe(_ context.Context, _ protocol.NodeID, topic protocol.Topic) (link.Subscription, error) {
	stream := &assemblyTestSubscription{closed: make(chan struct{})}
	switch topic.Kind {
	case "node":
		stream.initial = &protocol.Update{Kind: "snapshot", Data: n.snapshot}
	case "conversation":
		history := protocol.DefaultConversationSnapshot()
		history.Node, history.Sequence, history.Page.Session, history.Page.Revision = n.snapshot.Node, 1, n.snapshot.Sessions[0].ID, 1
		stream.initial = &protocol.Update{Kind: "conversation_snapshot", Data: history}
	}
	return stream, nil
}

type assemblyTestSubscription struct {
	closed  chan struct{}
	once    sync.Once
	initial *protocol.Update
}

func (s *assemblyTestSubscription) Next(ctx context.Context) (protocol.Update, error) {
	if s.initial != nil {
		initial := *s.initial
		s.initial = nil
		return initial, nil
	}
	select {
	case <-s.closed:
		return protocol.Update{}, context.Canceled
	case <-ctx.Done():
		return protocol.Update{}, ctx.Err()
	}
}

func (s *assemblyTestSubscription) Close() error {
	s.once.Do(func() { close(s.closed) })
	return nil
}

func assemblyTester(view func(*ui.Context)) *ui.Tester {
	tester := ui.NewTesterWith(view, 1280, 820, ui.TesterOptions{Preferences: ui.Preferences{ReduceMotion: true, ReduceTransparency: true}})
	tester.SetLocale("en")
	return tester
}

func awaitAssemblyFrame(t *testing.T, tester *ui.Tester, wakes <-chan struct{}, description string, ready func() bool) {
	t.Helper()
	timeout := time.NewTimer(5 * time.Second)
	defer timeout.Stop()
	for {
		tester.Frame()
		if ready() {
			return
		}
		select {
		case <-wakes:
		case <-timeout.C:
			t.Fatalf("waiting for %s: %q", description, tester.Texts())
		}
	}
}

func TestCloseConversationDraftWithoutResourcePages(t *testing.T) {
	for _, conflict := range []bool{false, true} {
		name := "save"
		if conflict {
			name = "revision-conflict"
		}
		t.Run(name, func(t *testing.T) {
			const source = "package fixture\n"
			var diskChanged atomic.Bool
			wakes := make(chan struct{}, 1)
			wake := func() {
				select {
				case wakes <- struct{}{}:
				default:
				}
			}
			writes := make(chan protocol.CommandWriteFilePayload, 4)
			completion := make(chan link.Response, 1)
			controller, session := newAssemblyTestClient(t, func() protocol.FileContent {
				if diskChanged.Load() {
					return protocol.FileContent{Path: assemblyFile, Text: "package external\n", Revision: new("r2")}
				}
				return protocol.FileContent{Path: assemblyFile, Text: source, Revision: new("r1")}
			}, func(write protocol.CommandWriteFilePayload) <-chan link.Response {
				writes <- write
				wake()
				return completion
			})
			prefs := preferences.Open(filepath.Join(t.TempDir(), "preferences.json"))
			prefs.Data.WelcomeSeen = new(true)
			root, err := live.NewWithAssembly(t.Context(), controller, prefs, shell.Assemble(market.Register, filesdesktop.RegisterPanel))
			if err != nil {
				t.Fatal(err)
			}
			defer root.Close()
			if root.App.NativePageEnabled(shell.FilesPage) || root.App.NativePageEnabled(shell.GitPage) {
				t.Fatal("standalone resource pages were registered in the omitted assembly")
			}
			root.SetInvalidate(wake)
			root.OpenSession(session.ID.String())
			entry := &desktopWindow{app: root.App, live: root}
			closes := 0
			guard := &exitGuard{
				windows:   func() []*desktopWindow { return []*desktopWindow{entry, {}} },
				documents: func(w *desktopWindow) []shell.CloseDocument { return w.live.UnsavedDocuments() },
				save:      func(w *desktopWindow) { w.live.SaveDocuments() },
				saving:    func(w *desktopWindow) bool { return w.live.SavingDocuments() },
				flush:     func(w *desktopWindow, done func(bool)) bool { return w.live.FlushEdits(done) },
				raise:     func(*desktopWindow) {},
			}
			guard.shut = func(w *desktopWindow) {
				if w != entry || guard.closing(w) {
					t.Error("the confirmed window close was blocked")
				}
				closes++
			}
			var prompts []kit.Confirmation
			var answer func(int)
			tester := assemblyTester(func(c *ui.Context) {
				kit.BindConfirmations(c, func(prompt kit.Confirmation, done func(int)) {
					if prompt.Title == i18n.In("en", "files_unsaved") {
						prompts, answer = append(prompts, prompt), done
					}
				})
				entry.app.View(c)
			})
			tester.Frame()
			tester.Key(ui.Cmd, ui.KeyP)
			awaitAssemblyFrame(t, tester, wakes, "the conversation file panel", func() bool {
				_, shown := tester.AccessByRole(ui.RoleTreeItem, assemblyFile)
				return shown
			})
			row, _ := tester.AccessByRole(ui.RoleTreeItem, assemblyFile)
			if entry.app.Page != shell.ConversationPage || !entry.app.PanelOpen || entry.app.Panel.Active.Kind != panel.Files {
				t.Fatalf("the conversation file panel did not open: %q", tester.Texts())
			}
			tester.ClickAt(row.Bounds.X+row.Bounds.W/2, row.Bounds.Y+row.Bounds.H/2)
			awaitAssemblyFrame(t, tester, wakes, "the file editor", func() bool {
				_, shown := tester.Access("Code editor")
				return shown
			})
			editor, _ := tester.Access("Code editor")
			if entry.app.Panel.Active != (panel.Tab{Kind: panel.Files, ID: assemblyFile}) {
				t.Fatalf("the panel did not open the file editor: %q", tester.Texts())
			}
			tester.ClickAt(editor.Bounds.X+120, editor.Bounds.Y+8)
			tester.Frame()
			tester.Type("X")
			tester.Frame()
			project := entry.app.Hooks.WorkspaceProject()
			if project == nil {
				t.Fatal("the conversation panel has no workspace")
			}
			draft, dirty := project.FileDraft(assemblyFile)
			docs := root.UnsavedDocuments()
			if !dirty || strings.Replace(draft, "X", "", 1) != source || len(docs) != 1 || docs[0].Name != "fixture/"+assemblyFile {
				t.Fatalf("typing left draft %q (dirty=%v), close documents %v", draft, dirty, docs)
			}
			close := func(count int, choice string) {
				t.Helper()
				if !guard.closing(entry) {
					t.Fatal("the window close did not wait for the draft prompt")
				}
				tester.Frame()
				if len(prompts) != count || answer == nil || !strings.Contains(prompts[count-1].Message, "fixture/"+assemblyFile) {
					t.Fatalf("unexpected close prompts: %+v", prompts)
				}
				index := slices.Index(prompts[count-1].Buttons, i18n.In("en", choice))
				if index < 0 {
					t.Fatalf("close prompt did not offer %q: %+v", choice, prompts[count-1])
				}
				answer(index)
				tester.Frame()
			}
			close(1, "settings_cancel")
			if got, dirty := project.FileDraft(assemblyFile); !dirty || got != draft || entry.app.Closing() || closes != 0 || len(writes) != 0 {
				t.Fatalf("Cancel changed the draft or continued closing: %q dirty=%v closes=%d writes=%d", got, dirty, closes, len(writes))
			}
			close(2, "files_save_all")
			awaitAssemblyFrame(t, tester, wakes, "Save All to reach the Node", func() bool { return len(writes) != 0 })
			tester.Advance(250 * time.Millisecond)
			tester.Frame()
			if !guard.closing(entry) || !root.SavingDocuments() || closes != 0 || len(writes) != 1 {
				t.Fatalf("close while saving: saving=%v closes=%d writes=%d", root.SavingDocuments(), closes, len(writes))
			}
			write := <-writes
			if write.Worktree != session.Worktree || write.Path != assemblyFile || write.Text != draft || write.ExpectedRevision == nil || *write.ExpectedRevision != "r1" {
				t.Fatalf("Save All wrote %+v, want the edited draft over r1", write)
			}
			if conflict {
				diskChanged.Store(true)
				completion <- link.Response{Fault: &protocol.Fault{Code: protocol.ErrorCodeRevisionConflict, Message: "changed on disk"}}
			} else {
				completion <- link.Response{Output: protocol.Output{Kind: "file_written", Data: protocol.FileWritten{Path: assemblyFile, Revision: "r2", Size: uint64(len(draft))}}}
			}
			awaitAssemblyFrame(t, tester, wakes, "Save All to settle", func() bool { return !root.SavingDocuments() })
			tester.Advance(250 * time.Millisecond)
			tester.Frame()
			if entry.app.Closing() || root.SavingDocuments() || len(prompts) != 2 || len(writes) != 0 {
				t.Fatalf("save did not settle once: closing=%v saving=%v prompts=%d extra writes=%d", entry.app.Closing(), root.SavingDocuments(), len(prompts), len(writes))
			}
			docs = root.UnsavedDocuments()
			if conflict {
				if got, dirty := project.FileDraft(assemblyFile); !dirty || got != draft || closes != 0 || len(docs) != 1 || !tester.HasText(i18n.In("en", "files_changed_on_disk")) {
					t.Fatalf("conflicting Save All changed the draft or closed: %q dirty=%v closes=%d docs=%v", got, dirty, closes, docs)
				}
			} else if _, dirty := project.FileDraft(assemblyFile); dirty || len(docs) != 0 || closes != 1 {
				t.Fatalf("Save All did not clear the draft and close once: dirty=%v docs=%v closes=%d", dirty, docs, closes)
			}
		})
	}
}

func TestWindowPreferencesAcrossNativeAssemblies(t *testing.T) {
	controller, session := newAssemblyTestClient(t, nil, nil)
	dir := t.TempDir()
	profilePath := filepath.Join(dir, "profile", "desktop", "preferences.json")
	store := preferences.Open(profilePath)
	store.Data.WelcomeSeen = new(true)
	if err := store.Save(); err != nil {
		t.Fatal(err)
	}
	open := func(assembly shell.Assembly) (*live.Runtime, *ui.Tester, *windowPreferences) {
		t.Helper()
		// Reopen both files, as another process would after an assembly change.
		window := &windowPreferences{directory: dir, write: shell.SavePrefs}
		root, err := live.NewWithAssembly(t.Context(), controller, preferences.Open(profilePath), assembly)
		if err != nil {
			t.Fatal(err)
		}
		t.Cleanup(func() { _ = root.Close() })
		root.App.SetPrefs(window.load())
		root.OpenSession(session.ID.String())
		tester := assemblyTester(root.App.View)
		tester.Frame()
		tester.Frame()
		return root, tester, window
	}
	save := func(root *live.Runtime, tester *ui.Tester, window *windowPreferences) {
		t.Helper()
		tester.Frame()
		window.save(root.App.Prefs())
		window.flush()
		if err := root.Close(); err != nil {
			t.Fatal(err)
		}
	}
	assembly := desktop.Full()
	full, tester, window := open(assembly)
	full.App.SetFeaturePreferences([]string{string(shell.ConversationPage), string(shell.GitPage), string(shell.FilesPage), string(shell.MarketPage)}, map[string]bool{
		string(shell.FilesPage): false, string(shell.GitPage): true, string(shell.MarketPage): true,
	})
	order, pins := full.App.FeaturePreferences()
	tab := panel.Tab{Kind: panel.Files, ID: assemblyFile}
	full.App.Panel.Show(tab)
	full.App.PanelOpen, full.App.DiffSplit = true, true
	full.App.PanelWidth, full.App.NavWidth = 420, 278
	full.App.Navigate(shell.GitPage)
	full.App.SetResourceTarget(shell.ResourceTarget{Node: controller.Target().String(), Project: session.Project.String()})
	if full.App.Page != shell.GitPage || full.App.ResourceTarget().Project == "" {
		t.Fatal("fixture did not select an independent Git route")
	}
	save(full, tester, window)
	if saved := shell.LoadPrefs(dir); saved.PanelOf != session.ID.String() || saved.PanelTab != tab || !saved.PanelOpen || !saved.DiffSplit {
		t.Fatalf("conversation panel did not reach %s: %+v", shell.PrefsFile, saved)
	}
	if saved := preferences.Open(profilePath); !saved.Readable || saved.Data.FeatureOrder == nil || !slices.Equal(*saved.Data.FeatureOrder, order) || saved.Data.FeaturePins == nil || !maps.Equal(*saved.Data.FeaturePins, pins) {
		t.Fatal("the rail choices did not reach the profile preferences")
	}
	checkConversation := func(app *shell.App, nav float32) {
		t.Helper()
		if app.Page != shell.ConversationPage || app.FocusedConversation() != session.ID.String() || app.Panel.Active != tab || !app.PanelOpen || !app.DiffSplit || app.PanelWidth != 420 || app.NavWidth != nav {
			t.Fatalf("new window did not restore the conversation layout: page=%q focus=%q layout=%+v", app.Page, app.FocusedConversation(), app.Prefs())
		}
	}
	omitted, tester, window := open(shell.Assembly{})
	checkConversation(omitted.App, 278)
	for _, page := range assembly.SettingsPages {
		if !full.App.Settings.HasSection(page.Section) || omitted.App.Settings.HasSection(page.Section) {
			t.Errorf("window settings assembly did not control availability of %q", page.Section)
		}
	}
	catalog := i18n.New(assembly.Locales...)
	for _, page := range assembly.WorkspacePages {
		omitted.App.Do(page.Feature.ID)
		omitted.App.Navigate(page.Page)
		if omitted.App.Page != shell.ConversationPage {
			t.Fatalf("the omitted route %q accepted navigation", page.Page)
		}
		omitted.App.Page = page.Page
		tester.Frame()
		if omitted.App.Page != shell.ConversationPage {
			t.Fatalf("the omitted route %q survived the frame", page.Page)
		}
		if _, found := tester.AccessByRole(ui.RoleButton, catalog.In("en", page.Feature.Label)); found {
			t.Errorf("the omitted route %q has a rail button", page.Page)
		}
	}
	omitted.App.NavWidth = 290
	omitted.App.Effects = false
	save(omitted, tester, window)
	if saved := preferences.Open(profilePath); !saved.Readable || saved.Data.ThemeEffects == nil || *saved.Data.ThemeEffects {
		t.Fatal("the omitted assembly did not save the profile preferences")
	}
	restored, tester, _ := open(assembly)
	checkConversation(restored.App, 290)
	if gotOrder, gotPins := restored.App.FeaturePreferences(); !slices.Equal(gotOrder, order) || !maps.Equal(gotPins, pins) {
		t.Fatalf("restored rail = %v and %v, want %v and %v", gotOrder, gotPins, order, pins)
	}
	for _, page := range assembly.WorkspacePages {
		restored.App.Do(page.Feature.ID)
		tester.Frame()
		if restored.App.Page != page.Page || restored.App.ResourceTarget() != (shell.ResourceTarget{}) {
			t.Fatalf("restored route %q opened with page %q and target %+v", page.Page, restored.App.Page, restored.App.ResourceTarget())
		}
		restored.App.ToggleSettings()
		restored.App.ToggleSettings()
		if restored.App.Page != page.Page || !restored.App.ClosePage() || restored.App.Page != shell.ConversationPage {
			t.Fatalf("Settings or Close Page did not return from the restored route %q", page.Page)
		}
	}
}
