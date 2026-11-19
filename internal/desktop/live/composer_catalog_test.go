package live

import (
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/preferences"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
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
	transport.conversation = protocol.DefaultConversationSnapshot()
	transport.conversation.Node = node
	transport.conversation.Page.Session, transport.conversation.Page.Revision = shown, 1
	controller := client.New(transport)
	if err := controller.Start(t.Context()); err != nil {
		t.Fatal(err)
	}
	defer controller.Close()
	r, err := NewWithPreferences(t.Context(), controller, preferences.Open(t.TempDir()+"/preferences.json"))
	if err != nil {
		t.Fatal(err)
	}
	defer r.Close()
	cv := &viewmodel.Conversation{Node: node.String(), ID: shown.String()}
	comp := r.App.Composer(cv.ID)
	var ctx *composer.Context
	tester := ui.NewTester(func(c *ui.Context) {
		r.drain(c)
		ctx, _ = r.conversation(c, cv, comp)
		r.finishFrame()
	}, 640, 400)
	tester.SetLocale("en")
	for range 4 {
		r.workers.Wait()
		tester.Frame()
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
