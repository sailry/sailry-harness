package live

import (
	"context"
	"os"
	"path/filepath"
	"runtime"
	"slices"
	"testing"
	"testing/synctest"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/testassembly"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/link"
	nodefiles "github.com/sailry/sailry-harness/internal/node/files"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func registerConversationFiles(assembly *shell.Assembly) {
	assembly.RegisterLinkHandler(shell.LinkRoute{Kind: shell.LinkFile}, func(_ *ui.Context, target shell.LinkTarget, actions shell.LinkActions) error {
		destination := shell.LinkFileDestination{Page: shell.FilesPage}
		if target.ConversationWorkspace {
			destination = shell.LinkFileDestination{Panel: panel.Files}
		}
		return actions.OpenWorkspaceFile(destination)
	})
	assembly.RegisterLinkHandler(shell.LinkRoute{Kind: shell.LinkDirectory}, revealConversationTestDirectory)
}

func revealConversationTestDirectory(_ *ui.Context, _ shell.LinkTarget, actions shell.LinkActions) error {
	return actions.ShowWorkspaceDirectory(shell.FilesPage, (*panel.Project).RevealFileDirectory)
}

func conversationLinkSnapshot(node byte, directory string) (protocol.Snapshot, protocol.Session, protocol.Worktree) {
	snapshot := protocol.DefaultSnapshot()
	snapshot.Node = protocol.NodeID{node}
	project := protocol.DefaultProject()
	project.ID, project.Name, project.Path = protocol.ProjectID{1}, "Captured project", directory
	tree := protocol.Worktree{ID: protocol.WorktreeID{1}, Path: directory, Main: true, Project: &project.ID}
	session := protocol.DefaultSession()
	session.ID, session.Worktree, session.Project = protocol.SessionID{node}, tree.ID, &project.ID
	snapshot.Projects, snapshot.Worktrees, snapshot.Sessions = []protocol.Project{project}, []protocol.Worktree{tree}, []protocol.Session{session}
	return snapshot, session, tree
}

func conversationLinkRuntime(t *testing.T, assembly shell.Assembly, snapshot protocol.Snapshot) (*Runtime, *terminalTabsTestNode) {
	t.Helper()
	r, source := snapshotFixture(t, snapshot)
	r.App = shell.NewLiveWithAssembly(Sidebar(snapshot, r.Preferences.Data), assembly)
	r.App.Sidebar.Selected = snapshot.Sessions[0].ID.String()
	r.initWorkspace()
	if err := r.bindConversationLinks(); err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() {
		r.closeNativeWindow()
		r.closeWorkspace()
		r.closeAttachments()
	})
	return r, source
}

func conversationLinkWindow(t *testing.T, r *Runtime) (*ui.Tester, func(func(*ui.Context))) {
	t.Helper()
	procs := runtime.GOMAXPROCS(1)
	t.Cleanup(func() { runtime.GOMAXPROCS(procs) })
	var step func(*ui.Context)
	window := ui.NewTester(func(c *ui.Context) {
		r.mu.Lock()
		queued := r.queue
		r.queue = nil
		r.mu.Unlock()
		for _, apply := range queued {
			apply(c)
		}
		if step != nil {
			step(c)
			step = nil
		}
		r.frameNativeWindowResources()
		kit.ToastHost(c)
	}, 520, 350)
	window.SetLocale("en")
	return window, func(action func(*ui.Context)) {
		step = action
		for range 4 {
			window.Frame()
			synctest.Wait()
		}
	}
}

func conversationLinkFixtureDirectory(t *testing.T) string {
	t.Helper()
	root, err := filepath.EvalSymlinks(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(root, "README.md"), []byte("fixture"), 0o600); err != nil {
		t.Fatal(err)
	}
	if err := os.Mkdir(filepath.Join(root, "docs"), 0o700); err != nil {
		t.Fatal(err)
	}
	return root
}

func conversationLinkLocalResponse(directory string, worktree protocol.WorktreeID) func(protocol.Request) link.Response {
	return func(request protocol.Request) link.Response {
		if input, ok := detail[protocol.CommandListDirectoryPayload](request.Command.Data); request.Command.Kind == "list_directory" && ok && input.Worktree == worktree {
			out, err := nodefiles.Inspect(context.Background(), directory, protocol.Command{Kind: request.Command.Kind, Data: &input}, core.ManualFiles, nil)
			if err == nil {
				return link.Response{Output: out}
			}
			return link.Response{Fault: &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "fixture listing denied"}}
		}
		return link.Response{Fault: &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "fixture command is unavailable"}}
	}
}

func TestConversationFileLinkDefaultsRevealLocal(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		directory := conversationLinkFixtureDirectory(t)
		snapshot, session, tree := conversationLinkSnapshot(1, directory)
		r, source := conversationLinkRuntime(t, shell.Assembly{}, snapshot)
		source.respond = conversationLinkLocalResponse(directory, tree.ID)
		var revealed, opened []string
		r.SetNativeFileActions(NativeFileActions{LocalNode: snapshot.Node,
			RevealLocal: func(path string) { revealed = append(revealed, path) },
			OpenLocal:   func(path string) { opened = append(opened, path) },
		})
		window, run := conversationLinkWindow(t, r)
		for _, link := range []string{"README.md#L42", "docs", "./"} {
			run(func(c *ui.Context) { r.App.Messages.OpenLink(c, session.ID.String(), link) })
		}
		ref := protocol.ConversationReference{Target: protocol.ConversationReferenceTarget{Kind: "file", Data: "README.md"}}
		r.openReference(session.ID.String(), protocol.TurnID{}, ref)
		run(nil)
		if want := []string{filepath.Join(directory, "README.md"), filepath.Join(directory, "docs"), directory, filepath.Join(directory, "README.md")}; !slices.Equal(revealed, want) || len(opened) != 0 || len(window.OpenedURLs()) != 0 || len(r.App.Panel.Tabs) != 0 {
			t.Fatalf("default link actions: revealed %v, opened %v, tabs %v, URLs %v", revealed, opened, r.App.Panel.Tabs, window.OpenedURLs())
		}
	})
}

func TestConversationFileLinkRegisteredUsesExactNodeWorktreeAndLine(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		const alternatePanel = panel.Kind("fixture-file-panel")
		var captured shell.LinkTarget
		var owner *Runtime
		var openLine int
		destination := shell.LinkFileDestination{Panel: alternatePanel}
		assembly := shell.Assemble(testassembly.RegisterFilesFixture, func(a *shell.Assembly) {
			a.RegisterPanelResource(panel.Resource{Kind: alternatePanel, Label: "files", Icon: panel.ResourceOf(panel.Files).Icon,
				Render: func(*ui.Context, *panel.State, panel.Tab) {}})
			a.RegisterLinkHandler(shell.LinkRoute{Kind: shell.LinkFile}, func(_ *ui.Context, target shell.LinkTarget, actions shell.LinkActions) error {
				captured = target
				if err := actions.OpenWorkspaceFile(destination); err != nil {
					return err
				}
				project := owner.workspace.projects[target.Worktree]
				if project == nil || project.openingFile == nil {
					t.Fatal("captured workspace did not start the shared editor read")
				}
				openLine = project.openingFile.line
				if destination.Panel != "" {
					if err := actions.OpenWorkspaceFile(shell.LinkFileDestination{Page: shell.FilesPage}); faultCode(err) != protocol.ErrorCodeUnavailable {
						t.Fatalf("one click admitted a second presentation: %v", err)
					}
				}
				return nil
			})
			a.RegisterLinkHandler(shell.LinkRoute{Kind: shell.LinkDirectory}, revealConversationTestDirectory)
		})
		local, _, localTree := conversationLinkSnapshot(1, "/fixture/local")
		remote, session, tree := conversationLinkSnapshot(2, "/fixture/remote")
		tree.ID, tree.Path, tree.Main = protocol.WorktreeID{2}, "/fixture/remote-topic", false
		localTree.ID, localTree.Path, localTree.Main = tree.ID, "/fixture/local-topic", false
		local.Worktrees = append(local.Worktrees, localTree)
		remote.Worktrees = append(remote.Worktrees, tree)
		session.Worktree = tree.ID
		remote.Sessions[0] = session
		root, localCalls := conversationLinkRuntime(t, assembly, local)
		var remoteCalls *terminalTabsTestNode
		owner, remoteCalls = conversationLinkRuntime(t, shell.Assembly{}, remote)
		root.App.Data.Projects = append(root.App.Data.Projects, owner.App.Data.Projects...)
		owner.parent, owner.App = root, root.App
		root.bindings = map[string]*Runtime{remote.Node.String(): owner}
		const filename, pageFile = "README.md", "guide.md"
		remoteCalls.respond = func(request protocol.Request) link.Response {
			switch request.Command.Kind {
			case "list_directory":
				input, _ := detail[protocol.CommandListDirectoryPayload](request.Command.Data)
				return link.Response{Output: protocol.Output{Kind: "directory", Data: protocol.Directory{Path: input.Path, Entries: []protocol.FileEntry{
					{Name: "docs", Kind: protocol.EntryKindDirectory}, {Name: filename, Kind: protocol.EntryKindFile}, {Name: pageFile, Kind: protocol.EntryKindFile},
				}}}}
			case "read_file":
				input, _ := detail[protocol.CommandReadFilePayload](request.Command.Data)
				return link.Response{Output: protocol.Output{Kind: "file_content", Data: protocol.FileContent{Path: input.Path, Text: "fixture file content"}}}
			}
			return link.Response{Fault: &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "fixture command is unavailable"}}
		}
		unexpected := func(path string) { t.Errorf("remote link opened path on this device: %s", path) }
		root.SetNativeFileActions(NativeFileActions{LocalNode: local.Node, RevealLocal: unexpected, OpenLocal: unexpected})
		window, run := conversationLinkWindow(t, root)
		if bound, cv := root.conversationLinkOwner(session.ID.String()); bound != owner {
			t.Fatalf("fixture has no route to its remote conversation: binding %p, conversation %+v", bound, cv)
		}
		run(func(c *ui.Context) { root.openChatLink(c, session.ID.String(), filename+":42") })
		if captured.Node != remote.Node || captured.Session != session.ID.String() || captured.Worktree != tree.ID || captured.Path != filename || captured.Line != 42 || openLine != 42 {
			t.Fatalf("resolved target = %+v, editor line %d; labels %q", captured, openLine, window.Texts())
		}
		wantTabs := []panel.Tab{{Kind: alternatePanel, ID: filename}}
		if project := owner.selectedWorkspace(); project == nil || project.panel.Files.File() != filename || !slices.Equal(root.App.AllPanelTabs(), wantTabs) || root.App.Page != shell.ConversationPage {
			t.Fatal("file did not open only in the explicitly selected panel")
		}
		destination = shell.LinkFileDestination{Page: shell.FilesPage}
		run(func(c *ui.Context) { root.openChatLink(c, session.ID.String(), pageFile+"#L7") })
		if target := root.App.ResourceTarget(); root.App.Page != shell.FilesPage || target.Node != remote.Node.String() || target.Worktree != tree.ID.String() || openLine != 7 ||
			root.shownResourceWorkspace() != owner.selectedWorkspace() || owner.selectedWorkspace().panel.Files.File() != pageFile || !slices.Equal(root.App.AllPanelTabs(), wantTabs) {
			t.Fatalf("file did not use only the explicitly selected page: %v, %+v, tabs %v, line %d", root.App.Page, target, root.App.AllPanelTabs(), openLine)
		}
		commands := remoteCalls.sent("read_file")
		if len(commands) != 2 || len(localCalls.sent("read_file")) != 0 || len(localCalls.sent("list_directory")) != 0 {
			t.Fatalf("file read escaped the captured Node or opened twice: %+v", commands)
		}
		for index, want := range []string{filename, pageFile} {
			if read, ok := detail[protocol.CommandReadFilePayload](commands[index].Data); !ok || read.Worktree != tree.ID || read.Path != want {
				t.Fatalf("read did not retain its worktree: %+v", read)
			}
		}
		run(func(c *ui.Context) { root.openChatLink(c, session.ID.String(), "docs/") })
		project := owner.selectedWorkspace()
		if target := root.App.ResourceTarget(); root.App.Page != shell.FilesPage || target.Node != remote.Node.String() || target.Worktree != tree.ID.String() ||
			project == nil || root.shownResourceWorkspace() != project || !slices.Equal(root.App.AllPanelTabs(), wantTabs) {
			t.Fatalf("directory did not open Code Files at the captured location: %v, %+v", root.App.Page, target)
		}
		if directory := project.panel.TakeFileDirectoryReveal(); directory == nil || *directory != "docs" {
			t.Fatalf("Files did not receive the captured directory: %v", directory)
		}
	})
}

func TestConversationFileLinkRemoteDefaultNeverUsesOS(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		snapshot, session, tree := conversationLinkSnapshot(2, "/fixture/remote")
		r, source := conversationLinkRuntime(t, shell.Assembly{}, snapshot)
		source.respond = func(request protocol.Request) link.Response {
			if input, ok := detail[protocol.CommandListDirectoryPayload](request.Command.Data); ok {
				return link.Response{Output: protocol.Output{Kind: "directory", Data: protocol.Directory{Path: input.Path, Entries: []protocol.FileEntry{
					{Name: "docs", Kind: protocol.EntryKindDirectory}, {Name: "README.md", Kind: protocol.EntryKindFile},
				}}}}
			}
			return link.Response{Fault: &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "fixture download denied"}}
		}
		unexpected := func(path string) { t.Errorf("remote path was sent to the OS: %s", path) }
		r.SetNativeFileActions(NativeFileActions{LocalNode: protocol.NodeID{1}, RevealLocal: unexpected, OpenLocal: unexpected})
		window, run := conversationLinkWindow(t, r)
		run(func(c *ui.Context) { r.openChatLink(c, session.ID.String(), "file:///outside.txt") })
		if !window.HasText(i18n.In("en", "files_remote_path")) {
			t.Fatal("out-of-worktree remote path did not report that it is unavailable")
		}
		run(func(c *ui.Context) { r.openChatLink(c, session.ID.String(), "docs/") })
		if !window.HasText(i18n.In("en", "unavailable")) {
			t.Fatal("unhandled remote directory did not report that it is unavailable")
		}
		run(func(c *ui.Context) { r.openChatLink(c, session.ID.String(), "README.md") })
		requests := source.sent("download_file")
		if len(requests) != 1 || len(window.OpenedURLs()) != 0 {
			t.Fatalf("remote preview did not use one scoped file request: %+v", requests)
		}
		if payload, ok := detail[protocol.CommandDownloadFilePayload](requests[0].Data); !ok || payload.Worktree != tree.ID || payload.Path != "README.md" {
			t.Fatalf("remote preview lost its source: %+v", payload)
		}
	})
}

func TestConversationFileLinkFailuresDoNotFallback(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		directory := conversationLinkFixtureDirectory(t)
		snapshot, session, tree := conversationLinkSnapshot(1, directory)
		r, source := conversationLinkRuntime(t, shell.Assemble(testassembly.RegisterFilesFixture), snapshot)
		source.respond = conversationLinkLocalResponse(directory, tree.ID)
		var revealed []string
		r.SetNativeFileActions(NativeFileActions{LocalNode: snapshot.Node, RevealLocal: func(path string) { revealed = append(revealed, path) },
			OpenLocal: func(path string) { t.Errorf("unexpected system file open: %s", path) }})
		var saved func(shell.LinkFileDestination) error
		var attempts int
		release, err := r.App.RegisterLinkHandler(shell.LinkRoute{Kind: shell.LinkFile}, func(_ *ui.Context, _ shell.LinkTarget, actions shell.LinkActions) error {
			attempts++
			saved = actions.OpenWorkspaceFile
			return fileActionFault(protocol.ErrorCodePermissionDenied, "fixture handler denied the link")
		})
		if err != nil {
			t.Fatal(err)
		}
		window, run := conversationLinkWindow(t, r)
		click := func(link string) { run(func(c *ui.Context) { r.openChatLink(c, session.ID.String(), link) }) }
		click("README.md")
		if attempts != 1 || saved == nil || len(revealed) != 0 || !window.HasText(i18n.In("en", "files_open_failed")) {
			t.Fatalf("rejected link fell through: handler %d, revealed %v, labels %q", attempts, revealed, window.Texts())
		}
		late := make(chan error, 1)
		go func() { late <- saved(shell.LinkFileDestination{Page: shell.FilesPage}) }()
		if err := <-late; faultCode(err) != protocol.ErrorCodeUnavailable {
			t.Fatalf("retained callback remained callable: %v", err)
		}
		release()
		for _, destination := range []shell.LinkFileDestination{{Page: shell.FilesPage, Panel: panel.Files}, {Panel: "not-registered"}} {
			release, err := r.App.RegisterLinkHandler(shell.LinkRoute{Kind: shell.LinkFile}, func(_ *ui.Context, _ shell.LinkTarget, actions shell.LinkActions) error {
				err := actions.OpenWorkspaceFile(destination)
				if faultCode(err) != protocol.ErrorCodeUnavailable || faultCode(actions.OpenWorkspaceFile(shell.LinkFileDestination{Page: shell.FilesPage})) != protocol.ErrorCodeUnavailable {
					t.Fatalf("invalid or unavailable destination allowed another presentation: %v, %v", destination, err)
				}
				return err
			})
			if err != nil {
				t.Fatal(err)
			}
			click("README.md")
			release()
		}
		if r.App.Page != shell.ConversationPage || len(r.App.AllPanelTabs()) != 0 || len(source.sent("read_file")) != 0 || len(revealed) != 0 {
			t.Fatal("an invalid or unavailable destination opened another surface")
		}
		before := len(source.sent("list_directory"))
		for _, scheme := range []string{"javascript", "data"} {
			if _, err := r.App.RegisterLinkHandler(shell.LinkRoute{Kind: shell.LinkScheme, Name: scheme}, func(*ui.Context, shell.LinkTarget, shell.LinkActions) error {
				t.Error("unsafe scheme reached a declared handler")
				return nil
			}); err != nil {
				t.Fatal(err)
			}
		}
		for _, scheme := range []string{"JaVaScRiPt:12", "data:12", "data:text/html,unsafe", "blob:12", "../outside.txt", "file://other-node/README.md"} {
			click(scheme)
		}
		if len(revealed) != 0 || len(window.OpenedURLs()) != 0 || len(source.sent("list_directory")) != before {
			t.Fatal("a blocked URI or workspace escape was treated as a file or system URL")
		}
		outside := filepath.Join(conversationLinkFixtureDirectory(t), "README.md")
		if err := os.Symlink(outside, filepath.Join(directory, "escape.md")); err != nil {
			if runtime.GOOS != "windows" {
				t.Fatal(err)
			}
		} else {
			click("escape.md")
			if len(revealed) != 0 {
				t.Fatalf("a symlink escaped the workspace: %v", revealed)
			}
		}
		source.respond = func(protocol.Request) link.Response {
			return link.Response{Fault: &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "fixture Node denied the link"}}
		}
		click("README.md")
		if len(revealed) != 0 {
			t.Fatalf("a Node refusal fell back to the filesystem: %v", revealed)
		}
		source.respond = conversationLinkLocalResponse(directory, tree.ID)
		click("README.md")
		if attempts != 1 || !slices.Equal(revealed, []string{filepath.Join(directory, "README.md")}) {
			t.Fatalf("release did not restore the absent-handler default: handler %d, revealed %v", attempts, revealed)
		}
	})
}

func TestConversationFileLinkReplacesPendingManagedPresentation(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		directory := conversationLinkFixtureDirectory(t)
		snapshot, session, tree := conversationLinkSnapshot(1, directory)
		r, source := conversationLinkRuntime(t, shell.Assemble(testassembly.RegisterFilesFixture), snapshot)
		list := conversationLinkLocalResponse(directory, tree.ID)
		source.respond = func(request protocol.Request) link.Response {
			if request.Command.Kind == "read_file" {
				return link.Response{Fault: &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "binary file cannot be displayed as text"}}
			}
			return list(request)
		}
		unexpected := func(string) { t.Error("the declared presentation used the old system opener") }
		r.SetNativeFileActions(NativeFileActions{LocalNode: snapshot.Node, OpenLocal: unexpected, RevealLocal: unexpected})
		if _, err := r.App.RegisterLinkHandler(shell.LinkRoute{Kind: shell.LinkFile}, func(_ *ui.Context, target shell.LinkTarget, actions shell.LinkActions) error {
			project := r.conversationLinkWorkspace(target)
			if project == nil {
				t.Fatal("fixture has no workspace")
			}
			project.openManagedFile(target.Path, 0)
			if project.openingFile == nil || !project.openingFile.managed {
				t.Fatal("no pending explorer open")
			}
			return actions.OpenWorkspaceFile(shell.LinkFileDestination{Panel: panel.Files})
		}); err != nil {
			t.Fatal(err)
		}
		_, run := conversationLinkWindow(t, r)
		run(func(c *ui.Context) { r.openChatLink(c, session.ID.String(), "README.md#L5") })
		if len(source.sent("read_file")) != 1 || len(source.sent("download_file")) != 0 || len(r.App.AllPanelTabs()) != 0 {
			t.Fatal("a refused coalesced read opened another presentation")
		}
	})
}

func TestConversationFileLinkRetiresWithScope(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		directory := conversationLinkFixtureDirectory(t)
		snapshot, session, tree := conversationLinkSnapshot(1, directory)
		r, source := conversationLinkRuntime(t, shell.Assembly{}, snapshot)
		source.respond = conversationLinkLocalResponse(directory, tree.ID)
		r.SetNativeFileActions(NativeFileActions{LocalNode: snapshot.Node, RevealLocal: func(path string) {
			t.Errorf("a retired conversation revealed %s", path)
		}})
		_, run := conversationLinkWindow(t, r)
		run(func(c *ui.Context) {
			r.openChatLink(c, session.ID.String(), "README.md")
			if r.pendingFileLink == nil {
				t.Fatal("no scoped read was started")
			}
			r.App.Navigate(shell.SettingsPage)
		})
		if r.pendingFileLink != nil {
			t.Fatal("a link read survived leaving its scope")
		}
	})
}

func TestConversationLinkSchemesCoexist(t *testing.T) {
	var routes []string
	assembly := shell.Assemble(func(a *shell.Assembly) {
		for _, name := range []string{"example-module", "sailry-knowledge"} {
			a.RegisterLinkHandler(shell.LinkRoute{Kind: shell.LinkScheme, Name: name}, func(_ *ui.Context, target shell.LinkTarget, _ shell.LinkActions) error {
				routes = append(routes, target.URL)
				return nil
			})
		}
	})
	r := &Runtime{App: shell.NewLiveWithAssembly(&viewmodel.Data{Workspace: &viewmodel.Workspace{}}, assembly)}
	ran := false
	window := ui.NewTester(func(c *ui.Context) {
		if ran {
			return
		}
		ran = true
		if r.openChatLink(c, "", "#anchor") {
			t.Fatal("transcript anchor was consumed by the window router")
		}
		for _, link := range []string{"example-module://destination", "sailry-knowledge://collection/chunk?revision=1", "https://example.invalid/", "mailto:example@example.invalid", "tel:1234"} {
			r.openChatLink(c, "", link)
		}
	}, 320, 240)
	window.Frame()
	if !slices.Equal(routes, []string{"example-module://destination", "sailry-knowledge://collection/chunk?revision=1"}) ||
		!slices.Equal(window.OpenedURLs(), []string{"https://example.invalid/", "mailto:example@example.invalid", "tel:1234"}) {
		t.Fatalf("registered and default scheme routes did not coexist: %v, %v", routes, window.OpenedURLs())
	}
}
