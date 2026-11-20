package live

import (
	"context"

	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func (r *Runtime) localRuntime() *Runtime {
	if r.parent != nil {
		return r.parent
	}
	return r
}

func (r *Runtime) runtimeFor(node string) *Runtime {
	root := r.localRuntime()
	if node == "" || node == root.controller.Target().String() {
		return root
	}
	if bound := root.bindings[node]; bound != nil {
		return bound
	}
	return nil
}

func (r *Runtime) conversationRuntime(cv *viewmodel.Conversation, comp *composer.State) *Runtime {
	root := r.localRuntime()
	if cv == nil {
		return root
	}
	if unsentDraft(cv) {
		if pending := root.guardedCreation(cv.ID); pending != nil {
			cv.Node = pending.request.Target.String()
			if comp != nil {
				comp.Host = cv.Node
			}
			return root.runtimeFor(cv.Node)
		}
	}
	node := cv.Node
	if comp != nil && len(cv.ID) >= 6 && cv.ID[:6] == "draft/" {
		previous := cv.Node
		if comp.Project == "" && comp.Host != "" {
			node = comp.Host
		}
		if comp.Project != "" {
			var matches []*viewmodel.Project
			for _, project := range root.App.Data.Projects {
				if project.ID == comp.Project {
					matches = append(matches, project)
					if project.Host == comp.Host {
						node = project.Host
						matches = nil
						break
					}
				}
			}
			if len(matches) == 1 {
				node = matches[0].Host
			} else if len(matches) > 1 {
				for _, project := range matches {
					if project.Host == cv.Node {
						node = project.Host
						break
					}
				}
			}
		}
		cv.Node, comp.Host = node, node
		if previous != "" && previous != node {
			root.resetDraftComposer(cv.ID, comp)
			for _, bound := range root.bindings {
				bound.resetDraftComposer(cv.ID, comp)
			}
			comp.Model, comp.Effort = "", 0
			comp.Features, comp.Team, comp.MemberModels = nil, "", nil
			if owner := root.runtimeFor(node); owner != nil {
				owner.initializeDraftComposer(comp)
			}
		}
	} else if comp != nil {
		// An admitted session and every queue action retain its execution
		// Node even when a different draft or device monitor is selected.
		comp.Host = node
	}
	return root.runtimeFor(node)
}

func (r *Runtime) selectedRuntime() *Runtime {
	root := r.localRuntime()
	if node, _, shown := root.App.ProjectOwner(); shown {
		return root.runtimeFor(node)
	}
	cv := root.App.Data.Find(root.App.Sidebar.Selected)
	if cv == nil {
		return root
	}
	return root.conversationRuntime(cv, root.App.Composer(cv.ID))
}

func (r *Runtime) SessionClient(sessionID string) (*client.Client, bool) {
	root := r.localRuntime()
	cv := root.App.Data.Find(sessionID)
	if cv == nil {
		var controller *client.Client
		find := func(owner *Runtime) bool {
			if _, ok := owner.session(sessionID); !ok {
				return true
			}
			if controller != nil {
				return false
			}
			controller = owner.controller
			return true
		}
		if !find(root) {
			return nil, false
		}
		for _, bound := range root.bindings {
			if !find(bound) {
				return nil, false
			}
		}
		return controller, controller != nil
	}
	owner := root
	if cv.Node != "" && cv.Node != root.controller.Target().String() {
		owner = root.bindings[cv.Node]
	}
	if owner == nil {
		return nil, false
	}
	if _, ok := owner.session(sessionID); !ok {
		return nil, false
	}
	return owner.controller, true
}

func (r *Runtime) seenConversation(id protocol.SessionID) bool {
	if r.conversationSeen[id] {
		return true
	}
	for _, bound := range r.bindings {
		if bound.conversationSeen[id] {
			return true
		}
	}
	return false
}

func (r *Runtime) models() []composer.Model {
	if r.parent != nil {
		return r.nodeModels
	}
	return r.App.Data.Models
}

func (r *Runtime) newBinding(controller *client.Client) *Runtime {
	controller.OnDispatch(countSaved(r.controller))
	ctx, cancel := context.WithCancel(r.ctx)
	bound := &Runtime{
		parent: r, network: r.network, controller: controller, ctx: ctx, cancel: cancel,
		readsConnected: controller.Connected(),
		App:            r.App, Preferences: r.Preferences, busy: map[string]bool{}, uncertain: map[string]protocol.Request{},
		conversationWatches: map[protocol.SessionID]func(){}, conversationSeen: map[protocol.SessionID]bool{},
		composerInitialized: map[string]bool{}, composerObserved: map[string]string{}, composerRevisions: map[string]uint64{},
		composerSaving: map[string]bool{}, composerWaiters: map[string][]func(bool){}, composerBases: map[*composer.State]protocol.SessionConfig{}, sessionBases: map[*composer.State]*sessionBaseStart{},
		admittedSessions: map[string]protocol.Session{}, conversationEdits: map[string]conversationEdit{}, conversationPending: map[string]protocol.TurnID{},
		conversationCreations: map[string]*conversationCreation{}, conversationCreating: map[string]bool{},
		providerPresets: map[string]protocol.ConversationPresetsEntry{}, providerModels: map[string]providerDiscovery{},
		providerDraftBases: map[string]protocol.ConversationProvider{}, teams: newTeamState(),
		openService: r.openService, openWindow: r.openWindow, readAloud: r.readAloud, exportConversation: r.exportConversation,
		pickDirectory: r.pickDirectory,
	}
	bound.initCommands()
	bound.initAttachments()
	bound.initWorkspace()
	if r.attachments != nil {
		bound.attachments.native = r.attachments.native
	}
	if r.workspace != nil {
		bound.workspace.native = r.workspace.native
	}
	bound.unsubscribe = controller.OnChange(r.changed)
	bound.projectModels()
	bound.restoreDetachedCreations()
	return bound
}

func (r *Runtime) SetNetwork(network *Network) {
	if r.unsubscribeNetwork != nil {
		r.unsubscribeNetwork()
		r.unsubscribeNetwork = nil
	}
	r.network = network
	if r.bindings == nil {
		r.bindings = map[string]*Runtime{}
	}
	if network != nil {
		r.unsubscribeNetwork = network.onChange(r.changed)
	}
	r.initConnectionActions()
	r.changed()
}

func (r *Runtime) syncNetwork() {
	if r.network == nil {
		return
	}
	present := map[string]bool{}
	for _, peer := range r.network.snapshot() {
		id := peer.address.ID.String()
		if peer.controller == nil || !r.resourceVisible(id) {
			continue
		}
		present[id] = true
		if old := r.bindings[id]; old != nil && old.controller != peer.controller {
			r.retireBinding(id, old)
		}
		if peer.controller != nil && r.bindings[id] == nil {
			r.bindings[id] = r.newBinding(peer.controller)
		}
	}
	for id, bound := range r.bindings {
		if !present[id] {
			if r.transferDialogOwner == bound {
				r.transferDialogOwner = nil
			}
			if r.shownSettings() == bound {
				// The pages show no Node until the frame turns them to the
				// selected host's.
				r.showNodeSettings(nil)
			}
			r.retireBinding(id, bound)
		}
	}
}

func (r *Runtime) retireBinding(id string, bound *Runtime) {
	for draft, pending := range bound.conversationCreations {
		if pending.dispatched {
			bound.detachCreation(draft, pending)
		}
	}
	clear(bound.conversationSeen)
	bound.finishConversationFrame()
	r.detachBindingTerminals(bound)
	_ = bound.Close()
	delete(r.bindings, id)
}

func (r *Runtime) workspaceRuntime(worktree protocol.WorktreeID) *Runtime {
	root := r.localRuntime()
	owns := func(owner *Runtime) bool {
		if owner.workspace != nil {
			if _, ok := owner.workspace.admitted[worktree]; ok {
				return true
			}
			if _, ok := owner.workspace.projects[worktree]; ok {
				return true
			}
		}
		for _, tree := range owner.snapshot().Worktrees {
			if tree.ID == worktree {
				return true
			}
		}
		return false
	}
	if owns(root) {
		return root
	}
	for _, bound := range root.bindings {
		if owns(bound) {
			return bound
		}
	}
	return r
}

func (r *Runtime) bindArtifactCallbacks() {
	root := r.localRuntime()
	root.App.Panel.SetArtifactCallbacks(panel.ArtifactCallbacks{OpenExternal: root.OpenArtifactExternal, Image: root.ArtifactImageAction, Render: root.renderArtifactPreview, Markdown: root.artifactMarkdown,
		Versions: root.artifactVersions, ShowVersion: root.showArtifactVersion})
}

func (r *Runtime) SetPickKeyFile(pick func(func(string))) {
	r.pickKeyFile = pick
}
