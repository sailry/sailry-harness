package live

import (
	"context"
	"strings"
	"testing"
	"testing/synctest"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/knowledge"
	"github.com/sailry/sailry-harness/internal/protocol"
	knowledgedesktop "github.com/sailry/sailry-harness/modules/knowledge/desktop"
)

func TestKnowledgeConversationLinkFollowsSourceNode(t *testing.T) {
	for _, test := range []struct {
		name       string
		registered bool
		explicit   bool
	}{
		{name: "without Knowledge"},
		{name: "conversation Node", registered: true},
		{name: "explicit storage Node", registered: true, explicit: true},
	} {
		t.Run(test.name, func(t *testing.T) {
			synctest.Test(t, func(t *testing.T) {
				local, remote := protocol.NodeID{1}, protocol.NodeID{2}
				session := protocol.SessionID{1}.String()
				newOwner := func(node protocol.NodeID) (*Runtime, *terminalTabsTestNode) {
					ctx, cancel := context.WithCancel(t.Context())
					source := &terminalTabsTestNode{}
					source.snapshot = protocol.DefaultSnapshot()
					source.snapshot.Node = node
					source.respond = func(request protocol.Request) link.Response {
						input, ok := detail[protocol.KnowledgeCitationRequest](request.Command.Data)
						if request.Command.Kind != "read_knowledge_citation" || !ok {
							return link.Response{Fault: &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest}}
						}
						citation := protocol.KnowledgeCitation{Node: node, Collection: input.Collection, Chunk: input.Chunk, Revision: input.Revision}
						citation.Link = knowledge.CitationURL(citation)
						page := protocol.KnowledgeCitationPage{Citation: citation, Title: "Fixture", Text: "source text", Untrusted: true}
						return link.Response{Output: protocol.Output{Kind: "knowledge_citation", Data: page}}
					}
					controller := client.New(link.NewLocal(node, node, source))
					owner := &Runtime{ctx: ctx, controller: controller, busy: map[string]bool{}, uncertain: map[string]protocol.Request{}}
					t.Cleanup(func() {
						cancel()
						owner.workers.Wait()
						_ = controller.Close()
					})
					if err := controller.Start(ctx); err != nil {
						t.Fatal(err)
					}
					return owner, source
				}
				root, localSource := newOwner(local)
				bound, remoteSource := newOwner(remote)
				data := &viewmodel.Data{Workspace: &viewmodel.Workspace{}, Unassigned: []*viewmodel.Conversation{{ID: session, Node: remote.String()}}}
				var assembly shell.Assembly
				if test.registered {
					assembly = shell.Assemble(knowledgedesktop.Module())
				}
				root.App = shell.NewLiveWithAssembly(data, assembly)
				bound.parent, bound.App = root, root.App
				root.bindings = map[string]*Runtime{remote.String(): bound}
				synctest.Wait()
				citation := protocol.KnowledgeCitation{Collection: protocol.KnowledgeID{1}, Chunk: strings.Repeat("a", 64), Revision: strings.Repeat("b", 64)}
				want := remote
				if test.explicit {
					citation.Node, want = local, local
				}
				address := knowledge.CitationURL(citation)
				tester := ui.NewTester(func(c *ui.Context) {
					if address != "" {
						root.openChatLink(c, session, address)
						address = ""
					}
					root.mu.Lock()
					pending := root.queue
					root.queue = nil
					root.mu.Unlock()
					for _, run := range pending {
						run(c)
					}
				}, 320, 240)
				tester.Frame()
				synctest.Wait()
				tester.Frame()
				localReads, remoteReads := len(localSource.sent("read_knowledge_citation")), len(remoteSource.sent("read_knowledge_citation"))
				view := root.App.Reading()
				if !test.registered {
					if localReads+remoteReads != 0 || view != nil || len(tester.OpenedURLs()) != 0 {
						t.Fatal("unregistered citation reached a viewer, Node, or OS opener")
					}
					return
				}
				if view == nil || view.Node != want.String() || view.Text != "source text" || localReads+remoteReads != 1 || (localReads == 1) != test.explicit {
					t.Fatalf("citation dispatched to wrong Node: local=%d remote=%d viewer=%+v", localReads, remoteReads, view)
				}
			})
		})
	}
}

func TestConnectionReferenceLinkUsesCapturedOwner(t *testing.T) {
	local, remote := protocol.NodeID{1}, protocol.NodeID{2}
	project, session := protocol.ProjectID{1}, protocol.SessionID{1}
	ssh, database := protocol.SshID{1}, protocol.DatabaseID{1}
	shared := &protocol.ConnectionSharing{Kind: "projects", Data: []protocol.ProjectID{project}}
	makeOwner := func(node protocol.NodeID, host, name string) *Runtime {
		snapshot := protocol.DefaultSnapshot()
		snapshot.Node = node
		snapshot.Sessions = []protocol.Session{{ID: session, Project: &project}}
		snapshot.Ssh = []protocol.SshProfile{
			{ID: ssh, Host: host, Sharing: shared},
			{ID: protocol.SshID{2}, Host: "private.invalid"},
		}
		snapshot.Databases = []protocol.DatabaseProfile{{ID: database, Name: name, Sharing: shared}}
		return pluginViewFixture(t, &pluginViewTestNode{snapshot: snapshot})
	}
	root := makeOwner(local, "local.invalid", "local")
	bound := makeOwner(remote, "remote.invalid", "remote")
	bound.parent, bound.App = root, root.App
	root.bindings = map[string]*Runtime{remote.String(): bound}
	root.App.Data.Unassigned = append(root.App.Data.Unassigned, &viewmodel.Conversation{ID: session.String(), Node: remote.String()})
	var captured shell.LinkActions
	if _, err := root.App.RegisterLinkHandler(shell.LinkRoute{Kind: shell.LinkReference, Name: "ssh"}, func(_ *ui.Context, target shell.LinkTarget, actions shell.LinkActions) error {
		captured = actions
		if value, ok := actions.SSHReference(ssh); target.Node != remote || !ok || value.Host != "remote.invalid" {
			t.Fatalf("SSH lookup node %v, endpoint %+v, available %v", target.Node, value, ok)
		}
		if value, ok := actions.DatabaseReference(database); !ok || value != "remote" {
			t.Fatalf("database lookup name %q, available %v", value, ok)
		}
		if _, ok := actions.SSHReference(protocol.SshID{2}); ok {
			t.Fatal("a restricted SSH profile was exposed")
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
	bound.openReference(session.String(), protocol.TurnID{}, protocol.ConversationReference{
		Target: protocol.ConversationReferenceTarget{Kind: "ssh", Data: ssh},
	})
	pluginViewRunQueue(root)
	if captured.SSHReference == nil || captured.DatabaseReference == nil {
		t.Fatal("the captured owner did not dispatch its reference")
	}
	if _, ok := captured.SSHReference(ssh); ok {
		t.Fatal("retired link actions retained SSH endpoints")
	}
	if _, ok := captured.DatabaseReference(database); ok {
		t.Fatal("retired link actions retained database names")
	}
}
