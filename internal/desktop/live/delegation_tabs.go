package live

import (
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func delegated(session protocol.Session) bool {
	return session.Delegation != nil && *session.Delegation != nil
}

func (r *Runtime) childSession(parent protocol.SessionID, id string) (protocol.ConversationChild, protocol.Session, bool) {
	state, ok := r.controller.Conversation(parent)
	if !ok {
		return protocol.ConversationChild{}, protocol.Session{}, false
	}
	for _, child := range client.LatestConversationChildren(state.Snapshot.Page.Children) {
		if child.Run.Session.String() == id {
			session, valid := client.ConversationChildSession(r.snapshot(), parent, child)
			return child, session, valid
		}
	}
	return protocol.ConversationChild{}, protocol.Session{}, false
}

func childTitle(page *protocol.ConversationPage, child protocol.ConversationChild, locale string) string {
	name, title := "", ""
	if child.Name != nil && strings.TrimSpace(*child.Name) != "" {
		name = *child.Name
	}
	if page != nil {
		for _, entry := range page.Entries {
			if entry.ID != child.Origin.Entry || entry.Turn != child.Origin.Turn || child.Origin.Index >= uint64(len(entry.Parts)) {
				continue
			}
			part := entry.Parts[child.Origin.Index]
			if part.Kind == "tool_call" {
				if call, ok := detail[protocol.ConversationPartToolCallPayload](part.Data); ok && call.Summary != nil && call.Summary.Title != nil && strings.TrimSpace(*call.Summary.Title) != "" {
					title = *call.Summary.Title
				}
			}
			break
		}
	}
	if title != "" {
		if name != "" && child.Origin.Role != nil {
			return name + " " + title
		}
		return title
	}
	if name != "" {
		return name
	}
	return i18n.In(locale, "chat_child")
}

func (r *Runtime) openLiveChild(parent *viewmodel.Conversation, id string) {
	parentID, err := protocol.ParseSessionID(parent.ID)
	if err != nil {
		return
	}
	child, session, ok := r.childSession(parentID, id)
	if !ok {
		r.enqueue(func(c *ui.Context) { kit.Toast(c, kit.Failed, i18n.T(c, "chat_child_unavailable")) })
		return
	}
	r.executeScoped("child-tab/"+id, protocol.Command{Kind: "read_session", Data: protocol.CommandReadSessionPayload{Session: session.ID}}, func(c *ui.Context, out protocol.Output, err error) {
		if err != nil {
			r.reportKeys(c, err, "chat_child_unavailable", "chat_child_unavailable", "chat_child_unavailable")
			return
		}
		current, valid := detail[protocol.Session](out.Data)
		if !valid || current.ID != session.ID || !delegated(current) || (*current.Delegation).Session != parentID {
			kit.Toast(c, kit.Conflict, i18n.T(c, "chat_child_unavailable"))
			return
		}
		r.admittedSessions[id] = current
		r.teams.childParents[id] = parentID
		var page *protocol.ConversationPage
		if state, ready := r.controller.Conversation(parentID); ready {
			page = &state.Snapshot.Page
		}
		r.App.Panel.ShowChild(id, childTitle(page, child, c.Locale()))
	})
}

func (r *Runtime) renderLiveChild(c *ui.Context, id string) {
	if r.parent == nil {
		for _, bound := range r.bindings {
			if _, ok := bound.teams.childParents[id]; ok {
				bound.renderLiveChild(c, id)
				return
			}
		}
	}
	if r.teams == nil {
		return
	}
	parent, known := r.teams.childParents[id]
	if !known {
		kit.Result(c, kit.ResultProps{Title: i18n.T(c, "chat_child_unavailable")}).Grow(1).Justify(ui.Center)
		return
	}
	r.watchChild(parent)
	child, session, ok := r.childSession(parent, id)
	if !ok || !delegated(session) {
		kit.Result(c, kit.ResultProps{Title: i18n.T(c, "chat_child_unavailable")}).Grow(1).Justify(ui.Center)
		return
	}
	r.conversationSeen[parent] = true
	var page *protocol.ConversationPage
	if state, ready := r.conversationState(parent); ready {
		page = &state.Snapshot.Page
	}
	cv := &viewmodel.Conversation{Node: r.controller.Target().String(), ID: id, Title: childTitle(page, child, c.Locale())}
	comp := r.App.Composer(id)
	ctx, actions := r.conversation(c, cv, comp)
	ctx.Readonly = true
	conversation.Docked(c, r.App.Messages, id, cv.Turns, &cv.Session, actions, func() {
		composer.View(c, comp, ctx, actions)
	})
	conversation.Navigation(c, r.App.Messages)
}
