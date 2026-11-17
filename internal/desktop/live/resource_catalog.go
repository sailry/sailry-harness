package live

import (
	"encoding/json"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/preferences"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// The Node owns durable directory recovery. The window observes that same
// directory even before Network has established a direct resource Client.
func (r *Runtime) observeResourceCatalog() {
	observer, err := r.controller.Watch(r.ctx, "watch_resources", nil)
	if err != nil {
		return
	}
	if !r.observe(func() {
		defer observer.Close()
		for {
			body, err := observer.Next(r.ctx)
			if err != nil {
				return
			}
			var view struct {
				Catalog *protocol.ResourceCatalog `json:"catalog"`
			}
			if json.Unmarshal(body, &view) != nil || view.Catalog == nil {
				continue
			}
			r.enqueue(func(*ui.Context) {
				r.resourceCatalog = view.Catalog
				r.pruneResourceViews()
				r.syncNetwork()
				r.project()
			})
		}
	}) {
		_ = observer.Close()
	}
}

// Only a confirmed directory can remove an open projection. A temporary
// source fault retains its workspace and local drafts for reconnection.
func (r *Runtime) pruneResourceViews() {
	if r.resourceCatalog == nil {
		return
	}
	r.pruneKnowledgeView()
	sources := map[string]protocol.ResourceSource{}
	for _, source := range r.resourceCatalog.Sources {
		sources[source.Node.String()] = source
	}
	for id, owner := range r.bindings {
		source, present := sources[id]
		restricted := !present || restrictedResourceSource(source)
		if !restricted && (source.Fault != nil || source.LastSyncedAtMs == nil || source.Revision < owner.snapshot().Cursor) {
			continue
		}
		if restricted {
			owner.invalidateFilePreview(nil)
		}
		if owner.workspace == nil {
			continue
		}
		projects, worktrees := map[protocol.ProjectID]bool{}, map[protocol.WorktreeID]bool{}
		if !restricted {
			for _, project := range source.Projects {
				projects[project.ID] = true
			}
			for _, worktree := range source.Worktrees {
				worktrees[worktree.ID] = true
			}
		}
		for worktree, project := range owner.workspace.projects {
			if worktrees[worktree] && (project.owner.Project == nil || projects[*project.owner.Project]) {
				continue
			}
			project.stopGitRefreshTimer()
			project.panel.ResetFileSearch()
			if project.cancel != nil {
				project.cancel()
			}
			if project.release != nil {
				project.release()
			}
			owner.invalidateFilePreview(&worktree)
			delete(owner.workspace.projects, worktree)
			delete(owner.workspace.admitted, worktree)
			if owner.workspace.selected == worktree {
				owner.workspace.selected = protocol.WorktreeID{}
				r.App.Panel.Actions = nil
				r.App.Panel.Data = &viewmodel.Workspace{}
			}
		}
	}
}

func (r *Runtime) resourceVisible(id string) bool {
	root := r.localRuntime()
	if id == root.controller.Target().String() || root.resourceCatalog == nil {
		return true
	}
	for _, source := range root.resourceCatalog.Sources {
		if source.Node.String() == id {
			return !restrictedResourceSource(source)
		}
	}
	return false
}

func restrictedResourceSource(source protocol.ResourceSource) bool {
	if source.Fault == nil {
		return false
	}
	switch source.Fault.Code {
	case protocol.ErrorCodePermissionDenied, protocol.ErrorCodeNotFound, protocol.ErrorCodeExpired:
		return true
	}
	return false
}

// resourceSidebar keeps live session observations separate from the durable
// directory: a cached project is not a cached filesystem or conversation.
func resourceSidebar(source protocol.ResourceSource, snapshot protocol.Snapshot, connected bool, prefs preferences.Data) *viewmodel.Data {
	snapshot.Node = source.Node
	if source.Revision >= snapshot.Cursor {
		snapshot.Projects, snapshot.Worktrees = source.Projects, source.Worktrees
	}
	data := Sidebar(snapshot, prefs)
	offline := source.Fault != nil || !connected
	var synced time.Time
	if source.LastSyncedAtMs != nil {
		synced = time.UnixMilli(int64(*source.LastSyncedAtMs))
	}
	mark := func(cv *viewmodel.Conversation) {
		cv.Offline, cv.LastSyncedAt = offline, synced
		markOfflineConversation(cv)
	}
	for _, project := range data.Projects {
		project.Offline, project.LastSyncedAt = offline, synced
		for _, cv := range project.Conversations {
			mark(cv)
		}
	}
	for _, cv := range data.Unassigned {
		mark(cv)
	}
	return data
}

func markOfflineConversation(cv *viewmodel.Conversation) {
	if !cv.Offline {
		return
	}
	cv.Session.Connection = conversation.Offline
	cv.Animating = false
	if cv.Status == viewmodel.Running || cv.Status == viewmodel.Waiting {
		cv.Status = viewmodel.Idle
	}
	for index := range cv.Turns {
		reply := &cv.Turns[index].Reply
		if reply.Status.Active() {
			reply.Status = conversation.Unsynced
			reply.LoginRecovery = false
		}
	}
}

func (r *Runtime) appendResourceDirectory(data *viewmodel.Data) {
	seen := map[string]bool{}
	if r.resourceCatalog != nil {
		for _, source := range r.resourceCatalog.Sources {
			id := source.Node.String()
			if source.Node == r.controller.Target() {
				continue
			}
			seen[id] = true
			if restrictedResourceSource(source) {
				continue
			}
			snapshot := protocol.Snapshot{Node: source.Node}
			connected := false
			if bound := r.bindings[id]; bound != nil {
				snapshot = bound.snapshot()
				bound.projectAdmission(&snapshot)
				connected = bound.controller.Connected()
			}
			part := resourceSidebar(source, snapshot, connected, r.Preferences.Data)
			data.Projects = append(data.Projects, part.Projects...)
			data.Unassigned = append(data.Unassigned, part.Unassigned...)
		}
	}
	for id, bound := range r.bindings {
		if seen[id] {
			continue
		}
		// Before the first directory response, keep the existing live
		// projection. A completed catalog is authoritative about membership.
		if r.resourceCatalog != nil {
			continue
		}
		snapshot := bound.snapshot()
		bound.projectAdmission(&snapshot)
		part := resourceSidebar(protocol.ResourceSource{Node: snapshot.Node, Projects: snapshot.Projects, Worktrees: snapshot.Worktrees}, snapshot, bound.controller.Connected(), r.Preferences.Data)
		data.Projects = append(data.Projects, part.Projects...)
		data.Unassigned = append(data.Unassigned, part.Unassigned...)
	}
}
