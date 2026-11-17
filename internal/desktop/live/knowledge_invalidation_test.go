package live

import (
	"strings"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/node/knowledge"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestKnowledgeProjectionFollowsItsOwnSource(t *testing.T) {
	first, second := protocol.NodeID{1}, protocol.NodeID{2}
	collection := protocol.KnowledgeID{3}
	root := &Runtime{App: shell.NewLive(&viewmodel.Data{Workspace: &viewmodel.Workspace{}}),
		resourceCatalog: &protocol.ResourceCatalog{Sources: []protocol.ResourceSource{
			{Node: first, Knowledge: []protocol.KnowledgeCollection{{ID: collection}}},
			{Node: second, Fault: &protocol.Fault{Code: protocol.ErrorCodePermissionDenied}},
		}}}
	view := &shell.Reading{Node: first.String(), Collection: collection.String(), Text: "First source contents"}
	root.App.ShowKnowledge(view)
	root.knowledgeWanted = second.String() + "\x00" + collection.String() + "\x00chunk\x00revision"
	root.pruneKnowledgeView()
	if root.knowledgeWanted != "" || root.App.Reading() != view || view.Text != "First source contents" {
		t.Fatal("revoking a pending source cleared another source's open page")
	}
	root.resourceCatalog.Sources[0].Fault = &protocol.Fault{Code: protocol.ErrorCodeUnavailable}
	root.pruneKnowledgeView()
	if root.App.Reading() != view || view.Text != "First source contents" || !view.Unavailable {
		t.Fatal("offline knowledge lost its last page or remained queryable")
	}
	root.resourceCatalog.Sources[0].Fault = nil
	root.pruneKnowledgeView()
	if view.Unavailable {
		t.Fatal("reconnected knowledge retained offline state")
	}
	root.resourceCatalog.Sources[0].Knowledge = nil
	root.pruneKnowledgeView()
	if root.App.Reading() != nil || view.Text != "" || view.Turn != nil {
		t.Fatal("confirmed collection removal retained restricted text or callbacks")
	}
}

func TestKnowledgePendingReadUsesRootOwnershipAndCannotReopenRemovedSource(t *testing.T) {
	node, other := protocol.NodeID{1}, protocol.NodeID{2}
	collection := protocol.KnowledgeID{3}
	input := protocol.KnowledgeCitationRequest{Node: node, Collection: collection,
		Chunk: strings.Repeat("a", 64), Revision: strings.Repeat("b", 64), Limit: 16384}
	root := &Runtime{App: shell.NewLive(&viewmodel.Data{Workspace: &viewmodel.Workspace{}}),
		resourceCatalog: &protocol.ResourceCatalog{Sources: []protocol.ResourceSource{{Node: node, Knowledge: []protocol.KnowledgeCollection{{ID: collection}}}}}}
	child := &Runtime{parent: root}
	var answer func(*ui.Context, protocol.Output, error)
	child.readKnowledge(node.String(), input, func(_ protocol.KnowledgeCitationRequest, done func(*ui.Context, protocol.Output, error)) {
		answer = done
	}, func(*ui.Context) string { return "Storage" })
	if root.knowledgeWanted == "" || child.knowledgeWanted != "" {
		t.Fatal("a child captured the shared viewer's request ownership")
	}
	root.resourceCatalog.Sources = []protocol.ResourceSource{{Node: other}}
	root.pruneKnowledgeView()
	citation := protocol.KnowledgeCitation{Node: node, Collection: collection, Chunk: input.Chunk, Revision: input.Revision}
	citation.Link = knowledge.CitationURL(citation)
	answer(nil, protocol.Output{Kind: "knowledge_citation", Data: protocol.KnowledgeCitationPage{Citation: citation, Text: "Restricted late page", Untrusted: true}}, nil)
	if root.App.Reading() != nil {
		t.Fatal("a late answer reopened text from a removed source")
	}
}

func TestUnavailableKnowledgeViewerCannotSendAnotherPage(t *testing.T) {
	next := uint32(1)
	sent := false
	view := &shell.Reading{Unavailable: true, Text: "Last synchronized page"}
	viewer := &knowledgeViewer{view: view, page: protocol.KnowledgeCitationPage{NextOffset: &next},
		read: func(protocol.KnowledgeCitationRequest, func(*ui.Context, protocol.Output, error)) { sent = true }}
	viewer.turn(true)
	if sent || view.Pending || view.Text != "Last synchronized page" {
		t.Fatal("offline knowledge performed another source read or lost its cached page")
	}
}
