package live

import (
	"encoding/json"
	"fmt"
	"maps"
	"slices"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/plugins"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type pluginNativeConversation struct {
	request  string
	session  protocol.SessionID
	draft    *protocol.Session
	draftID  string
	view     *viewmodel.Conversation
	composer *composer.State
	context  *composer.Context
	actions  shell.ConversationActions
	// history is the assistant's session picker: its query and row.
	history struct {
		open  bool
		query string
		row   int
	}
}

type pluginNativeConversationRequest struct {
	Assistant *string                      `json:"assistant"`
	Resource  *protocol.ConnectionResource `json:"resource"`
	Session   *protocol.SessionID          `json:"session"`
}

func (state *pluginNativeState) renderConversation(view *ui.Context, element plugins.NativeElement, _ func(), _ func(plugins.CallbackRef, json.RawMessage)) (ui.Element, error) {
	identity, props, err := pluginNativeProps(element)
	if err != nil {
		return ui.Element{}, err
	}
	if active := props["active"]; string(active) == "false" {
		state.releaseConversation(state.conversations[identity])
		delete(state.conversations, identity)
		return ui.Box(view).Size(0, 0), nil
	}
	body, err := json.Marshal(props)
	if err != nil {
		return ui.Element{}, err
	}
	var request pluginNativeConversationRequest
	if err := json.Unmarshal(body, &request); err != nil || request.Resource != nil && request.Assistant == nil ||
		request.Session != nil && (request.Assistant != nil || request.Resource != nil) {
		return ui.Element{}, pluginNativeFault(protocol.ErrorCodeInvalidRequest, "invalid conversation resource")
	}
	key, _ := json.Marshal(request)
	mounted := state.conversations[identity]
	if mounted != nil && mounted.request != string(key) {
		return ui.Element{}, pluginNativeFault(protocol.ErrorCodeUnavailable, "conversation mount source changed")
	}
	suspended := state.native.Suspended()
	var target *viewmodel.Conversation
	if suspended {
		if mounted == nil || mounted.view == nil || mounted.context == nil {
			return ui.Box(view).Size(0, 0), nil
		}
		target = mounted.view
	} else {
		if err := state.workspaceComponent(protocol.PluginActionReadConversation, protocol.PluginActionControlConversation); err != nil {
			return ui.Element{}, err
		}
		if mounted == nil {
			session, err := state.conversationSession(request)
			if err != nil {
				return ui.Element{}, err
			}
			mounted = &pluginNativeConversation{request: string(key), session: session.ID}
			if session.ID == (protocol.SessionID{}) {
				mounted.draft = &session
				mounted.view, mounted.composer = state.newAssistantDraft(identity, session)
				mounted.draftID = mounted.view.ID
			}
			state.conversations[identity] = mounted
		}
		target = mounted.view
		if mounted.session != (protocol.SessionID{}) {
			session, available := state.runtime.session(mounted.session.String())
			if !available || session.Archived {
				return ui.Element{}, pluginNativeFault(protocol.ErrorCodeUnavailable, "captured conversation is unavailable")
			}
			target = state.runtime.App.Data.Find(session.ID.String())
			if target == nil {
				state.runtime.project()
				target = state.runtime.App.Data.Find(session.ID.String())
			}
			if target == nil || target.Node != state.runtime.controller.Target().String() {
				return ui.Element{}, pluginNativeFault(protocol.ErrorCodeUnavailable, "captured conversation is unavailable")
			}
			if mounted.composer == nil {
				mounted.composer = state.runtime.App.Composer(target.ID)
				state.runtime.read(target, true)
			}
		}
		mounted.view = target
	}
	comp := mounted.composer
	var heading string
	if raw := props["heading"]; len(raw) > 0 && json.Unmarshal(raw, &heading) != nil {
		return ui.Element{}, pluginNativeFault(protocol.ErrorCodeInvalidRequest, "invalid conversation heading")
	}
	region := ui.Column(view.Key(fmt.Sprintf("plugin-conversation-%s", identity))).Fill().MinWidth(0).MinHeight(0).Disabled(suspended)
	region.Children(func() {
		if request.Assistant != nil && heading != "" && state.native.DrawingDetails() {
			// In the Shell's right sidebar the heading and the session
			// controls are the sidebar header's.
			state.native.SetDetailsHeading(func(header *ui.Context) error {
				ui.Text(header, heading).FontSize(14).SingleLine().Grow(1).Shrink(1).MinWidth(0)
				state.sessionControls(header, identity, mounted, target, suspended)
				return nil
			})
		} else if request.Assistant != nil {
			// An assistant's panel moves between its own sessions: the
			// heading, then New and the session history.
			ui.Row(view.Key("session-controls")).FillWidth().MinWidth(0).Gap(4).AlignItems(ui.Center).Children(func() {
				ui.Text(view, heading).FontSize(14).FontWeight(600).SingleLine().Grow(1).Shrink(1).MinWidth(0)
				state.sessionControls(view, identity, mounted, target, suspended)
			})
		} else if heading != "" {
			ui.Text(view, heading).FontSize(14).FontWeight(600).SingleLine()
		}
		if !suspended {
			if mounted.draft != nil && mounted.session == (protocol.SessionID{}) {
				state.initializeAssistantComposer(comp, *mounted.draft)
			}
			mounted.context, mounted.actions = state.runtime.conversation(view, target, comp)
			if request.Assistant != nil {
				mounted.context.LocationLocked, mounted.context.Docked = true, true
				if actions, valid := mounted.actions.(*conversationActions); valid {
					actions.created = func(session protocol.Session) {
						state.releaseConversation(mounted)
						mounted.session = session.ID
						mounted.draft = nil
						if current := state.runtime.App.Data.Find(session.ID.String()); current != nil {
							mounted.view = current
						}
					}
				}
			}
		}
		context := *mounted.context
		context.Readonly = context.Readonly || suspended
		if request.Assistant != nil && len(target.Turns) == 0 && !target.Session.Loading {
			// An assistant's session without a message yet says what the
			// panel is for.
			ui.Column(view.Key("session-empty")).Grow(1).MinHeight(0).FillWidth().Justify(ui.Center).AlignItems(ui.Center).Children(func() {
				kit.Result(view, kit.ResultProps{Title: i18n.T(view, "chat_sidebar_empty"), Icon: icons.Bot})
			})
			composer.View(view, comp, &context, mounted.actions)
		} else {
			conversation.Docked(view, state.runtime.App.Messages, target.ID, target.Turns, &target.Session, mounted.actions, func() {
				composer.View(view, comp, &context, mounted.actions)
			})
		}
		conversation.Navigation(view, state.runtime.App.Messages)
	})
	return region, nil
}

// assistantSessions are the sessions of the assistant and resource session
// belongs to, in its project and worktree, newest first: what the panel's
// history lists.
func (state *pluginNativeState) assistantSessions(like protocol.Session) []protocol.Session {
	var sessions []protocol.Session
	all := state.runtime.snapshot().Sessions
	for index := len(all) - 1; index >= 0; index-- {
		session := all[index]
		assistant := session.Config.Assistant
		if session.Archived || session.Delegation != nil || assistant == nil || like.Config.Assistant == nil || *assistant != *like.Config.Assistant ||
			!pluginNativeResourceEqual(session.Config.Resource, like.Config.Resource) || !pluginNativeProjectEqual(session.Project, like.Project) ||
			like.Project != nil && session.Worktree != like.Worktree {
			continue
		}
		sessions = append(sessions, session)
	}
	return sessions
}

// sessionControls are New and the session history of an assistant's panel.
// They act while the Host is connected and no turn runs.
func (state *pluginNativeState) sessionControls(view *ui.Context, identity string, mounted *pluginNativeConversation, target *viewmodel.Conversation, suspended bool) {
	like, current := protocol.Session{}, false
	if mounted.draft != nil {
		like = *mounted.draft
	} else {
		like, current = state.runtime.session(mounted.session.String())
	}
	blocked := suspended || like.Config.Assistant == nil || !state.runtime.controller.Connected() || target.Running()
	// The mount moves to another session of the assistant, or to a draft
	// that keeps the composer's choices and asks before acting.
	move := func(to *protocol.Session) {
		next := &pluginNativeConversation{request: mounted.request}
		if to != nil {
			next.session = to.ID
		} else {
			draft := protocol.Session{Project: like.Project, Config: protocol.DefaultSessionConfig()}
			draft.Config.Assistant, draft.Config.Resource = like.Config.Assistant, like.Config.Resource
			if like.Project != nil {
				draft.Worktree = like.Worktree
			}
			previous := mounted.composer
			next.draft = &draft
			next.view, next.composer = state.newAssistantDraft(identity, draft)
			next.draftID = next.view.ID
			if previous != nil && previous != next.composer {
				next.composer.Model, next.composer.Effort, next.composer.Mode = previous.Model, previous.Effort, previous.Mode
				next.composer.Features = maps.Clone(previous.Features)
			}
		}
		state.releaseConversation(mounted)
		state.conversations[identity] = next
	}
	kit.IconButton(view.Key("session-new"), icons.Plus, i18n.T(view, "chat_new"), 28, false).
		Disabled(blocked || mounted.draft != nil).OnClick(func() { move(nil) })
	history := &mounted.history
	kit.IconButton(view.Key("session-history"), icons.BookOpen, i18n.T(view, "chat_connection_history"), 28, history.open).
		Disabled(blocked).OnClick(func() { history.open, history.query, history.row = true, "", 0 })
	if !history.open {
		return
	}
	if blocked {
		history.open = false
		return
	}
	sessions := state.assistantSessions(like)
	var shown []protocol.Session
	var items []kit.CommandItem
	query := strings.ToLower(history.query)
	for _, session := range sessions {
		title := session.Activity.Title
		if title == "" {
			title = i18n.T(view, "conversation")
		}
		if strings.Contains(strings.ToLower(title), query) {
			shown = append(shown, session)
			items = append(items, kit.CommandItem{ID: session.ID.String(), Label: title, Checked: current && session.ID == like.ID})
		}
	}
	empty := "connection_search_empty"
	if len(sessions) == 0 {
		empty = "chat_connection_history_empty"
	}
	kit.Command(view, "session-history-picker", &history.open, &history.query, &history.row, items,
		kit.CommandProps{Placeholder: i18n.T(view, "connection_search"), Empty: i18n.T(view, empty)}, func(index int) {
			history.open = false
			if chosen := shown[index]; !current || chosen.ID != like.ID {
				move(&chosen)
			}
		})
}

func (state *pluginNativeState) newAssistantDraft(identity string, session protocol.Session) (*viewmodel.Conversation, *composer.State) {
	target := &viewmodel.Conversation{
		Node: state.runtime.controller.Target().String(),
		ID:   fmt.Sprintf("draft/plugin/%p/%s", state, identity),
	}
	comp := state.runtime.App.Composer(target.ID)
	state.initializeAssistantComposer(comp, session)
	state.runtime.composerInitialized[target.ID] = true
	comp.Host, comp.Permission = target.Node, composer.AskFirst
	if session.Project != nil {
		comp.Project = session.Project.String()
		for _, worktree := range state.runtime.snapshot().Worktrees {
			if worktree.ID == session.Worktree {
				comp.Worktree = worktree.Path
				break
			}
		}
	}
	root := state.runtime.localRuntime()
	root.initPluginViews()
	root.pluginViews.drafts[target.ID] = target
	return target, comp
}

func (state *pluginNativeState) releaseConversation(mounted *pluginNativeConversation) {
	if mounted == nil || mounted.draftID == "" {
		return
	}
	root := state.runtime.localRuntime()
	if root.pluginViews != nil {
		delete(root.pluginViews.drafts, mounted.draftID)
	}
	mounted.draftID = ""
}

// releaseConversations ends the view's conversation mounts. Nothing else
// prunes a draft that never became a session and each view's draft key is
// new, so the draft's composer state goes with the view.
func (state *pluginNativeState) releaseConversations() {
	runtime := state.runtime
	for identity, mounted := range state.conversations {
		state.releaseConversation(mounted)
		delete(state.conversations, identity)
		id := fmt.Sprintf("draft/plugin/%p/%s", state, identity)
		if comp := runtime.App.DropComposer(id); comp != nil {
			delete(runtime.composerBases, comp)
		}
		delete(runtime.composerInitialized, id)
	}
}

func (state *pluginNativeState) initializeAssistantComposer(comp *composer.State, session protocol.Session) {
	if comp.Model == "" {
		state.runtime.defaultComposer(comp)
	}
	config, exists := state.runtime.composerBases[comp]
	if !exists {
		config = protocol.DefaultSessionConfig()
	}
	config.Assistant, config.Resource, config.Permission = session.Config.Assistant, session.Config.Resource, protocol.PermissionAsk
	state.runtime.composerBases[comp] = config
}

func (state *pluginNativeState) conversationSession(request pluginNativeConversationRequest) (protocol.Session, error) {
	info, available := state.packageInfo()
	if !available {
		return protocol.Session{}, pluginNativeFault(protocol.ErrorCodeUnavailable, "plugin conversation scope is unavailable")
	}
	snapshot := state.runtime.snapshot()
	if request.Assistant == nil {
		id := request.Session
		if id == nil {
			id = state.scope.Session
		}
		if id == nil {
			return protocol.Session{}, pluginNativeFault(protocol.ErrorCodeUnavailable, "conversation has no captured session")
		}
		for _, session := range snapshot.Sessions {
			if session.ID != *id || session.Archived {
				continue
			}
			for _, worktree := range snapshot.Worktrees {
				if worktree.ID == session.Worktree && pluginNativeProjectEqual(worktree.Project, session.Project) {
					return session, nil
				}
			}
		}
		return protocol.Session{}, pluginNativeFault(protocol.ErrorCodeNotFound, "captured conversation is unavailable")
	}
	if info.Extension == nil || info.Extension.Desktop == nil {
		return protocol.Session{}, pluginNativeFault(protocol.ErrorCodePermissionDenied, "plugin assistant is not declared")
	}
	var declaration *protocol.PluginConversationDeclaration
	for _, entry := range info.Extension.Desktop.Conversations {
		if entry.ID == *request.Assistant {
			value := entry
			declaration = &value
			break
		}
	}
	if declaration == nil {
		return protocol.Session{}, pluginNativeFault(protocol.ErrorCodePermissionDenied, "plugin assistant is not declared")
	}
	var project *protocol.ProjectID
	switch declaration.Resource {
	case protocol.PluginConversationResourcePlugin:
		if request.Resource != nil {
			return protocol.Session{}, pluginNativeFault(protocol.ErrorCodeInvalidRequest, "plugin assistant does not accept a resource")
		}
	case protocol.PluginConversationResourceWorkspace:
		if request.Resource != nil || state.scope.Worktree == nil {
			return protocol.Session{}, pluginNativeFault(protocol.ErrorCodeInvalidRequest, "assistant requires a captured workspace")
		}
		found := false
		for _, worktree := range snapshot.Worktrees {
			if worktree.ID == *state.scope.Worktree {
				project, found = worktree.Project, true
				break
			}
		}
		if !found || project == nil {
			return protocol.Session{}, pluginNativeFault(protocol.ErrorCodeUnavailable, "assistant workspace is unavailable")
		}
	case protocol.PluginConversationResourceDatabase:
		if request.Resource == nil || request.Resource.Kind != "database" {
			return protocol.Session{}, pluginNativeFault(protocol.ErrorCodeInvalidRequest, "assistant requires a captured database")
		}
		id, valid := detail[protocol.DatabaseID](request.Resource.Data)
		if !valid || !slices.ContainsFunc(snapshot.Databases, func(profile protocol.DatabaseProfile) bool { return profile.ID == id }) {
			return protocol.Session{}, pluginNativeFault(protocol.ErrorCodeUnavailable, "assistant database is unavailable")
		}
	case protocol.PluginConversationResourceSsh:
		if request.Resource == nil || request.Resource.Kind != "ssh" {
			return protocol.Session{}, pluginNativeFault(protocol.ErrorCodeInvalidRequest, "assistant requires a captured SSH profile")
		}
		id, valid := detail[protocol.SshID](request.Resource.Data)
		if !valid || !slices.ContainsFunc(snapshot.Ssh, func(profile protocol.SshProfile) bool { return profile.ID == id }) {
			return protocol.Session{}, pluginNativeFault(protocol.ErrorCodeUnavailable, "assistant SSH profile is unavailable")
		}
	default:
		return protocol.Session{}, pluginNativeFault(protocol.ErrorCodeInvalidRequest, "invalid assistant resource declaration")
	}
	for index := len(snapshot.Sessions) - 1; index >= 0; index-- {
		session := snapshot.Sessions[index]
		assistant := session.Config.Assistant
		if session.Archived || session.Delegation != nil || assistant == nil || assistant.ID != declaration.ID ||
			assistant.Package != state.scope.Package || !pluginNativeResourceEqual(session.Config.Resource, request.Resource) || !pluginNativeProjectEqual(session.Project, project) ||
			project != nil && session.Worktree != *state.scope.Worktree {
			continue
		}
		return session, nil
	}
	session := protocol.Session{Project: project, Config: protocol.DefaultSessionConfig()}
	session.Config.Assistant = &protocol.PluginConversationBinding{Package: state.scope.Package, ID: declaration.ID}
	session.Config.Resource = request.Resource
	if project != nil {
		session.Worktree = *state.scope.Worktree
	}
	return session, nil
}

func pluginNativeResourceEqual(first, second *protocol.ConnectionResource) bool {
	if first == nil || second == nil {
		return first == nil && second == nil
	}
	if first.Kind != second.Kind {
		return false
	}
	switch first.Kind {
	case "database":
		left, leftValid := detail[protocol.DatabaseID](first.Data)
		right, rightValid := detail[protocol.DatabaseID](second.Data)
		return leftValid && rightValid && left == right
	case "ssh":
		left, leftValid := detail[protocol.SshID](first.Data)
		right, rightValid := detail[protocol.SshID](second.Data)
		return leftValid && rightValid && left == right
	default:
		return false
	}
}

func pluginNativeProjectEqual(first, second *protocol.ProjectID) bool {
	return first == nil && second == nil || first != nil && second != nil && *first == *second
}
