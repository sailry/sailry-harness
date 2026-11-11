package live

import (
	"fmt"
	"slices"
	"strings"
	"time"

	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
)

// A draft is a conversation of the desktop alone until its first send creates
// the Node session. It stays in the shell's data, which shows it, and out of
// the navigation column, which lists sessions.

func unsentDraft(cv *viewmodel.Conversation) bool { return strings.HasPrefix(cv.ID, "draft/") }

// newDraft opens the unsent conversation of a location: the one it already
// has, with whatever was typed in it, otherwise a new one.
func (r *Runtime) newDraft(p *viewmodel.Project) {
	cv := r.draftIn(p)
	if p != nil {
		r.App.Sidebar.Reveal(p.ID)
	}
	r.App.Page = shell.ConversationPage
	r.App.Sidebar.Selected = cv.ID
	r.listSidebar()
}

func (r *Runtime) draftIn(p *viewmodel.Project) *viewmodel.Conversation {
	list := &r.App.Data.Unassigned
	if p != nil {
		list = nil
		for _, current := range r.App.Data.Projects {
			if current.ID == p.ID && current.Host == p.Host {
				list = &current.Conversations
				break
			}
		}
	}
	if list != nil {
		for _, current := range *list {
			// A submitted draft is becoming its session.
			if unsentDraft(current) && !r.draftSubmitted(current) {
				return current
			}
		}
	}
	r.drafts++
	cv := &viewmodel.Conversation{Node: r.controller.Target().String(), ID: fmt.Sprintf("draft/%d", r.drafts), Updated: time.Now(), Created: int(^uint(0) >> 1)}
	if p != nil {
		cv.Node = p.Host
	}
	if r.openDrafts == nil {
		r.openDrafts = map[string]bool{}
	}
	r.openDrafts[cv.ID] = true
	if list != nil {
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
	r.App.ApplyData(r.App.Data)
	r.listSidebar()
}

// listSidebar gives the navigation column the data without drafts. The shell
// keeps the whole data, so the selected draft still shows.
func (r *Runtime) listSidebar() {
	data := r.App.Data
	listed := *data
	listed.Unassigned = slices.DeleteFunc(slices.Clone(data.Unassigned), unsentDraft)
	hidden := len(listed.Unassigned) != len(data.Unassigned)
	listed.Projects = slices.Clone(data.Projects)
	for index, project := range listed.Projects {
		if slices.ContainsFunc(project.Conversations, unsentDraft) {
			copied := *project
			copied.Conversations = slices.DeleteFunc(slices.Clone(project.Conversations), unsentDraft)
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

// pruneDrafts drops, after a frame, the drafts New session opened that the
// user left untouched: not selected, in no pane, nothing typed or attached.
func (r *Runtime) pruneDrafts() {
	for id := range r.openDrafts {
		cv := r.App.Data.Find(id)
		if cv == nil {
			// The draft became its session, or was removed.
			delete(r.openDrafts, id)
			continue
		}
		if id == r.App.Sidebar.Selected || r.draftsShown[id] || r.draftSubmitted(cv) {
			continue
		}
		comp := r.App.Composer(id)
		if strings.TrimSpace(comp.Draft) != "" || len(comp.Attachments) != 0 || len(comp.References) != 0 {
			continue
		}
		r.removeDraft(id)
	}
}
