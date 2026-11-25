package live

import (
	"fmt"
	"maps"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
	"github.com/sailry/sailry-harness/sdk/desktop/view"
)

type windowDockedConversation struct {
	runtime         *Runtime
	spec            view.ConversationSpec
	resource        *protocol.ConnectionResource
	session, viewed protocol.SessionID
	draft           *viewmodel.Conversation
	draftID         string
	history         dockedSessionHistory
	tools           *dockedToolObserver
	closed          bool
}

func (h *windowViewHost) mountConversation(spec view.ConversationSpec) view.Conversation {
	r := h.runtime
	if spec.Match == nil || spec.Config == nil || !h.open() || r.App.DemoOn() {
		return view.Conversation{}
	}
	resource := spec.Config(protocol.DefaultSessionConfig()).Resource
	if resource != nil && !connectionResourceExists(r.snapshot(), resource) {
		return view.Conversation{}
	}
	mounted := &windowDockedConversation{runtime: r, spec: spec, resource: resource}
	if spec.Tools != nil && spec.Tools.Publish != nil {
		mounted.tools = newDockedToolObserver(r.ctx, *spec.Tools)
	}
	return view.Conversation{
		Render: mounted.render, Header: mounted.header,
		Close: h.trackMount(nativeWindowResource{
			afterFrame: func() {
				available := mounted.resourceAvailable()
				if r.controller.Connected() && !available {
					mounted.releaseDraft()
				}
				if mounted.tools != nil {
					mounted.observeTools(available)
				}
			},
			close: mounted.close,
		}),
	}
}

func (mounted *windowDockedConversation) resourceAvailable() bool {
	return mounted.resource == nil || connectionResourceExists(mounted.runtime.snapshot(), mounted.resource)
}

func (mounted *windowDockedConversation) match(session protocol.Session) bool {
	return !session.Archived && session.Delegation == nil &&
		(mounted.resource == nil || pluginNativeResourceEqual(mounted.resource, session.Config.Resource)) && mounted.spec.Match(session)
}

func (mounted *windowDockedConversation) sessions() []protocol.Session {
	all := mounted.runtime.visibleSessions()
	var matching []protocol.Session
	for i := len(all) - 1; i >= 0; i-- {
		if mounted.match(all[i]) {
			matching = append(matching, all[i])
		}
	}
	return matching
}

func (mounted *windowDockedConversation) selectTarget() {
	if mounted.draft != nil {
		return
	}
	r := mounted.runtime
	if session, ok := r.sessionWithID(mounted.session); ok && !r.sessionHidden(session.ID) && mounted.match(session) {
		return
	}
	all := r.visibleSessions()
	for i := len(all) - 1; i >= 0; i-- {
		if mounted.match(all[i]) {
			mounted.session = all[i].ID
			return
		}
	}
	mounted.newDraft(nil)
}

func (mounted *windowDockedConversation) target() *viewmodel.Conversation {
	if mounted.closed || mounted.runtime.ctx.Err() != nil || mounted.runtime.App.DemoOn() || !mounted.resourceAvailable() {
		return nil
	}
	mounted.selectTarget()
	if mounted.draft != nil {
		return mounted.draft
	}
	r := mounted.runtime
	if target := r.App.Data.Find(mounted.session.String()); target != nil && target.Node == r.controller.Target().String() {
		return target
	}
	return nil
}

func (mounted *windowDockedConversation) header(c *ui.Context, heading string) {
	if mounted.closed || mounted.runtime.App.DemoOn() || mounted.runtime.ctx.Err() != nil || !mounted.resourceAvailable() {
		return
	}
	target := mounted.target()
	ui.Text(c, heading).FontSize(14).SingleLine().Grow(1).Shrink(1).MinWidth(0)
	r := mounted.runtime
	blocked := !r.controller.Connected() || target == nil || target.Running() || r.draftSubmitted(target)
	dockedSessionControls(c, &mounted.history, blocked, mounted.draft != nil, mounted.session, mounted.sessions, func(session *protocol.Session) {
		if target == nil || mounted.target() != target || !r.controller.Connected() || target.Running() || r.draftSubmitted(target) {
			return
		}
		if session == nil {
			mounted.newDraft(r.App.Composer(target.ID))
			return
		}
		current, ok := r.sessionWithID(session.ID)
		if !ok || r.sessionHidden(current.ID) || !mounted.match(current) {
			return
		}
		mounted.releaseDraft()
		mounted.session = session.ID
		mounted.viewed = protocol.SessionID{}
	})
}

func (mounted *windowDockedConversation) render(c *ui.Context) {
	target := mounted.target()
	if target == nil {
		return
	}
	r := mounted.runtime
	comp := r.App.Composer(target.ID)
	if mounted.session != (protocol.SessionID{}) && mounted.viewed != mounted.session {
		mounted.viewed = mounted.session
		r.read(target, true)
	}
	if mounted.draft != nil && comp.Model == "" {
		revision := r.composerBaseRevision
		r.defaultComposer(comp)
		if r.composerBaseRevision != revision {
			mounted.scopeDraft(comp)
		}
	}
	c = c.Key(target.ID)
	ui.Column(c).Fill().MinWidth(0).MinHeight(0).Children(func() {
		context, actions := r.conversation(c, target, comp)
		context.LocationLocked, context.Docked = true, true
		if draft, valid := actions.(*conversationActions); valid && mounted.draft == target {
			draft.created = func(session protocol.Session) {
				if !mounted.closed && mounted.draft == target && mounted.match(session) {
					mounted.releaseDraft()
					mounted.session = session.ID
				}
			}
		}
		r.drawDockedConversation(c, target, comp, context, actions, true)
	})
}

func (mounted *windowDockedConversation) newDraft(previous *composer.State) {
	r := mounted.runtime
	mounted.releaseDraft()
	r.drafts++
	id := fmt.Sprintf("draft/native/%d", r.drafts)
	target := &viewmodel.Conversation{Node: r.controller.Target().String(), ID: id}
	comp := r.App.Composer(id)
	if previous != nil {
		comp.Model, comp.Effort, comp.Mode = previous.Model, previous.Effort, previous.Mode
		comp.Features = maps.Clone(previous.Features)
	}
	if comp.Model == "" {
		r.defaultComposer(comp)
	}
	comp.Host = target.Node
	mounted.scopeDraft(comp)
	r.composerInitialized[id] = true
	r.pluginViews.drafts[id] = target
	mounted.session, mounted.viewed = protocol.SessionID{}, protocol.SessionID{}
	mounted.draft, mounted.draftID = target, id
}

func (mounted *windowDockedConversation) scopeDraft(comp *composer.State) {
	r := mounted.runtime
	base, exists := r.composerBases[comp]
	if !exists {
		base = protocol.DefaultSessionConfig()
	}
	base.Assistant = nil
	base = mounted.spec.Config(base)
	if mounted.resource != nil {
		base.Resource = mounted.resource
	}
	base.Permission = protocol.PermissionAsk
	comp.Permission = composer.AskFirst
	r.setComposerBase(comp, base)
}

func (mounted *windowDockedConversation) releaseDraft() {
	if id := mounted.draftID; id != "" {
		mounted.draft, mounted.draftID = nil, ""
		delete(mounted.runtime.pluginViews.drafts, id)
		mounted.runtime.releaseDraftComposer(id)
	}
}

func (mounted *windowDockedConversation) close() {
	mounted.closed = true
	if mounted.tools != nil {
		mounted.tools.close()
	}
	mounted.releaseDraft()
}
