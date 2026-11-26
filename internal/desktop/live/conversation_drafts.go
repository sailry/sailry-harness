package live

import (
	"fmt"
	"slices"
	"strings"
	"time"

	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/panes"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// A draft is a conversation of the desktop alone until its first send creates
// the Node session. It stays in the shell's data, which shows it, and out of
// the navigation column, which lists sessions.

func unsentDraft(cv *viewmodel.Conversation) bool { return strings.HasPrefix(cv.ID, "draft/") }

// newDraft opens the unsent conversation of a location: the one it already
// has, with whatever was typed in it, otherwise a new one.
func (r *Runtime) newDraft(p *viewmodel.Project) {
	r.openDraft(r.draftIn(p), p)
}

// Explicit tab and split creation needs an independent composer even while the
// location already has an unsent draft. Ordinary New session still reuses it.
func (r *Runtime) newConversationDraft(p *viewmodel.Project) {
	r = r.localRuntime()
	r.rememberDraftComposers()
	node := ""
	if p == nil {
		if current := r.App.Data.Find(r.App.FocusedConversation()); current != nil {
			node = current.Node
		}
	}
	cv := r.createDraftIn(p)
	if node != "" {
		cv.Node = node
		r.App.Composer(cv.ID).Host = node
	}
	r.openDraft(cv, p)
}

func (r *Runtime) openDraft(cv *viewmodel.Conversation, p *viewmodel.Project) {
	if p != nil {
		comp := r.App.Composer(cv.ID)
		comp.Host, comp.Project = p.Host, p.ID
		r.App.Sidebar.Reveal(p.ID)
	}
	r.App.Page = shell.ConversationPage
	r.App.Sidebar.Selected = cv.ID
	r.listSidebar()
}

func (r *Runtime) draftIn(p *viewmodel.Project) *viewmodel.Conversation {
	if list := r.draftList(p); list != nil {
		for _, current := range *list {
			// A submitted draft is becoming its session.
			if unsentDraft(current) && !r.draftSubmitted(current) {
				return current
			}
		}
	}
	return r.createDraftIn(p)
}

func (r *Runtime) draftList(p *viewmodel.Project) *[]*viewmodel.Conversation {
	if p == nil {
		return &r.App.Data.Unassigned
	}
	for _, current := range r.App.Data.Projects {
		if current.ID == p.ID && current.Host == p.Host {
			return &current.Conversations
		}
	}
	return nil
}

func (r *Runtime) createDraftIn(p *viewmodel.Project) *viewmodel.Conversation {
	r.drafts++
	cv := &viewmodel.Conversation{Node: r.controller.Target().String(), ID: fmt.Sprintf("draft/%d", r.drafts), Updated: time.Now(), Created: int(^uint(0) >> 1)}
	if p != nil {
		cv.Node = p.Host
	}
	if r.openDrafts == nil {
		r.openDrafts = map[string]bool{}
	}
	r.openDrafts[cv.ID] = true
	if list := r.draftList(p); list != nil {
		*list = append([]*viewmodel.Conversation{cv}, *list...)
	}
	return cv
}

// draftSubmitted reports whether a draft's session is being created on any
// Node, or a creation of it has an unknown outcome.
func (r *Runtime) draftSubmitted(cv *viewmodel.Conversation) bool {
	root := r.localRuntime()
	submitted := func(owner *Runtime) bool {
		_, captured := owner.conversationCreations[cv.ID]
		return captured || owner.conversationCreating[cv.ID]
	}
	if submitted(root) {
		return true
	}
	for _, bound := range root.bindings {
		if submitted(bound) {
			return true
		}
	}
	return false
}

func (r *Runtime) removeDraft(id string) {
	r = r.localRuntime()
	remove := func(list []*viewmodel.Conversation) []*viewmodel.Conversation {
		out := list[:0]
		for _, cv := range list {
			if cv.ID != id {
				out = append(out, cv)
			}
		}
		return out
	}
	r.App.Data.Unassigned = remove(r.App.Data.Unassigned)
	for _, p := range r.App.Data.Projects {
		p.Conversations = remove(p.Conversations)
	}
	delete(r.openDrafts, id)
	r.applyConversationData(r.App.Data)
	r.App.ForgetSession(id)
}

func (r *Runtime) applyConversationData(data *viewmodel.Data) {
	app := r.App
	previous, selected := app.Data, app.Sidebar.Selected
	focused := app.FocusedConversation()
	target := panes.Target{Kind: panes.Session, ID: focused}
	var peers []panes.Target
	var terminals []shell.WorkspaceTerminal
	if focused != "" && data.Find(focused) == nil {
		if cv := previous.Find(focused); cv == nil || !unsentDraft(cv) {
			// Applying the data prunes the vanished pane, possibly dissolving its split.
			layout := app.Prefs()
			workspace := panes.Workspace{Groups: layout.Panes}
			peers, terminals = workspace.Members(target), layout.WorkspaceTerminals
		}
	}
	app.ApplyData(data)
	// A background selection change would make the shell leave the current
	// resource page or terminal. Keep it until a conversation needs selection.
	app.Sidebar.Selected = selected
	r.listSidebar()
	r.selectPendingSession()
	if app.Page != shell.ConversationPage || data.Find(app.Sidebar.Selected) != nil {
		return
	}
	defer r.fallBackSelection(app.Sidebar.Selected)
	for _, peer := range peers {
		if peer == target {
			continue
		}
		switch peer.Kind {
		case panes.Session:
			if previous.Find(peer.ID) != nil && data.Find(peer.ID) != nil {
				app.Sidebar.Selected = peer.ID
				return
			}
		case panes.Terminal:
			if i := slices.IndexFunc(terminals, func(t shell.WorkspaceTerminal) bool { return t.ID == peer.ID }); i >= 0 {
				app.OpenWorkspaceTerminal(peer.ID, terminals[i].Title)
				return
			}
		}
	}
	r.ensureConversationSelection()
}

func (r *Runtime) ensureConversationSelection() {
	app := r.App
	selected := app.Sidebar.Selected
	if app.Page != shell.ConversationPage || app.Data.Find(selected) != nil {
		return
	}
	defer r.fallBackSelection(selected)
	focused := app.FocusedConversation()
	if focused == "" && selected != "" {
		// An independent terminal owns the main workspace.
		return
	}
	if app.Data.Find(focused) != nil {
		app.Sidebar.Selected = focused
	} else if ids := app.Sidebar.Order(); len(ids) > 0 {
		app.Sidebar.Selected = ids[0]
	} else {
		r.newDraft(nil)
	}
}

// listSidebar excludes drafts and resource-owned assistants from ordinary
// navigation. The shell keeps them for embedded views and explicit session links.
func (r *Runtime) listSidebar() {
	type key struct{ node, session string }
	owned := map[key]bool{}
	collect := func(owner *Runtime) {
		node := owner.controller.Target().String()
		add := func(session protocol.Session) {
			if session.Config.Assistant != nil || session.Config.Resource != nil {
				owned[key{node, session.ID.String()}] = true
			}
		}
		for _, session := range owner.snapshot().Sessions {
			add(session)
		}
		// First admission may reach the mounted view before its snapshot.
		for _, session := range owner.admittedSessions {
			add(session)
		}
	}
	root := r.localRuntime()
	collect(root)
	for _, owner := range root.bindings {
		collect(owner)
	}
	excluded := func(cv *viewmodel.Conversation) bool {
		return unsentDraft(cv) || owned[key{cv.Node, cv.ID}]
	}
	data := r.App.Data
	listed := *data
	listed.Unassigned = slices.DeleteFunc(slices.Clone(data.Unassigned), excluded)
	hidden := len(listed.Unassigned) != len(data.Unassigned)
	listed.Projects = slices.Clone(data.Projects)
	for index, project := range listed.Projects {
		if slices.ContainsFunc(project.Conversations, excluded) {
			copied := *project
			copied.Conversations = slices.DeleteFunc(slices.Clone(project.Conversations), excluded)
			listed.Projects[index] = &copied
			hidden = true
		}
	}
	if !hidden {
		r.App.Sidebar.Data = data
		return
	}
	r.App.Sidebar.Data = &listed
}

// pruneDrafts drops abandoned New session drafts with no input to keep.
func (r *Runtime) pruneDrafts() {
	for id, discardable := range r.openDrafts {
		cv := r.App.Data.Find(id)
		if cv == nil {
			// The draft became its session, or was removed.
			delete(r.openDrafts, id)
			continue
		}
		if !discardable {
			continue
		}
		if r.draftSubmitted(cv) {
			// A failed first send can leave recovery input after the request
			// settles and the visible editor is cleared.
			r.openDrafts[id] = false
			continue
		}
		if id == r.App.Sidebar.Selected || r.draftsShown[id] {
			continue
		}
		comp := r.App.Composer(id)
		if comp.Draft != "" || len(comp.Attachments) != 0 || len(comp.References) != 0 || comp.Dictation.Phase != composer.Idle {
			continue
		}
		layout := r.App.Prefs()
		workspace := panes.Workspace{Groups: layout.Panes}
		target := panes.Target{Kind: panes.Session, ID: id}
		if workspace.Group(target) >= 0 || slices.Contains(layout.Tabs, target) {
			continue
		}
		r.removeDraft(id)
		r.releaseDraftComposer(id)
	}
}

func (r *Runtime) releaseDraftComposer(id string) {
	comp := r.App.DropComposer(id)
	release := func(owner *Runtime) {
		delete(owner.composerCatalogs, id)
		delete(owner.composerInitialized, id)
		delete(owner.composerObserved, id)
		delete(owner.composerRevisions, id)
		owner.clearComposerBase(comp)
		delete(owner.sessionBases, comp)
		if owner.teams != nil {
			delete(owner.teams.composers, id)
		}
	}
	release(r)
	delete(r.localRuntime().usage.dictated, comp)
	for _, bound := range r.bindings {
		release(bound)
	}
}
