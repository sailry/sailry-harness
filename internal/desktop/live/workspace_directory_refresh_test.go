package live

import (
	"context"
	"reflect"
	"testing"
	"testing/synctest"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type workspaceDirectoryRead struct {
	target protocol.NodeID
	input  protocol.CommandListDirectoryPayload
	done   chan link.Response
}

type workspaceDirectoryTransport struct {
	conversationTestTransport
	reads chan workspaceDirectoryRead
}

func (transport *workspaceDirectoryTransport) Dispatch(ctx context.Context, request protocol.Request) (link.Admission, error) {
	if input, ok := detail[protocol.CommandListDirectoryPayload](request.Command.Data); request.Command.Kind == "list_directory" && ok {
		done := make(chan link.Response, 1)
		select {
		case transport.reads <- workspaceDirectoryRead{request.Target, input, done}:
			return link.Admission{Receipt: protocol.Receipt{ID: request.ID}, Completion: done}, nil
		case <-ctx.Done():
			return link.Admission{}, ctx.Err()
		}
	}
	return transport.conversationTestTransport.Dispatch(ctx, request)
}

func TestWorkspaceDirectoryHostDeliversScopedPages(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		snapshot := protocol.DefaultSnapshot()
		node, project := protocol.NodeID{1}, protocol.ProjectID{2}
		main := protocol.Worktree{ID: protocol.WorktreeID{3}, Project: &project, Path: "/fixture/main", Main: true}
		linked := protocol.Worktree{ID: protocol.WorktreeID{4}, Project: &project, Path: "/fixture/linked"}
		snapshot.Node = node
		snapshot.Projects = []protocol.Project{{ID: project, Path: main.Path}}
		snapshot.Worktrees = []protocol.Worktree{main, linked}
		transport := &workspaceDirectoryTransport{conversationTestTransport: conversationTestTransport{snapshot: snapshot}, reads: make(chan workspaceDirectoryRead, 1)}
		controller := client.New(transport)
		if err := controller.Start(t.Context()); err != nil {
			t.Fatal(err)
		}
		defer controller.Close()
		r := &Runtime{ctx: t.Context(), controller: controller, App: shell.NewLiveWithAssembly(&viewmodel.Data{Workspace: &viewmodel.Workspace{}}, shell.Assembly{})}
		r.initWorkspace()
		defer r.closeWorkspace()
		r.workspace.selected = main.ID
		host := r.workspaceFor(linked).panel.FileDirectory

		cursor := &protocol.DirectoryCursor{Revision: "revision", Name: "first.txt"}
		type delivery struct {
			page protocol.Directory
			err  error
		}
		read := func(ctx context.Context, path string, after *protocol.DirectoryCursor) (workspaceDirectoryRead, <-chan delivery) {
			t.Helper()
			result := make(chan delivery, 1)
			host.Read(ctx, path, after, func(_ *ui.Context, page protocol.Directory, err error) { result <- delivery{page, err} })
			synctest.Wait()
			select {
			case call := <-transport.reads:
				if call.target != node || call.input.Worktree != linked.ID || call.input.Path != path || !reflect.DeepEqual(call.input.After, after) {
					t.Fatalf("directory request changed its captured scope or cursor: %+v, %+v", call.target, call.input)
				}
				return call, result
			default:
				t.Fatalf("no directory read for %q", path)
				return workspaceDirectoryRead{}, nil
			}
		}
		drain := func() {
			synctest.Wait()
			pluginViewRunQueue(r)
		}
		receive := func(result <-chan delivery) delivery {
			t.Helper()
			drain()
			select {
			case got := <-result:
				return got
			default:
				t.Fatal("directory response was not delivered")
				return delivery{}
			}
		}
		call, result := read(host.Context, "docs", cursor)
		page := protocol.Directory{Path: "docs", Entries: []protocol.FileEntry{{Name: "second.txt", Kind: protocol.EntryKindFile}}, Next: &protocol.DirectoryCursor{Revision: cursor.Revision, Name: "second.txt"}}
		call.done <- link.Response{Output: protocol.Output{Kind: "directory", Data: page}}
		if got := receive(result); got.err != nil || !reflect.DeepEqual(got.page, page) {
			t.Fatalf("directory page was changed in transit: %+v", got)
		}

		call, result = read(host.Context, "docs", page.Next)
		call.done <- link.Response{Fault: &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "fixture directory denied"}}
		if got := receive(result); host.Fault(got.err) != protocol.ErrorCodePermissionDenied || len(got.page.Entries) != 0 {
			t.Fatalf("denied read delivered file entries or the wrong fault: %+v", got)
		}

		ctx, cancel := context.WithCancel(host.Context)
		call, result = read(ctx, "docs", nil)
		cancel()
		call.done <- link.Response{Output: protocol.Output{Kind: "directory", Data: page}}
		drain()
		select {
		case got := <-result:
			t.Fatalf("closed directory read delivered a late result: %+v", got)
		default:
		}
	})
}
