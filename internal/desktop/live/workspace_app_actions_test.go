package live

import (
	"encoding/json"
	"sync"
	"testing"
	"testing/synctest"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestWorkspaceAppActionSharesTheScopedFileOpen(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		snapshot, session, tree := conversationLinkSnapshot(1, t.TempDir())
		root, transport := conversationLinkRuntime(t, shell.Assembly{}, snapshot)
		if err := root.bindWorkspaceAppActions(); err != nil {
			t.Fatal(err)
		}
		if release, err := root.App.RegisterAppAction(shell.AppAction{ID: workspaceOpenFileAction,
			Run: func(*ui.Context, *shell.App, shell.AppActionContext, json.RawMessage) error {
				t.Error("a duplicate action opened a file")
				return nil
			},
		}); release != nil || faultCode(err) != protocol.ErrorCodeRevisionConflict {
			t.Fatalf("duplicate file action was accepted: %v", err)
		}
		var privateRuns int
		if _, err := root.App.RegisterAppAction(shell.AppAction{ID: "fixture.schedule",
			Run: func(*ui.Context, *shell.App, shell.AppActionContext, json.RawMessage) error {
				privateRuns++
				return nil
			},
		}); err != nil {
			t.Fatal(err)
		}
		late := make(chan struct{})
		releaseLate := sync.OnceFunc(func() { close(late) })
		defer releaseLate()
		transport.respond = func(request protocol.Request) link.Response {
			input, valid := detail[protocol.CommandReadFilePayload](request.Command.Data)
			if request.Command.Kind != "read_file" || !valid || input.Worktree != tree.ID {
				return link.Response{Fault: &protocol.Fault{Code: protocol.ErrorCodeUnavailable}}
			}
			if input.Path == "denied.txt" {
				return link.Response{Fault: &protocol.Fault{Code: protocol.ErrorCodePermissionDenied}}
			}
			if input.Path == "late.txt" {
				<-late
			}
			return link.Response{Output: protocol.Output{Kind: "file_content", Data: protocol.FileContent{Path: input.Path, Text: "first\nsecond\n"}}}
		}
		root.App.Hooks.WorkspaceProject = root.workspaceProject
		root.workspaceFrame(nil)
		root.overviews = map[protocol.SessionID]*overviewRead{session.ID: {open: true}}
		window, run := conversationLinkWindow(t, root)
		rows := overviewAssets([]protocol.ConversationAssetsGroup{{
			Sequence: 1, Worktree: tree.ID, Kind: protocol.ConversationAssetsKindResource,
			Items: []protocol.ConversationAssetsTarget{{Kind: "file", Data: protocol.ConversationAssetsTargetFilePayload{Path: "overview.txt"}}},
		}})
		run(func(*ui.Context) { root.openOverviewAsset(session, rows[0]) })
		assertOpen := func(relative string, reads int) {
			t.Helper()
			want := panel.Tab{Kind: panel.Files, ID: relative}
			if root.App.Panel.Active != want || !root.App.PanelOpen || len(transport.sent("read_file")) != reads || len(window.OpenedURLs()) != 0 {
				t.Fatalf("expected %v and %d scoped reads; active %v, reads %v, external %v", want, reads, root.App.Panel.Active, transport.sent("read_file"), window.OpenedURLs())
			}
		}
		assertOpen("overview.txt", 1)
		permit := true
		source := root.App.BindAppNavigation(shell.AppActionScope{Node: snapshot.Node, Worktree: tree.ID, Session: session.ID},
			func() bool { return true }, func(action protocol.PluginAction) bool { return permit && action == protocol.PluginActionReadFiles })
		defer source.Close()
		invoke := func(body string, code protocol.ErrorCode) {
			t.Helper()
			run(func(view *ui.Context) {
				if err := source.Invoke(view, workspaceOpenFileAction, json.RawMessage(body)); faultCode(err) != code || code == "" && err != nil {
					t.Fatalf("%s: want %q, got %v", body, code, err)
				}
			})
		}
		invoke(`{"path":"plugin.txt","line":2}`, "")
		assertOpen("plugin.txt", 2)
		for _, body := range []string{`{"path":"../plugin.txt"}`, `{"path":"plugin.txt","line":-1}`, `{"path":"plugin.txt","worktree":"other"}`, `{"path":"one","path":"two"}`} {
			invoke(body, protocol.ErrorCodeInvalidRequest)
		}
		permit = false
		invoke(`{"path":"forbidden.txt"}`, protocol.ErrorCodePermissionDenied)
		assertOpen("plugin.txt", 2)
		permit = true
		invoke(`{"path":"denied.txt"}`, "") // The scoped read itself fails asynchronously.
		assertOpen("plugin.txt", 3)
		invoke(`{"path":"late.txt"}`, "")
		assertOpen("plugin.txt", 4)
		source.Close()
		releaseLate()
		invoke(`{"path":"not-opened.txt"}`, protocol.ErrorCodeUnavailable)
		run(func(view *ui.Context) {
			root.App.Messages.OpenLink(view, session.ID.String(), "fixture.schedule:run")
		})
		assertOpen("plugin.txt", 4)
		if privateRuns != 0 {
			t.Fatal("conversation text invoked a private app action")
		}
	})
}
