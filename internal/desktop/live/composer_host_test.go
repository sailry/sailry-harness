package live

import (
	"testing"

	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestProjectlessDraftExecutionHost(t *testing.T) {
	var owners []*Runtime
	for _, node := range []protocol.NodeID{{1}, {2}} {
		snapshot := protocol.DefaultSnapshot()
		snapshot.Node = node
		controller := client.New(&conversationTestTransport{snapshot: snapshot})
		if err := controller.Start(t.Context()); err != nil {
			t.Fatal(err)
		}
		defer controller.Close()
		owners = append(owners, &Runtime{controller: controller})
	}
	local, remote := owners[0], owners[1]
	local.App = shell.NewLive(&viewmodel.Data{Workspace: &viewmodel.Workspace{}})
	remote.App, remote.parent = local.App, local
	local.bindings = map[string]*Runtime{remote.controller.Target().String(): remote}
	cv := &viewmodel.Conversation{ID: "draft/new", Node: local.controller.Target().String()}
	comp := composer.New("")
	comp.Host = remote.controller.Target().String()
	if local.conversationRuntime(cv, comp) != remote || cv.Node != comp.Host {
		t.Fatal("projectless host selection did not route the draft")
	}
	delete(local.bindings, comp.Host)
	if local.conversationRuntime(cv, comp) != nil || cv.Node != comp.Host {
		t.Fatal("unavailable remote draft fell back to local execution")
	}
	comp.Host = local.controller.Target().String()
	if local.conversationRuntime(cv, comp) != local || cv.Node != comp.Host {
		t.Fatal("unadmitted offline draft could not return to local execution")
	}
	cv.ID, cv.Node, comp.Host = protocol.NewSessionID().String(), remote.controller.Target().String(), local.controller.Target().String()
	local.bindings[cv.Node] = remote
	if local.conversationRuntime(cv, comp) != remote || comp.Host != cv.Node {
		t.Fatal("admitted conversation followed draft host state")
	}
}

func TestSelectedProjectOwnsDraftModelAndPreservesText(t *testing.T) {
	makeOwner := func(node protocol.NodeID, id protocol.ProviderID, name string) *Runtime {
		snapshot := protocol.DefaultSnapshot()
		snapshot.Node = node
		provider := protocol.ConversationProvider{ID: id, Name: name, Enabled: true, DefaultModel: "model", Models: []protocol.ConversationModel{{ID: "model"}}}
		snapshot.Providers = []protocol.ConversationProvider{provider}
		config := protocol.DefaultSessionConfig()
		config.Provider, config.Model = id, "model"
		snapshot.Defaults.Config = &config
		controller := client.New(&conversationTestTransport{snapshot: snapshot})
		if err := controller.Start(t.Context()); err != nil {
			t.Fatal(err)
		}
		t.Cleanup(func() { _ = controller.Close() })
		return &Runtime{controller: controller, composerBases: map[*composer.State]protocol.SessionConfig{}}
	}
	localNode, remoteNode := protocol.NodeID{1}, protocol.NodeID{2}
	local, remote := makeOwner(localNode, protocol.ProviderID{1}, "Local"), makeOwner(remoteNode, protocol.ProviderID{2}, "Remote")
	local.App = shell.NewLive(&viewmodel.Data{Workspace: &viewmodel.Workspace{}, Projects: []*viewmodel.Project{{ID: "remote-project", Host: remoteNode.String()}}})
	remote.App, remote.parent = local.App, local
	local.bindings = map[string]*Runtime{remoteNode.String(): remote}
	cv := &viewmodel.Conversation{ID: "draft/project", Node: localNode.String()}
	comp := composer.New("")
	comp.Host, comp.Project, comp.Draft = localNode.String(), "remote-project", "retain this draft"
	comp.Model = modelKey(protocol.ProviderID{1}, "model")
	if owner := local.conversationRuntime(cv, comp); owner != remote {
		t.Fatal("selected project's Node did not own the draft")
	}
	if comp.Model != modelKey(protocol.ProviderID{2}, "model") || comp.Draft != "retain this draft" {
		t.Fatalf("host change did not select its default model while preserving text: model=%q draft=%q", comp.Model, comp.Draft)
	}
}

func TestHistoricalRemoteCaptureIsNotOfferedForNewTurns(t *testing.T) {
	node, controller := protocol.NodeID{2}, protocol.NodeID{1}
	provider := protocol.DefaultConversationProvider()
	provider.ID, provider.Enabled, provider.DefaultModel = protocol.ProviderID{3}, true, "fixture"
	provider.Models = []protocol.ConversationModel{{ID: "fixture"}}
	session := protocol.DefaultSession()
	session.ID = protocol.SessionID{4}
	session.Config.Provider, session.Config.Model = provider.ID, provider.DefaultModel
	session.Profile = &protocol.SessionProfile{Source: controller, Provider: provider}
	snapshot := protocol.DefaultSnapshot()
	snapshot.Node, snapshot.Sessions = node, []protocol.Session{session}
	transport := &conversationTestTransport{snapshot: snapshot}
	client := client.New(transport)
	if err := client.Start(t.Context()); err != nil {
		t.Fatal(err)
	}
	defer client.Close()
	r := &Runtime{controller: client}
	model := composer.Model{ID: modelKey(provider.ID, "fixture"), Provider: provider.Name}
	if offered := r.offeredModels(session.ID.String(), []composer.Model{model}); len(offered) != 0 {
		t.Fatalf("historical remote account remained selectable for a new turn: %v", offered)
	}
}
