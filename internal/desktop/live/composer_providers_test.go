package live

import (
	"slices"
	"testing"

	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestComposerProvidersStayOnTheirExecutionNode(t *testing.T) {
	t.Parallel()
	shared, controllerOnly, executorOnly, private := protocol.ProviderID{1}, protocol.ProviderID{2}, protocol.ProviderID{3}, protocol.ProviderID{4}
	provider := func(id protocol.ProviderID, name string) protocol.ConversationProvider {
		return protocol.ConversationProvider{ID: id, Name: name, Enabled: true,
			Models: []protocol.ConversationModel{{ID: "gpt-6-sol"}, {ID: "gpt-6.1-sol"}}}
	}
	controllerSnapshot, executorSnapshot := protocol.DefaultSnapshot(), protocol.DefaultSnapshot()
	controllerSnapshot.Node, executorSnapshot.Node = protocol.NodeID{1}, protocol.NodeID{2}
	controllerSnapshot.Providers = []protocol.ConversationProvider{provider(shared, "Controller copy"), provider(controllerOnly, "Controller only")}
	executorSnapshot.Providers = []protocol.ConversationProvider{provider(shared, "Execution copy"), provider(executorOnly, "Execution only")}
	captured := protocol.DefaultSessionProfile()
	captured.Provider = provider(private, "Captured")
	executorSnapshot.Sessions = []protocol.Session{{ID: protocol.SessionID{1}, Profile: &captured}}
	controller := client.New(&conversationTestTransport{snapshot: controllerSnapshot})
	executor := client.New(&conversationTestTransport{snapshot: executorSnapshot})
	defer controller.Close()
	defer executor.Close()
	for _, source := range []*client.Client{controller, executor} {
		if err := source.Start(t.Context()); err != nil {
			t.Fatal(err)
		}
	}
	root := &Runtime{controller: controller}
	r := &Runtime{controller: executor, parent: root, admittedSessions: map[string]protocol.Session{"pending": {Profile: &captured}}}
	providers := r.composerProviders()
	var ids []protocol.ProviderID
	modelIDs := map[string]bool{}
	for _, p := range providers {
		ids = append(ids, p.ID)
		for _, model := range p.Models {
			key := modelKey(p.ID, model.ID)
			if modelIDs[key] {
				t.Fatalf("the composer offers duplicate model identity %q", key)
			}
			modelIDs[key] = true
		}
	}
	if !slices.Equal(ids, []protocol.ProviderID{shared, executorOnly, private}) || providers[0].Name != "Execution copy" {
		t.Fatalf("composer exposed a provider not owned or captured by the execution Node: %+v", providers)
	}
	if len(r.snapshot().Providers) != 2 || len(root.snapshot().Providers) != 2 {
		t.Fatal("merging channels modified a source snapshot")
	}
}

func TestNewSessionCreationStaysOnSelectedNode(t *testing.T) {
	t.Parallel()
	r := &Runtime{parent: &Runtime{}}
	project := protocol.ProjectID{3}
	worktree := protocol.WorktreeID{4}
	command := r.creationCommand(&project, &worktree, protocol.DefaultSessionConfig(), 9)
	if command.Kind != "create_session" {
		t.Fatalf("new session creation command = %q, want create_session", command.Kind)
	}
	payload, ok := command.Data.(protocol.CommandCreateSessionPayload)
	if !ok || payload.Project == nil || *payload.Project != project || payload.Worktree == nil || *payload.Worktree != worktree {
		t.Fatalf("creation payload lost the selected project or worktree: %+v", command.Data)
	}
}
