package live

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type backupUnarchives struct {
	operation string
	pending   map[protocol.SessionID]protocol.Request
}

func (r *Runtime) backupArchiveWork() string {
	if unarchives := r.localRuntime().backupSettings.unarchives[r.controller.Target()]; unarchives != nil && unarchives.operation != "" {
		return unarchives.operation
	}
	return r.backupSettings.archive
}

func (r *Runtime) backupArchiveSelection(ids []string) []protocol.SessionRevision {
	if len(ids) == 0 {
		return nil
	}
	selected := make(map[string]bool, len(ids))
	for _, id := range ids {
		selected[id] = true
	}
	expected := make([]protocol.SessionRevision, 0, len(selected))
	for _, session := range r.visibleSessions() {
		id := session.ID.String()
		if session.Archived && session.Delegation == nil && selected[id] {
			delete(selected, id)
			expected = append(expected, protocol.SessionRevision{Session: session.ID, Revision: session.Revision})
		}
	}
	if len(selected) != 0 {
		r.enqueue(func(c *ui.Context) { kit.Toast(c, kit.Conflict, i18n.T(c, "session_changed")) })
		return nil
	}
	return expected
}

func (r *Runtime) submitUnarchives(expected []protocol.SessionRevision, operation string) {
	root := r.localRuntime()
	controller := r.controller
	host := controller.Target()
	if root.backupSettings.unarchives == nil {
		root.backupSettings.unarchives = make(map[protocol.NodeID]*backupUnarchives)
	}
	unarchives := root.backupSettings.unarchives[host]
	if unarchives == nil {
		unarchives = &backupUnarchives{pending: make(map[protocol.SessionID]protocol.Request)}
		root.backupSettings.unarchives[host] = unarchives
	}
	unarchives.operation, r.backupSettings.archive = operation, operation
	// Keep the window busy for the entire sequence, even if its binding changes.
	if !root.async(func() {
		results := make([]error, len(expected))
		for i, entry := range expected {
			request, retry := unarchives.pending[entry.Session]
			if retry {
				resolution, err := controller.Resolve(root.ctx, request)
				if err != nil {
					results[i] = fileActionFault(protocol.ErrorCodeOutcomeUnknown, "could not inspect the original unarchive request: "+err.Error())
				} else {
					_, results[i] = resolution.Result()
				}
			} else {
				request = controller.Prepare(protocol.Command{Kind: "set_session_archived", Data: protocol.CommandSetSessionArchivedPayload{
					Session: entry.Session, ExpectedRevision: entry.Revision, Archived: false,
				}})
				_, results[i] = controller.Settle(root.ctx, request)
			}
			if client.Retained(results[i]) {
				unarchives.pending[entry.Session] = request
			} else {
				delete(unarchives.pending, entry.Session)
			}
		}
		root.enqueue(func(c *ui.Context) {
			unarchives.operation, r.backupSettings.archive = "", ""
			if len(unarchives.pending) == 0 {
				delete(root.backupSettings.unarchives, host)
			}
			shown := map[string]bool{}
			for _, err := range results {
				if err != nil {
					outcome, key := backupSessionFailure(err)
					if !shown[key] {
						kit.Toast(c, outcome, i18n.T(c, key))
						shown[key] = true
					}
				}
			}
			if len(shown) == 0 {
				reportArchiveResult(c, "backup_unarchived", nil)
			}
		})
	}) {
		unarchives.operation, r.backupSettings.archive = "", ""
	}
}

func (r *Runtime) submitBackupDeletion(command protocol.Command, operation string) {
	if r.backupArchiveWork() != "" {
		return
	}
	if r.submitRemoval(command, func(c *ui.Context, err error) {
		if r.backupSettings.archive == operation {
			r.backupSettings.archive = ""
		}
		reportArchiveResult(c, "backup_deleted", err)
	}) {
		r.backupSettings.archive = operation
	}
}
