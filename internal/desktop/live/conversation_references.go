package live

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func (r *Runtime) conversationReferences(session string, page protocol.ConversationPage) map[string][]conversation.Reference {
	bound := map[string][]conversation.Reference{}
	fallback := map[protocol.TurnID][]protocol.ConversationReference{}
	for i := range page.Runs {
		run := &page.Runs[i]
		if run.Input != nil && len(run.Input.Message.References) != 0 {
			fallback[run.Turn] = run.Input.Message.References
		}
	}
	appendReference := func(turn protocol.TurnID, ref protocol.ConversationReference) {
		value, ok := composerReference(ref)
		if !ok {
			return
		}
		bound[turn.String()] = append(bound[turn.String()], conversation.Reference{ID: value.ID, Label: ref.Label, Open: func() {
			r.openReference(session, turn, ref, r)
		}})
	}
	for _, entry := range page.Entries {
		if entry.Author != "user" {
			continue
		}
		delete(fallback, entry.Turn)
		for _, part := range entry.Parts {
			if part.Kind == "reference" {
				if ref, ok := detail[protocol.ConversationReference](part.Data); ok {
					appendReference(entry.Turn, ref)
				}
			}
		}
	}
	for turn, refs := range fallback {
		for _, ref := range refs {
			appendReference(turn, ref)
		}
	}
	return bound
}

func (r *Runtime) openReference(session string, turn protocol.TurnID, ref protocol.ConversationReference, source ...*Runtime) {
	r.enqueue(func(c *ui.Context) {
		owner := r
		if len(source) != 0 {
			owner = source[0]
		} else if session != "" {
			owner, _ = r.localRuntime().conversationLinkOwner(session)
			if owner == nil || r.parent != nil && owner != r {
				r.reportLink(c, unavailableLink(), "unavailable")
				return
			}
		}
		owner.dispatchReference(c, session, turn, ref)
	})
}

func (r *Runtime) dispatchReference(c *ui.Context, session string, turn protocol.TurnID, ref protocol.ConversationReference) {
	target := shell.LinkTarget{
		Route: shell.LinkRoute{Kind: shell.LinkReference, Name: ref.Target.Kind}, Node: r.controller.Target(),
		Session: session, Turn: turn, Reference: ref,
	}
	if session != "" {
		current, found := r.session(session)
		if !r.ownsLinkConversation(session) || !found {
			r.reportLink(c, unavailableLink(), "unavailable")
			return
		}
		target.Worktree = current.Worktree
	}
	if r.openFileReference(c, session, turn, ref) {
		return
	}
	handled, err := r.dispatchConversationLink(c, r, target, shell.LinkActions{})
	if !handled {
		err = unavailableLink()
	}
	r.reportLink(c, err, "unavailable")
}

func (r *Runtime) bindReferenceLinks() error {
	register := func(kind string, handler shell.LinkHandler) error {
		return r.App.RegisterDefaultLinkHandler(shell.LinkRoute{Kind: shell.LinkReference, Name: kind}, handler)
	}
	if err := register("session", func(_ *ui.Context, target shell.LinkTarget, _ shell.LinkActions) error {
		id, ok := detail[protocol.SessionID](target.Reference.Target.Data)
		if !ok {
			return unavailableLink()
		}
		cv := r.App.Data.FindOn(target.Node.String(), id.String())
		if cv == nil {
			return unavailableLink()
		}
		r.App.Sidebar.SelectOn(cv.Node, cv.ID)
		r.OpenSession(id.String())
		return nil
	}); err != nil {
		return err
	}
	if err := register("host", func(_ *ui.Context, target shell.LinkTarget, _ shell.LinkActions) error {
		if r.runtimeFor(target.Node.String()) == nil {
			return unavailableLink()
		}
		r.App.Host = target.Node.String()
		r.App.Navigate(shell.HostPage)
		return nil
	}); err != nil {
		return err
	}
	for _, spec := range []struct{ kind, key string }{{"project", "reference_project"}, {"worktree", "reference_worktree"}} {
		if err := register(spec.kind, func(c *ui.Context, target shell.LinkTarget, actions shell.LinkActions) error {
			owner := r.runtimeFor(target.Node.String())
			id, err := protocol.ParseSessionID(target.Session)
			if owner == nil || err != nil || actions.ShowReference == nil {
				return unavailableLink()
			}
			state, found := owner.controller.Conversation(id)
			if !found || state.Removed {
				return unavailableLink()
			}
			name, project, ok := referenceLocation(owner.snapshot(), state, target.Turn)
			if !ok {
				return unavailableLink()
			}
			if spec.kind == "project" {
				name = project.Path
			}
			actions.ShowReference(target.Reference.Label, i18n.T(c, spec.key), name)
			return nil
		}); err != nil {
			return err
		}
	}
	return register("attachment", func(_ *ui.Context, target shell.LinkTarget, _ shell.LinkActions) error {
		owner := r.runtimeFor(target.Node.String())
		id, ok := detail[protocol.AttachmentID](target.Reference.Target.Data)
		if owner == nil || !ok || target.Session == "" {
			return unavailableLink()
		}
		descriptor, ok := owner.attachmentDescriptor(target.Session, id.String())
		if !ok || descriptor.attachment == nil {
			return unavailableLink()
		}
		owner.openCapturedFilePreview(target.Session, conversation.File{ID: id.String(), Path: descriptor.name}, descriptor)
		return nil
	})
}

func referenceLocation(snapshot protocol.Snapshot, state client.ConversationState, turn protocol.TurnID) (string, protocol.Project, bool) {
	for _, run := range state.Snapshot.Page.Runs {
		if run.Turn != turn {
			continue
		}
		for _, worktree := range snapshot.Worktrees {
			if worktree.ID != run.Worktree || worktree.Project == nil {
				continue
			}
			for _, project := range snapshot.Projects {
				if project.ID == *worktree.Project {
					return worktree.Path, project, true
				}
			}
		}
	}
	return "", protocol.Project{}, false
}
