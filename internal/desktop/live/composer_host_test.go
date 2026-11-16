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
	if local.conversationRuntime(cv, comp) != remote {
		t.Fatal("admitted conversation followed draft host state")
	}
}
