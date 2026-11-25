package live

import (
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/preferences"
	"github.com/sailry/sailry-harness/internal/desktop/testassembly"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// An idle frame reuses the composer catalog; a composer choice rebuilds it.
func TestComposerCatalogKeptAcrossIdleFrames(t *testing.T) {
	node, project, worktree, provider := protocol.NodeID{1}, protocol.ProjectID{2}, protocol.WorktreeID{3}, protocol.ProviderID{4}
	shown, other := protocol.SessionID{5}, protocol.SessionID{6}
	config := protocol.DefaultSessionConfig()
	config.Provider, config.Model, config.Mode, config.Permission = provider, "fixture", "code", "ask"
	snapshot := protocol.DefaultSnapshot()
	snapshot.Node = node
	snapshot.Projects = []protocol.Project{{ID: project, Name: "Workspace", Path: "/fixture/project"}}
	snapshot.Worktrees = []protocol.Worktree{{ID: worktree, Project: &project, Path: "/fixture/worktree", Main: true}}
	snapshot.Providers = []protocol.ConversationProvider{{ID: provider, Revision: 7, Enabled: true, Name: "Provider", Api: "responses", Authentication: "api_key", Models: []protocol.ConversationModel{{ID: "fixture", DefaultEffort: protocol.Effort{Kind: "default"}}}}}
	snapshot.Sessions = []protocol.Session{
		{ID: shown, Revision: 1, Worktree: worktree, Project: &project, Config: config, Roles: protocol.DefaultRoleSnapshot()},
		{ID: other, Revision: 1, Worktree: worktree, Project: &project, Config: config, Roles: protocol.DefaultRoleSnapshot()},
	}
	snapshot.Sessions[1].Activity.Title = "Sibling session"
	transport := &conversationTestTransport{snapshot: snapshot}
	transport.handle = func(request protocol.Request) (link.Response, bool) {
		switch request.Command.Kind {
		case "read_resource_catalog":
			source := protocol.ResourceSource{Node: node, Route: protocol.RouteKey{Entry: node, Target: node}, Projects: snapshot.Projects, Worktrees: snapshot.Worktrees}
			return link.Response{Output: protocol.Output{Kind: "resource_catalog", Data: protocol.ResourceCatalog{Entry: node, Sources: []protocol.ResourceSource{source}}}}, true
		case "inspect_git":
			return link.Response{Output: protocol.Output{Kind: "git_status", Data: protocol.GitStatus{Kind: protocol.RepositoryKindDirectory}}}, true
		case "list_worktrees":
			return link.Response{Output: protocol.Output{Kind: "git_worktrees", Data: protocol.GitWorktrees{Kind: protocol.RepositoryKindDirectory}}}, true
		case "list_directory":
			return link.Response{Output: protocol.Output{Kind: "directory", Data: protocol.Directory{Revision: "root"}}}, true
		default:
			return link.Response{}, false
		}
	}
	transport.conversation = protocol.DefaultConversationSnapshot()
	transport.conversation.Node = node
	transport.conversation.Page.Session, transport.conversation.Page.Revision = shown, 1
	controller := client.New(transport)
	if err := controller.Start(t.Context()); err != nil {
		t.Fatal(err)
	}
	defer controller.Close()
	r, err := NewWithAssembly(t.Context(), controller, preferences.Open(t.TempDir()+"/preferences.json"), testassembly.Full())
	if err != nil {
		t.Fatal(err)
	}
	defer r.Close()
	cv := &viewmodel.Conversation{Node: node.String(), ID: shown.String()}
	comp := r.App.Composer(cv.ID)
	wake := make(chan struct{}, 1)
	r.SetInvalidate(func() {
		select {
		case wake <- struct{}{}:
		default:
		}
	})
	var ctx *composer.Context
	tester := ui.NewTester(func(c *ui.Context) {
		r.drain(c)
		ctx, _ = r.conversation(c, cv, comp)
		r.finishFrame()
	}, 640, 400)
	tester.SetLocale("en")
	deadline := time.After(2 * time.Second)
	for {
		tester.Frame()
		r.workers.Wait()
		r.mu.Lock()
		pending := len(r.queue) > 0 || r.dirty
		r.mu.Unlock()
		workspace := r.workspace.projects[worktree]
		directoryReady := r.resourceCatalog != nil && workspace != nil && workspace.directories[""]
		connected := r.conversationCache[shown].state.Connected
		if directoryReady && ctx.Repository.Kind == composer.Directory && connected && !pending {
			break
		}
		select {
		case <-wake:
		case <-deadline:
			t.Fatalf("composer did not settle: directory ready=%t, repository=%v, connected=%t, callbacks pending=%t", directoryReady, ctx.Repository.Kind, connected, pending)
		}
	}
	catalog := r.composerCatalogs[cv.ID]
	if catalog == nil || len(ctx.Models) == 0 {
		t.Fatalf("no catalog or models: %+v", catalog)
	}
	references, models := &catalog.references[0], &ctx.Models[0]
	sibling := func() bool {
		for _, page := range ctx.References {
			for _, child := range page.Children {
				if child.ID == other.String() {
					return true
				}
			}
		}
		return false
	}
	if !sibling() {
		t.Fatalf("sibling session not offered: %+v", ctx.References)
	}
	tester.Frame()
	if &catalog.references[0] != references || &ctx.Models[0] != models {
		t.Fatal("an idle frame rebuilt the composer catalog")
	}
	// The session's project is the composer's; a model choice rebuilds.
	comp.Effort++
	tester.Frame()
	if &catalog.references[0] == references || !sibling() {
		t.Fatal("a composer choice kept the stale catalog")
	}
}
