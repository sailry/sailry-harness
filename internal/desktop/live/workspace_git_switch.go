package live

import (
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// workspaceRemoteBranch splits a remote-tracking branch into its remote and
// the branch there. A remote's name may hold a slash, so the longest listed
// remote that begins the name is the one meant, as the Node resolves it.
func workspaceRemoteBranch(remotes []string, name string) (remote, branch string, ok bool) {
	for _, candidate := range remotes {
		if strings.HasPrefix(name, candidate+"/") && len(candidate) > len(remote) {
			remote = candidate
		}
	}
	if remote == "" {
		return "", "", false
	}
	return remote, name[len(remote)+1:], true
}
func workspaceTrackingBranch(branches protocol.GitBranches, remoteBranch string) (string, bool) {
	for _, branch := range branches.Entries {
		if !branch.Remote && branches.Sync.Tracking[branch.Name] == remoteBranch {
			return branch.Name, false
		}
	}
	_, local, ok := workspaceRemoteBranch(branches.Sync.Remotes, remoteBranch)
	if !ok {
		return "", false
	}
	for _, branch := range branches.Entries {
		if !branch.Remote && branch.Name == local {
			return local, true
		}
	}
	return local, false
}
func (p *workspaceProject) switchBranch(action panel.GitAction, status protocol.GitStatus) {
	remote := false
	for _, branch := range p.branches.Entries {
		if branch.Name == action.Target {
			if branch.Current {
				return
			}
			remote = branch.Remote
			break
		}
	}
	branches := p.branches
	p.panel.Confirm("git_switch_confirm", action.Target, func(done func(bool)) {
		if !remote {
			command := protocol.Command{Kind: "switch_git_branch", Data: protocol.CommandSwitchGitBranchPayload{Worktree: p.owner.ID, Name: action.Target, Commit: action.Secondary, ExpectedHead: status.Head, ExpectedBranch: status.Branch, ExpectedIndex: workspaceIndex(status)}}
			p.runBranchSwitch(command, action, "", done)
			return
		}
		local, collision := workspaceTrackingBranch(branches, action.Target)
		if local == "" {
			done(false)
			return
		}
		run := func(name string, finish func(bool)) {
			command := p.gitCommand(status, protocol.GitAction{Kind: "switch_tracking", Data: protocol.GitActionSwitchTrackingPayload{Remote: action.Target, Local: name}})
			p.runBranchSwitch(command, action, name, finish)
		}
		if collision {
			done(true)
			p.panel.Prompt("git_tracking_name", []string{"settings_name"}, []string{local}, func(values []string, finish func(bool)) { run(values[0], finish) })
			return
		}
		run(local, done)
	}, "git_switch_confirm_detail")
	p.panel.SetPromptAction("git_switch_confirm_action")
}
func (p *workspaceProject) gitCommand(status protocol.GitStatus, action protocol.GitAction) protocol.Command {
	return protocol.Command{Kind: "run_git_action", Data: protocol.CommandRunGitActionPayload{Worktree: p.owner.ID, Action: action, ExpectedIndex: workspaceIndex(status), ExpectedHead: status.Head, ExpectedBranch: status.Branch}}
}
func (p *workspaceProject) runBranchSwitch(command protocol.Command, branch panel.GitAction, local string, finish func(bool)) {
	p.panel.SetGitSwitchPending(nil)
	p.mutateNamed(gitSwitch, command, func(c *ui.Context, out protocol.Output) {
		if finish != nil {
			finish(out.Kind != "")
		}
		if out.Kind != "" {
			p.panel.SetGitSwitchPending(nil)
			kit.Toast(c, kit.Done, i18n.T(c, "workspace_completed"))
		}
	}, func(c *ui.Context, err error) {
		if client.Retained(err) {
			p.panel.SetGitSwitchPending([]panel.PendingOperation{{Label: "files_check_result", Run: func(*ui.Context) { p.runBranchSwitch(command, branch, local, finish) }}})
		} else if faultCode(err) == protocol.ErrorCodeConflict {
			p.panel.SetGitSwitchPending([]panel.PendingOperation{{Label: "git_stash_switch", Run: func(*ui.Context) {
				delete(p.failedReads, "git-stash-switch")
				p.read("git-stash-switch", protocol.Command{Kind: "inspect_git", Data: protocol.CommandInspectGitPayload{Worktree: p.owner.ID}}, func(c *ui.Context, out protocol.Output) {
					status, ok := detail[protocol.GitStatus](out.Data)
					if !ok {
						return
					}
					action := protocol.GitAction{Kind: "switch_stashing", Data: protocol.GitActionSwitchStashingPayload{Branch: branch.Target, Commit: branch.Secondary}}
					if local != "" {
						action = protocol.GitAction{Kind: "switch_tracking", Data: protocol.GitActionSwitchTrackingPayload{Remote: branch.Target, Local: local, Stash: true}}
					}
					p.runBranchSwitch(p.gitCommand(status, action), branch, local, finish)
				})
			}}})
		}
	})
}
