package live

import (
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func delegated(session protocol.Session) bool {
	return session.Delegation != nil && *session.Delegation != nil
}

type liveChildScopes struct {
	checks map[protocol.SessionID]*childOriginCheck
	tabs   map[childTabKey]childTabScope
}

type childTabKey struct{ selected, node, id string }

func (r *Runtime) childTabKey(id string) childTabKey {
	key := childTabKey{selected: r.App.Sidebar.Selected, id: id}
	if selected := r.App.Data.Find(key.selected); selected != nil {
		key.node = selected.Node
	}
	return key
}

type childTabScope struct {
	node     protocol.NodeID
	parent   protocol.SessionID
	revision uint64
	target   client.ConversationChildTarget
}

type childOriginCheck struct {
	childTabScope
	confirmed protocol.ConversationChild
}

func (check *childOriginCheck) matches(scope childTabScope) bool {
	if check == nil || check.node != scope.node || check.parent != scope.parent || check.revision != scope.revision ||
		check.target.Background == nil || scope.target.Background == nil {
		return false
	}
	a, b := check.target, scope.target
	return a.Background.Turn == b.Background.Turn && a.Background.Revision == b.Background.Revision && a.Child.Run.Turn == b.Child.Run.Turn &&
		a.Child.Run.Sequence == b.Child.Run.Sequence && a.Child.Run.Revision == b.Child.Run.Revision && a.MatchesSession(b.Session)
}

// liveChildren exposes canonical and confirmed background children to the
// composer, overview and approval lookup. A background edge supplies a turn
// to verify, not permission to observe or control another session.
func (r *Runtime) liveChildren(conversation protocol.ConversationSnapshot) []client.ConversationChildTarget {
	targets := client.ConversationChildTargets(r.snapshot(), conversation)
	verified := targets[:0]
	for _, target := range targets {
		if target.Background != nil {
			if r.teams == nil {
				continue
			}
			scope := childTabScope{node: conversation.Node, parent: conversation.Page.Session, revision: conversation.Page.Revision, target: target}
			confirmed, ok := r.checkedChildOrigin(scope)
			if !ok || !runStatus(confirmed.Run.Status).Active() {
				continue
			}
			target.Child.Origin, target.Child.Name = confirmed.Origin, confirmed.Name
		}
		verified = append(verified, target)
	}
	return verified
}

func (r *Runtime) checkedChildOrigin(scope childTabScope) (protocol.ConversationChild, bool) {
	checks := r.teams.childScopes.checks
	id := scope.target.Session.ID
	if !r.controller.Connected() || scope.node != r.controller.Target() {
		delete(checks, id)
		return protocol.ConversationChild{}, false
	}
	if check := checks[id]; check.matches(scope) {
		return check.confirmed, check.confirmed.Run.Turn != (protocol.TurnID{})
	}
	if checks == nil {
		checks = map[protocol.SessionID]*childOriginCheck{}
		r.teams.childScopes.checks = checks
	}
	check := &childOriginCheck{childTabScope: scope}
	checks[id] = check
	if !r.async(func() {
		child, err := r.controller.ConfirmConversationChild(r.ctx, scope.node, scope.revision, scope.target)
		if err == nil {
			r.enqueue(func(*ui.Context) { check.confirmed = child })
		}
	}) {
		delete(checks, id)
	}
	return protocol.ConversationChild{}, false
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

func liveChildTitle(page *protocol.ConversationPage, target client.ConversationChildTarget, locale string) string {
	if target.Background != nil && strings.TrimSpace(target.Background.Title) != "" {
		if name := target.Child.Name; name != nil && strings.TrimSpace(*name) != "" && target.Child.Origin.Role != nil {
			return *name + " " + target.Background.Title
		}
		return target.Background.Title
	}
	return childTitle(page, target.Child, locale)
}

func (r *Runtime) openLiveChild(parent *viewmodel.Conversation, id string) {
	parentID, err := protocol.ParseSessionID(parent.ID)
	if err != nil || parent.Node != r.controller.Target().String() || r.conversationRuntime(parent, nil) != r || r.teams == nil {
		return
	}
	state, ready := r.controller.Conversation(parentID)
	childID, parse := protocol.ParseSessionID(id)
	target, found := client.ConversationChildOpenTarget(r.snapshot(), state.Snapshot, childID)
	if parse != nil || childID.String() != id || !ready || state.Removed || !found {
		r.enqueue(func(c *ui.Context) { kit.Toast(c, kit.Failed, i18n.T(c, "chat_child_unavailable")) })
		return
	}
	scope := childTabScope{node: state.Snapshot.Node, parent: parentID, revision: state.Snapshot.Page.Revision, target: target}
	key, page := r.childTabKey(id), r.App.Page
	busy := "child-tab/" + key.selected + "/" + id
	if r.busy[busy] {
		return
	}
	r.busy[busy] = true
	if !r.async(func() {
		target := scope.target
		var out protocol.Output
		var err error
		if target.Background != nil || target.Child.Run.Turn == (protocol.TurnID{}) {
			// Only a current proof can bridge the conversation observation.
			var confirmed protocol.ConversationChild
			confirmed, err = r.controller.ConfirmConversationChild(r.ctx, scope.node, scope.revision, target)
			target.Child = confirmed
		}
		if err == nil {
			out, err = r.controller.Do(r.ctx, protocol.Command{Kind: "read_session", Data: protocol.CommandReadSessionPayload{Session: target.Session.ID}})
		}
		r.enqueue(func(c *ui.Context) {
			delete(r.busy, busy)
			if r.childTabKey(id) != key || r.App.Page != page || r.conversationRuntime(parent, nil) != r {
				return
			}
			if active := r.App.Data.Find(parent.ID); active != nil && active.Node != scope.node.String() {
				return
			}
			if err != nil {
				r.reportKeys(c, err, "chat_child_unavailable", "chat_child_unavailable", "chat_child_unavailable")
				return
			}
			current, valid := detail[protocol.Session](out.Data)
			fresh, ready := r.controller.Conversation(parentID)
			listed, inScope := target.CheckScope(r.snapshot(), scope.node)
			if out.Kind != "session" || !valid || !target.MatchesSession(current) || !ready || fresh.Removed ||
				fresh.Snapshot.Node != scope.node || fresh.Snapshot.Page.Session != parentID || fresh.Snapshot.Page.Revision != scope.revision || !inScope {
				kit.Toast(c, kit.Conflict, i18n.T(c, "chat_child_unavailable"))
				return
			}
			r.admittedSessions[id] = current
			r.teams.childParents[id] = parentID
			target.Listed = listed
			scope.target = target
			tabs := &r.localRuntime().teams.childScopes.tabs
			if *tabs == nil {
				*tabs = map[childTabKey]childTabScope{}
			}
			for k := range *tabs {
				if !r.App.HasPanelTab(panel.Tab{Kind: panel.Subagent, ID: k.id}) {
					delete(*tabs, k)
				}
			}
			(*tabs)[key] = scope
			r.App.RevealConversationPanel()
			r.App.Panel.ShowChild(id, liveChildTitle(&fresh.Snapshot.Page, target, c.Locale()))
		})
	}) {
		delete(r.busy, busy)
	}
}

func (r *Runtime) renderLiveChild(c *ui.Context, id string) {
	root := r.localRuntime()
	if root.teams == nil {
		return
	}
	key := r.childTabKey(id)
	scope, known := root.teams.childScopes.tabs[key]
	r = root.runtimeFor(scope.node.String())
	if !known || r == nil {
		kit.Result(c, kit.ResultProps{Title: i18n.T(c, "chat_child_unavailable")}).Grow(1).Justify(ui.Center)
		return
	}
	session, exists := r.session(id)
	listed, inScope := scope.target.CheckScope(r.snapshot(), scope.node)
	if !exists || !inScope || !scope.target.MatchesSession(session) {
		kit.Result(c, kit.ResultProps{Title: i18n.T(c, "chat_child_unavailable")}).Grow(1).Justify(ui.Center)
		return
	}
	if listed && !scope.target.Listed {
		scope.target.Listed = true
		root.teams.childScopes.tabs[key] = scope
	}
	r.watchChild(scope.parent)
	state, ready := r.conversationState(scope.parent)
	if !ready || state.Loading && state.Snapshot.Page.Revision == 0 {
		kit.Result(c, kit.ResultProps{Title: i18n.T(c, "chat_reconnecting")}).Grow(1).Justify(ui.Center)
		return
	}
	page := &state.Snapshot.Page
	if state.Removed || state.Snapshot.Node != scope.node || page.Session != scope.parent || page.Revision != scope.revision {
		kit.Result(c, kit.ResultProps{Title: i18n.T(c, "chat_child_unavailable")}).Grow(1).Justify(ui.Center)
		return
	}
	if r.childConversations == nil {
		r.childConversations = map[protocol.SessionID]*viewmodel.Conversation{}
	}
	cv := r.childConversations[session.ID]
	if cv == nil {
		cv = &viewmodel.Conversation{Node: r.controller.Target().String(), ID: id}
		r.childConversations[session.ID] = cv
	}
	cv.Title = liveChildTitle(page, scope.target, c.Locale())
	comp := r.App.Composer(id)
	ctx, actions := r.conversation(c, cv, comp)
	ctx.Readonly = true
	conversation.Docked(c, r.App.Messages, id, cv.Turns, &cv.Session, actions, func() {
		composer.View(c, comp, ctx, actions)
	})
	conversation.Navigation(c, r.App.Messages)
}
