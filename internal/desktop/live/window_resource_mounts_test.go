package live

import (
	"context"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	view "github.com/sailry/sailry-harness/internal/desktop/viewapi"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func fixtureConversationSpec(profile protocol.DatabaseID) view.ConversationSpec {
	return view.ConversationSpec{
		Match: func(session view.Session) bool {
			resource := session.Config.Resource
			if resource == nil || resource.Kind != "database" {
				return false
			}
			id, ok := detail[protocol.DatabaseID](resource.Data)
			return ok && id == profile
		},
		Config: func(base view.SessionConfig) view.SessionConfig {
			base.Resource = &protocol.ConnectionResource{Kind: "database", Data: profile}
			base.Permission = protocol.PermissionFull
			return base
		},
	}
}

func TestWindowServicesConversationHistory(t *testing.T) {
	profile, other := protocol.DatabaseID{1}, protocol.DatabaseID{2}
	spec := fixtureConversationSpec(profile)
	legacyBinding := protocol.PluginConversationBinding{Package: protocol.PluginReference{Name: "legacy"}, ID: "assistant"}
	legacy := protocol.Session{ID: protocol.SessionID{1}, Revision: 4, Worktree: protocol.WorktreeID{2}, Config: spec.Config(protocol.DefaultSessionConfig())}
	legacy.Config.Assistant = &legacyBinding
	legacy.Config.Resource.Data = &profile
	native := protocol.Session{ID: protocol.SessionID{2}, Config: spec.Config(protocol.DefaultSessionConfig())}
	otherSession := protocol.Session{ID: protocol.SessionID{3}, Config: fixtureConversationSpec(other).Config(protocol.DefaultSessionConfig())}
	archived, delegated := native, native
	archived.ID, archived.Archived = protocol.SessionID{4}, true
	delegation := &protocol.Delegation{}
	delegated.ID, delegated.Delegation = protocol.SessionID{5}, &delegation
	snapshot := protocol.DefaultSnapshot()
	snapshot.Node = protocol.NodeID{7}
	snapshot.Databases = []protocol.DatabaseProfile{{ID: profile}, {ID: other}}
	snapshot.Sessions = []protocol.Session{legacy, otherSession, native, archived, delegated}
	r, _, _ := newWindowServicesFixture(t, &terminalTabsTestNode{pluginViewTestNode: pluginViewTestNode{snapshot: snapshot}}, nil)
	mounted := &windowDockedConversation{runtime: r, spec: spec, resource: native.Config.Resource}
	got := mounted.sessions()
	if len(got) != 2 || got[0].ID != native.ID || got[1].ID != legacy.ID {
		t.Fatal("history must list only the resource's live non-delegated sessions, newest first")
	}
	saved, ok := r.sessionWithID(legacy.ID)
	if !ok || got[1].Config.Assistant == nil || got[1].Config.Assistant != saved.Config.Assistant ||
		*got[1].Config.Assistant != legacyBinding || got[1].Config.Permission != protocol.PermissionFull ||
		got[1].Revision != legacy.Revision || got[1].Worktree != legacy.Worktree {
		t.Fatal("history changed the saved session's assistant, permission, revision or worktree")
	}
	mounted.selectTarget()
	if mounted.session != native.ID || mounted.draft != nil {
		t.Fatal("the native view did not select the newest matching session")
	}
	mounted.session = legacy.ID
	mounted.selectTarget()
	if mounted.session != legacy.ID || mounted.draft != nil {
		t.Fatal("selecting the legacy session created a draft instead")
	}
	previous := r.App.Composer(legacy.ID.String())
	r.applyComposer(previous, legacy.Config)
	mounted.newDraft(previous)
	t.Cleanup(mounted.close)
	config := r.composerBases[r.App.Composer(mounted.draftID)]
	if !spec.Match(view.Session{Config: config}) || config.Assistant != nil || config.Permission != protocol.PermissionAsk {
		t.Fatal("a new resource draft inherited the legacy assistant or permission, or changed resources")
	}
	if saved, ok := r.sessionWithID(legacy.ID); !ok || saved.Config.Assistant == nil || *saved.Config.Assistant != legacyBinding || saved.Config.Permission != protocol.PermissionFull {
		t.Fatal("opening a new draft changed a legacy session's configuration")
	}
}

type windowTerminalProbe struct {
	terminalTabsTestNode
	inspecting chan context.Context
}

func (source *windowTerminalProbe) Dispatch(ctx context.Context, caller protocol.NodeID, request protocol.Request) (link.Admission, error) {
	if request.Command.Kind == "inspect_terminal" {
		source.inspecting <- ctx
		return link.Admission{Receipt: protocol.Receipt{ID: request.ID}, Completion: make(chan link.Response)}, nil
	}
	return source.terminalTabsTestNode.Dispatch(ctx, caller, request)
}

func TestWindowServicesReleaseOnLeaving(t *testing.T) {
	snapshot := protocol.DefaultSnapshot()
	snapshot.Node = protocol.NodeID{7}
	profile := protocol.DatabaseID{3}
	snapshot.Databases = []protocol.DatabaseProfile{{ID: profile}}
	source := &windowTerminalProbe{terminalTabsTestNode: terminalTabsTestNode{
		pluginViewTestNode: pluginViewTestNode{snapshot: snapshot},
		respond: func(protocol.Request) link.Response {
			return link.Response{Fault: &protocol.Fault{Code: protocol.ErrorCodeUnavailable}}
		},
	}, inspecting: make(chan context.Context, 1)}
	layoutKey := "native:" + snapshot.Node.String() + "/fixture"
	var terminal view.Terminal
	var conversation view.Conversation
	r, services, _ := bindWindowServicesFixture(t, snapshot.Node, source, func(binding shell.WindowContext) shell.WindowBinding {
		return shell.WindowBinding{
			Render: func(c *ui.Context) {
				terminal.Render(c.Key("terminal"))
				conversation.Header(c.Key("conversation"), "Conversation")
			},
			Slots: func() shell.WorkspaceSlots { return shell.WorkspaceSlots{PreferenceKey: layoutKey} },
			Sync: func() {
				if !binding.Visible() && conversation.Close != nil {
					conversation.Close()
				}
			},
		}
	})
	terminal = services.MountTerminal(protocol.TerminalID{1}, func(view.TerminalInfo) bool { return true })
	conversation = services.MountConversation(fixtureConversationSpec(profile))
	prefs := r.App.Prefs()
	saved := shell.PluginWorkspacePrefs{Width: 352, Folded: true}
	prefs.PluginWorkspaces = map[string]shell.PluginWorkspacePrefs{layoutKey: saved}
	r.App.SetPrefs(prefs)
	r.App.Navigate(servicesTestPage)
	tester := ui.NewTester(r.App.View, 1024, 768)
	read := awaitWindowValue(t, source.inspecting)
	var draftID string
	for id := range r.pluginViews.drafts {
		draftID = id
	}
	if read.Err() != nil || len(r.pluginViews.drafts) != 1 || r.App.Prefs().PluginWorkspaces[layoutKey] != saved {
		t.Fatal("the visible workspace did not retain its view, draft and stored layout")
	}
	r.App.Navigate(shell.ConversationPage)
	tester.Frame()
	awaitWindowValue(t, read.Done())
	if r.pluginViews.drafts[draftID] != nil || r.App.DropComposer(draftID) != nil || len(source.sent("close_terminal")) != 0 || len(source.sent("close_ssh_terminal")) != 0 {
		t.Fatal("leaving retained a draft or closed the Node terminal instead of the view")
	}
}
