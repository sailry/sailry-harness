package live

import (
	"slices"
	"sync"
	"testing"
	"testing/synctest"
	"time"

	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestDraftProjectSelectionReadFailure(t *testing.T) {
	for _, test := range []struct {
		name   string
		linked bool
		fault  protocol.Fault
	}{
		{name: "missing file in main checkout", fault: protocol.Fault{Code: protocol.ErrorCodeNotFound, Message: "path not found"}},
		{name: "permission denied in confirmed linked checkout", linked: true, fault: protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "permission denied"}},
	} {
		t.Run(test.name, func(t *testing.T) {
			synctest.Test(t, func(t *testing.T) {
				testDraftProjectSelectionReadFailure(t, &test.fault, nil, test.linked, []string{"workspace_action_failed"})
			})
		})
	}
}

// The file path exercises an explicit shared-host read without a Files page.
// The inspection path feeds an already-classified fault to the Desktop diagnostic sink.
func testDraftProjectSelectionReadFailure(t *testing.T, fileFault, inspectionFault *protocol.Fault, linked bool, want []string) {
	t.Helper()
	var assembly shell.Assembly
	if inspectionFault != nil {
		registerGitPanelFixture(&assembly)
	}
	fixture := newPluginLocationTestFixture(t, assembly)
	root := fixture.root
	root.busy, root.uncertain = map[string]bool{}, map[string]protocol.Request{}
	root.App.Data.Projects = Sidebar(root.snapshot(), root.Preferences.Data).Projects
	first, second := root.App.Composer("draft/first"), root.App.Composer("draft/second")
	first.Draft, first.Permission = "Keep this input", composer.ProjectAccess
	second.Draft, second.Permission = "Leave the visible draft alone", composer.FullAccess

	const path = "notes.txt"
	var mu sync.Mutex
	var sent []protocol.Request
	requests := func() []protocol.Request {
		mu.Lock()
		defer mu.Unlock()
		return slices.Clone(sent)
	}
	fixture.source.dispatch = func(request protocol.Request) (link.Admission, error) {
		mu.Lock()
		sent = append(sent, request)
		mu.Unlock()
		var fault *protocol.Fault
		switch request.Command.Kind {
		case "read_file":
			fault = fileFault
		case "inspect_git":
			fault = inspectionFault
		}
		if fault == nil {
			fault = &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "unexpected fixture command"}
		}
		completion := make(chan link.Response, 1)
		completion <- link.Response{Fault: fault}
		return link.Admission{Receipt: protocol.Receipt{ID: request.ID}, Completion: completion}, nil
	}

	selectLocation := func(selection map[string]any) error {
		t.Helper()
		body, err := fixture.call(t, "sailry/ui.selectLocation", pluginLocationTestArguments(t, "1", selection))
		if err == nil && string(body) != "true" {
			t.Fatalf("location selection was not applied: %s", body)
		}
		return err
	}
	for _, selection := range []map[string]any{
		{"project": protocol.ProjectID{9}}, {"worktree": fixture.target.ID},
	} {
		if err := selectLocation(selection); faultCode(err) != protocol.ErrorCodePermissionDenied {
			t.Fatalf("uncaptured location %v = %v, want permission_denied", selection, err)
		}
	}
	if err := selectLocation(map[string]any{"project": fixture.main.Project}); err != nil {
		t.Fatalf("select captured project: %v", err)
	}
	if first.Project != fixture.main.Project.String() || first.Worktree != fixture.main.Path || root.App.Sidebar.Selected != "draft/second" || len(fixture.source.commandKinds()) != 0 {
		t.Fatalf("project selection changed focus, used another location, or sent a Node command: %q %q %v", first.Project, first.Worktree, fixture.source.commandKinds())
	}
	selectedTree := fixture.main
	if linked {
		request := fixture.prepare(t, protocol.Command{Kind: "register_worktree", Data: protocol.CommandRegisterWorktreePayload{Project: *fixture.main.Project, Path: fixture.target.Path}})
		fixture.source.outcome = pluginLocationTestOutcome(protocol.Output{Kind: "worktree", Data: fixture.target})
		if err := selectLocation(map[string]any{"worktree": fixture.target.ID, "request": request.ID}); err != nil {
			t.Fatalf("select confirmed linked checkout: %v", err)
		}
		if got := fixture.source.commandKinds(); !slices.Equal(got, []string{"inspect_request"}) {
			t.Fatalf("handoff must inspect exactly its original request without replay: %v", got)
		}
		if _, err := fixture.call(t, "sailry/ui.readLocation", nil); faultCode(err) != protocol.ErrorCodePermissionDenied {
			t.Fatalf("former capture could read the retargeted draft: %v", err)
		}
		selectedTree = fixture.target
	}
	scope := composerPluginScope(root, root.App.Data.Find("draft/first"), first)
	if scope.Worktree == nil || *scope.Worktree != selectedTree.ID || scope.Session != nil {
		t.Fatalf("draft captured the wrong workspace: %+v", scope)
	}

	root.App.Sidebar.Selected = "draft/first"
	window := closeTestWindow(root)
	settle := func() {
		for range 3 {
			root.workspaceFrame(nil)
			synctest.Wait()
			window.Frame()
		}
	}
	settle()
	project := root.selectedWorkspace()
	if project == nil || project.owner.ID != selectedTree.ID {
		t.Fatal("the draft did not show its selected worktree")
	}
	if fileFault != nil {
		if got := requests(); len(got) != 0 {
			t.Fatalf("a draft without workspace pages opened resources: %+v", got)
		}
		project.readFile(path)
		project.readFile(path)
		settle()
		project.readFile(path)
		settle()
		if got := requests(); len(got) != 1 || !project.failedReads["file/"+path] || project.panel.FileLoaded(path) || project.entryIsMissing(path) != (fileFault.Code == protocol.ErrorCodeNotFound) {
			t.Fatalf("file read failure was not retained accurately or was replayed: %+v", got)
		}
	} else {
		before := len(requests())
		project.readGit()
		settle()
		if before == 0 || len(requests()) <= before || project.statusKnown || !project.repositoryUnavailable {
			t.Fatal("the repeated inspection did not deliver its failure to the host")
		}
	}
	assertToasts := func() {
		t.Helper()
		texts := window.Texts()
		for _, name := range want {
			if n := countText(texts, i18n.In("en", name)); n != 1 {
				t.Fatalf("%s appeared %d times: %q", name, n, texts)
			}
		}
		if inspectionFault != nil && window.HasText(i18n.In("en", "workspace_action_failed")) {
			t.Fatalf("an inspection was reported as a failed mutation: %q", texts)
		}
	}
	assertToasts()
	if fileFault != nil {
		window.Advance(time.Minute)
		settle()
		if window.HasText(i18n.In("en", want[0])) {
			t.Fatal("the original toast did not retire")
		}
		project.refresh()
		project.readFile(path)
		settle()
		if got := requests(); len(got) != 2 || got[0].ID == got[1].ID {
			t.Fatalf("explicit retry did not issue one new scoped read: %+v", got)
		}
		assertToasts()
	}

	for _, request := range requests() {
		if request.Target != root.controller.Target() || request.Plugin != nil {
			t.Fatalf("workspace read escaped its Node or reused plugin authority: %+v", request)
		}
		if fileFault != nil {
			read, ok := detail[protocol.CommandReadFilePayload](request.Command.Data)
			if request.Command.Kind != "read_file" || !ok || read.Worktree != selectedTree.ID || read.Path != path {
				t.Fatalf("explicit read used the wrong resource: %+v", request.Command)
			}
		} else {
			read, ok := detail[protocol.CommandInspectGitPayload](request.Command.Data)
			if request.Command.Kind != "inspect_git" || !ok || read.Worktree != selectedTree.ID {
				t.Fatalf("diagnostic inspection used the wrong resource: %+v", request.Command)
			}
		}
	}
	if first.Draft != "Keep this input" || first.Permission != composer.ProjectAccess || first.Project != fixture.main.Project.String() || first.Worktree != selectedTree.Path ||
		second.Draft != "Leave the visible draft alone" || second.Permission != composer.FullAccess || second.Worktree != fixture.main.Path {
		t.Fatal("the failure changed a draft's input, permission, or location")
	}
}

func countText(texts []string, text string) int {
	count := 0
	for _, value := range texts {
		if value == text {
			count++
		}
	}
	return count
}
