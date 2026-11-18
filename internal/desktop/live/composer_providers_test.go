package live

import (
	"slices"
	"testing"

	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
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

func TestComposerOnlyOffersCredentialsFromItsExecutionNode(t *testing.T) {
	snapshot := protocol.DefaultSnapshot()
	snapshot.Node = protocol.NodeID{1}
	origin := protocol.NodeID{2}
	local := protocol.ConversationProvider{
		ID: protocol.ProviderID{1}, Enabled: true,
		Source:     &protocol.ConfigurationOrigin{Node: origin},
		Credential: &protocol.CredentialRef{Node: snapshot.Node, ID: protocol.NewCredentialID()},
		Models:     []protocol.ConversationModel{{ID: "local"}},
	}
	legacy := local
	legacy.ID = protocol.ProviderID{2}
	legacy.Credential = &protocol.CredentialRef{Node: origin, ID: protocol.NewCredentialID()}
	legacy.Models = []protocol.ConversationModel{{ID: "legacy"}}
	snapshot.Providers = []protocol.ConversationProvider{local, legacy}
	controller := client.New(&conversationTestTransport{snapshot: snapshot})
	defer controller.Close()
	if err := controller.Start(t.Context()); err != nil {
		t.Fatal(err)
	}
	r := &Runtime{controller: controller}
	localModel, legacyModel := modelKey(local.ID, "local"), modelKey(legacy.ID, "legacy")
	offered := r.offeredModels("draft/1", []composer.Model{{ID: localModel}, {ID: legacyModel}})
	if len(offered) != 1 || offered[0].ID != localModel {
		t.Fatalf("wrong executable models: %+v", offered)
	}
	if _, _, err := r.composerConfig(composer.New(""), composer.Message{Model: legacyModel}); faultCode(err) != protocol.ErrorCodeNotConfigured {
		t.Fatalf("a stale selection used the foreign credential: %v", err)
	}
}
