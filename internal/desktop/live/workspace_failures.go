package live

import (
	"encoding/json"
	"log/slog"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// File mutations use the editor's own failure handling.
const (
	fileSave       = "file_save"
	// workspaceAction is any other workspace mutation: the general copy.
	workspaceAction = "workspace"
)

// workspaceFailure names the outcome of a shared workspace mutation.
func workspaceFailure(kind string, err error) (kit.Outcome, string) {
	code, message := faultCode(err), faultMessage(err)
	if code == protocol.ErrorCodeUnavailable && message == "resource host is offline" {
		return kit.Failed, "resource_offline_action"
	}
	outcome := kit.Failed
	switch code {
	case protocol.ErrorCodeConflict, protocol.ErrorCodeRevisionConflict:
		outcome = kit.Conflict
	case protocol.ErrorCodeOutcomeUnknown:
		outcome = kit.Uncertain
	}
	switch kind {
	case fileSave:
		switch {
		case fileSaveConflict(err):
			// The editor asks what to do with the draft instead; Save all
			// in the close prompt tells its own toast (closing.go).
			return outcome, ""
		case code == protocol.ErrorCodeConflict:
			return outcome, "workspace_changed"
		case code == protocol.ErrorCodeOutcomeUnknown:
			return outcome, "workspace_action_unknown"
		}
		return outcome, "workspace_action_failed"
	case workspaceAction:
		switch outcome {
		case kit.Conflict:
			return outcome, "workspace_changed"
		case kit.Uncertain:
			return outcome, "workspace_action_unknown"
		}
		return outcome, "workspace_action_failed"
	}
	return outcome, "workspace_action_failed"
}

// workspaceReadGroup is what a workspace read is for: "directory" for a
// folder listing, "git" for a repository read, "" for any other read.
func workspaceReadGroup(key string) string {
	switch {
	case strings.HasPrefix(key, "directory/"):
		return "directory"
	case key == "git-worktrees":
		return "locations"
	}
	return ""
}

// workspaceReadFailure names the toast of a failed folder listing or Git
// read. The workspace starts these on its own once a conversation or draft
// shows a worktree, as when a project is chosen for a new draft, so the copy
// says what was read and the cause the Node tells apart instead of naming a
// workspace action. own is false for any other read, which keeps the general
// copy. A failed local Git read names the inspection that failed.
func workspaceReadFailure(key string, err error) (outcome kit.Outcome, name string, own bool) {
	group := workspaceReadGroup(key)
	if group == "" {
		return kit.Failed, "", false
	}
	code := faultCode(err)
	switch {
	case code == protocol.ErrorCodeUnavailable && faultMessage(err) == "resource host is offline":
		return kit.Failed, "resource_offline_action", true
	case code == protocol.ErrorCodeOutcomeUnknown:
		return kit.Uncertain, "workspace_action_unknown", true
	case code == protocol.ErrorCodeConflict || code == protocol.ErrorCodeRevisionConflict:
		return kit.Conflict, "workspace_changed", true
	}
	if group == "directory" {
		switch code {
		case protocol.ErrorCodeNotFound:
			return kit.Failed, "directory_missing", true
		case protocol.ErrorCodePermissionDenied:
			return kit.Failed, "directory_denied", true
		}
		return kit.Failed, "directory_failed", true
	}
	switch code {
	case protocol.ErrorCodeNotFound:
		return kit.Failed, "git_index_missing", true
	case protocol.ErrorCodePermissionDenied:
		return kit.Failed, "git_index_denied", true
	case protocol.ErrorCodeBusy:
		return kit.Failed, "git_index_busy", true
	}
	return kit.Failed, "git_inspection_failed", true
}

// mutateNamed sends one workspace mutation: admission by its worktree,
// uncertain-outcome retention and refresh, with the failure toast named by
// its kind.
func (p *workspaceProject) mutateNamed(kind string, command protocol.Command, done func(*ui.Context, protocol.Output), failed ...func(*ui.Context, error)) {
	encoded, _ := json.Marshal(command)
	key := string(encoded)
	scope := "workspace-action/" + p.owner.ID.String()
	busyKey := scope + "\x00" + key
	trash, trashEntry := detail[protocol.CommandTrashEntryPayload](command.Data)
	trashEntry = trashEntry && command.Kind == "trash_entry"
	if trashEntry && !p.runtime.busy[busyKey] {
		p.beginTrash(trash.Path)
	}
	complete := func(c *ui.Context, out protocol.Output, err error, unknown bool) {
		if trashEntry {
			delete(p.trashing, trash.Path)
			p.pendingFiles = true
			if err == nil && out.Kind == "entry_trashed" {
				p.entryTrashed(trash.Path)
			}
		}
		p.panel.SetPromptUncertain(unknown)
		if unknown {
			if p.uncertainOperations == nil {
				p.uncertainOperations = map[string]protocol.Command{}
			}
			p.uncertainOperations[key] = command
			if p.uncertainCallbacks == nil {
				p.uncertainCallbacks = map[string]func(*ui.Context, protocol.Output){}
			}
			if _, exists := p.uncertainCallbacks[key]; !exists {
				p.uncertainCallbacks[key] = done
			}
		} else {
			delete(p.uncertainOperations, key)
			delete(p.uncertainCallbacks, key)
		}
		if err != nil {
			reportWorkspaceMutation(c, command.Kind, kind, err)
			for _, failure := range failed {
				failure(c, err)
			}
			if done != nil {
				done(c, protocol.Output{})
			}
			return
		}
		p.pendingFiles, p.pendingGit = true, true
		if done != nil {
			done(c, out)
		} else {
			kit.Toast(c, kit.Done, i18n.T(c, "workspace_completed"))
		}
	}
	// The trash prompt offers to check the original result, not dispatch the
	// mutation again. Other scoped operations keep their exact-replay contract.
	if request, retained := p.runtime.uncertain[busyKey]; trashEntry && retained {
		if p.runtime.busy[busyKey] {
			return
		}
		p.runtime.busy[busyKey] = true
		p.runtime.async(func() {
			resolution, err := p.runtime.controller.Resolve(p.runtime.ctx, request)
			unknown := err != nil || resolution.State == client.RequestPending || resolution.State == client.RequestReplayable
			var out protocol.Output
			if err == nil {
				out, err = resolution.Result()
			}
			p.runtime.enqueue(func(c *ui.Context) {
				delete(p.runtime.busy, busyKey)
				if !unknown {
					delete(p.runtime.uncertain, busyKey)
				}
				complete(c, out, err, unknown)
			})
		})
		return
	}
	p.runtime.executeScoped(scope, command, func(c *ui.Context, out protocol.Output, err error) {
		complete(c, out, err, client.Retained(err))
	})
}

func reportWorkspaceMutation(c *ui.Context, operation, kind string, err error) {
	outcome, name := workspaceFailure(kind, err)
	if name == "" {
		return
	}
	kit.Toast(c, outcome, i18n.T(c, name))
	kit.RecordFailure("desktop", operation, outcome, err, slog.String("stage", "workspace_mutation"), slog.String("reason", name))
}
