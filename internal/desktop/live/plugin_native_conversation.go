package live

import (
	"encoding/json"
	"fmt"
	"slices"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
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
		if heading != "" {
			ui.Text(view, heading).FontSize(14).FontWeight(600).SingleLine()
		}
		if !suspended {
			if mounted.draft != nil && mounted.session == (protocol.SessionID{}) {
				state.initializeAssistantComposer(comp, *mounted.draft)
			}
			mounted.context, mounted.actions = state.runtime.conversation(view, target, comp)
			if request.Assistant != nil {
				mounted.context.LocationLocked = true
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
		conversation.View(view, state.runtime.App.Messages, target.ID, target.Turns, &target.Session, mounted.actions)
		composer.View(view, comp, &context, mounted.actions)
	})
	return region, nil
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
		for _, worktree := range state.runtime.controller.Snapshot().Worktrees {
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
	snapshot := state.runtime.controller.Snapshot()
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
