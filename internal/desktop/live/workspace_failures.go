package live

import (
	"encoding/json"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/node/git"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// The kinds of Git, worktree and file mutation whose failures have their own
// copy.
const (
	gitIndex       = "index"
	gitCommit      = "commit"
	gitSwitch      = "switch"
	gitMerge       = "merge"
	gitDelete      = "delete"
	gitCreate      = "branch_create"
	gitRename      = "branch_rename"
	gitOperation   = "action"
	worktreeCreate = "worktree_create"
	worktreeOpen   = "worktree_open"
	worktreeRemove = "worktree_remove"
	worktreePrune  = "worktree_prune"
	fileSave       = "file_save"
)

// workspaceFailure names the toast of a failed Git or worktree mutation. The
// Node tells causes apart by fault code; the few that share a code are told
// apart by the messages its Git service exports. A cause it does not
// distinguish keeps the operation's general copy.
func workspaceFailure(kind string, err error) (kit.Outcome, string) {
	code, message := faultCode(err), faultMessage(err)
	outcome := kit.Failed
	switch code {
	case protocol.ErrorCodeConflict, protocol.ErrorCodeRevisionConflict:
		outcome = kit.Conflict
	case protocol.ErrorCodeOutcomeUnknown:
		outcome = kit.Uncertain
	}
	conflict := code == protocol.ErrorCodeConflict
	switch kind {
	case worktreeCreate, worktreeOpen:
		switch {
		case conflict && message == git.BranchExists:
			return outcome, "git_branch_exists"
		case conflict && message == git.DestinationExists:
			return outcome, "worktree_path_exists"
		}
		return outcome, worktreeFailureKey(code, "worktree_conflict", "worktree_not_found", "worktree_request_failed")
	case worktreeRemove:
		switch {
		case code == protocol.ErrorCodeBusy && message == git.WorktreeLocked:
			return outcome, "worktree_remove_locked"
		case code == protocol.ErrorCodeBusy:
			return outcome, "worktree_remove_busy"
		case conflict && message == git.WorktreeDirty:
			return outcome, "worktree_remove_dirty"
		}
		return outcome, worktreeFailureKey(code, "worktree_remove_conflict", "worktree_remove_missing", "worktree_request_failed")
	case worktreePrune:
		switch {
		case code == protocol.ErrorCodeBusy && message == git.WorktreeLocked:
			return outcome, "worktree_remove_locked"
		case code == protocol.ErrorCodeBusy:
			return outcome, "git_index_busy"
		case conflict && message == git.WorktreePathExists:
			return outcome, "worktree_prune_exists"
		}
		return outcome, worktreeFailureKey(code, "git_index_conflict", "worktree_remove_missing", "worktree_prune_failed")
	case gitIndex:
		switch code {
		case protocol.ErrorCodeRevisionConflict:
			return outcome, "git_index_changed"
		case protocol.ErrorCodeBusy:
			return outcome, "git_index_busy"
		case protocol.ErrorCodePermissionDenied:
			return outcome, "git_index_denied"
		case protocol.ErrorCodeNotFound:
			return outcome, "git_index_missing"
		case protocol.ErrorCodeOutcomeUnknown:
			return outcome, "git_index_unknown"
		}
		return outcome, "git_index_failed"
	case fileSave:
		switch {
		case fileSaveConflict(err):
			// The editor asks what to do with the draft instead.
			return outcome, ""
		case code == protocol.ErrorCodeConflict:
			return outcome, "workspace_changed"
		case code == protocol.ErrorCodeOutcomeUnknown:
			return outcome, "workspace_action_unknown"
		}
		return outcome, "workspace_action_failed"
	}
	branch := kind == gitSwitch || kind == gitDelete || kind == gitMerge
	switch {
	case code == protocol.ErrorCodeRevisionConflict && branch:
		return outcome, "git_branch_revision"
	case code == protocol.ErrorCodeRevisionConflict:
		return outcome, "git_index_conflict"
	case code == protocol.ErrorCodeNotConfigured && (kind == gitCommit || kind == gitMerge):
		return outcome, "git_identity_missing"
	case code == protocol.ErrorCodeUnavailable && (kind == gitCommit || kind == gitMerge):
		return outcome, "git_commit_unavailable"
	case code == protocol.ErrorCodeBusy && (kind == gitSwitch || kind == gitMerge):
		return outcome, "git_branch_busy"
	case conflict && message == git.BranchInWorktree:
		return outcome, "git_branch_checked_out"
	case conflict && message == git.BranchExists:
		return outcome, "git_branch_exists"
	case conflict:
		switch kind {
		case gitSwitch:
			return outcome, "git_branch_conflict"
		case gitMerge:
			return outcome, "git_merge_conflict"
		case gitDelete:
			return outcome, "git_branch_delete_conflict"
		case gitRename:
			return outcome, "git_branch_rename_conflict"
		case gitCommit:
			return outcome, "git_commit_failed"
		}
		return outcome, "git_action_conflict"
	case code == protocol.ErrorCodePermissionDenied && kind == gitOperation:
		return outcome, "git_auth_failed"
	}
	prefix := "git_action"
	switch kind {
	case gitSwitch:
		prefix = "git_branch"
	case gitDelete:
		prefix = "git_branch_delete"
	case gitMerge:
		prefix = "git_merge"
	case gitCommit:
		prefix = "git_commit"
	case gitCreate:
		prefix = "git_branch_create"
	case gitRename:
		prefix = "git_branch_rename"
	}
	if code == protocol.ErrorCodeOutcomeUnknown {
		return outcome, prefix + "_unknown"
	}
	return outcome, prefix + "_failed"
}

// worktreeFailureKey is a worktree operation's copy for a fault code.
func worktreeFailureKey(code protocol.ErrorCode, conflict, missing, failed string) string {
	switch code {
	case protocol.ErrorCodeOutcomeUnknown:
		return "worktree_outcome_unknown"
	case protocol.ErrorCodeConflict, protocol.ErrorCodeRevisionConflict:
		return conflict
	case protocol.ErrorCodeInvalidRequest:
		return "worktree_invalid"
	case protocol.ErrorCodeNotFound:
		return missing
	}
	return failed
}

// mutateNamed is mutate for a Git or worktree operation: the same admission,
// uncertain-outcome retention and refresh, with the failure toast named by
// its kind instead of the general workspace copy.
func (p *workspaceProject) mutateNamed(kind string, command protocol.Command, done func(*ui.Context, protocol.Output), failed ...func(*ui.Context, error)) {
	encoded, _ := json.Marshal(command)
	key := string(encoded)
	p.runtime.executeScoped("workspace-action/"+p.owner.ID.String(), command, func(c *ui.Context, out protocol.Output, err error) {
		unknown := faultCode(err) == protocol.ErrorCodeOutcomeUnknown
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
			if outcome, name := workspaceFailure(kind, err); name != "" {
				kit.Toast(c, outcome, i18n.T(c, name))
			}
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
	})
}
