package live

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func (r *Runtime) bindReferences(session string, page protocol.ConversationPage) {
	bound := map[string][]conversation.Reference{}
	owners := map[protocol.TurnID]protocol.TurnID{}
	canonical := map[protocol.TurnID]bool{}
	for _, run := range page.Runs {
		owners[run.Turn] = run.Turn
		if run.Joined != nil {
			owners[run.Turn] = *run.Joined
		}
	}
	appendReference := func(turn protocol.TurnID, ref protocol.ConversationReference) {
		value, ok := composerReference(ref)
		if !ok {
			return
		}
		owner := owners[turn].String()
		bound[owner] = append(bound[owner], conversation.Reference{ID: value.ID, Label: ref.Label, Open: func() { r.openReference(session, turn, ref) }})
	}
	for _, entry := range page.Entries {
		if entry.Author != "user" {
			continue
		}
		canonical[entry.Turn] = true
		for _, part := range entry.Parts {
			if part.Kind == "reference" {
				if ref, ok := detail[protocol.ConversationReference](part.Data); ok {
					appendReference(entry.Turn, ref)
				}
			}
		}
	}
	for _, run := range page.Runs {
		if !canonical[run.Turn] && run.Input != nil {
			for _, ref := range run.Input.Message.References {
				appendReference(run.Turn, ref)
			}
		}
	}
	r.App.Messages.SetReferences(session, bound)
}

func (r *Runtime) openReference(session string, turn protocol.TurnID, ref protocol.ConversationReference) {
	r.enqueue(func(c *ui.Context) {
		snapshot := r.controller.Snapshot()
		switch ref.Target.Kind {
		case "plugin":
			if name, ok := detail[string](ref.Target.Data); ok {
				r.App.Host = r.controller.Target().String()
				r.App.Settings.Open(settings.Plugins)
				r.App.Navigate(shell.SettingsPage)
				r.App.Settings.ShowDetails(settings.DetailPlugin, name)
				return
			}
		case "session":
			if id, ok := detail[protocol.SessionID](ref.Target.Data); ok && r.App.Data.Find(id.String()) != nil {
				r.OpenSession(id.String())
				return
			}
		case "host":
			r.App.Host = snapshot.Node.String()
			r.App.Navigate(shell.HostPage)
			return
		case "ssh", "database":
			scope := connectionScope{}
			if current, ok := r.session(session); ok {
				scope.project, scope.resource = current.Project, current.Config.Resource
			}
			if key, body, ok := connectionReferenceDetails(snapshot, scope, ref.Target); ok {
				r.App.Messages.ShowReference(session, ref.Label, i18n.T(c, key), "```text\n"+body+"\n```")
				return
			}
		case "project", "worktree":
			if id, err := protocol.ParseSessionID(session); err == nil {
				if state, ok := r.controller.Conversation(id); ok {
					if path, project, ok := referenceLocation(snapshot, state, turn); ok {
						key := "reference_worktree"
						if ref.Target.Kind == "project" {
							path, key = project.Path, "reference_project"
						}
						r.App.Messages.ShowReference(session, ref.Label, i18n.T(c, key), "```text\n"+path+"\n```")
						return
					}
				}
			}
		}
		kit.Toast(c, kit.Failed, i18n.T(c, "unavailable"))
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
