package live

import (
	"cmp"
	"context"
	"slices"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/testassembly"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func gitFixtureAssembly(assembly shell.Assembly) shell.Assembly {
	for i, page := range assembly.WorkspacePages {
		if page.Resource == panel.Changes {
			assembly.WorkspacePages[i].BindWorkspace = mountGitFixture
		}
	}
	return assembly
}

func registerGitFixture(assembly *shell.Assembly) {
	testassembly.Register(assembly)
	*assembly = gitFixtureAssembly(*assembly)
}

func registerGitPanelFixture(assembly *shell.Assembly) {
	assembly.RegisterPanelResource(panel.Resource{Kind: panel.Changes, Label: "git", Icon: icons.Network, Render: panel.RenderProjectResource, BindWorkspace: mountGitFixture})
}

func mountGitFixture(project *panel.Project, host panel.GitWorkspaceHost) panel.GitWorkspaceBinding {
	var cancel context.CancelFunc
	var scope context.Context
	var status protocol.GitStatus
	var loading, initialized, pending bool
	var notice string
	projectStatus := func() {
		project.Data.Changes = nil
		project.Data.Repo.Branch = workspaceString(status.Branch)
		for _, entry := range status.Entries {
			project.Data.Changes = append(project.Data.Changes, viewmodel.FileChange{Path: entry.Path, Added: int(entry.Diff.Additions), Removed: int(entry.Diff.Deletions)})
		}
		if len(project.Data.Changes) > 0 {
			project.Git.First(project.Data.Changes[0].Path)
		}
	}
	prepare := func() {
		if cancel == nil {
			scope, cancel = context.WithCancel(host.Context)
		}
	}
	inspect := func() {
		if loading {
			return
		}
		prepare()
		loading = true
		host.Read(scope, protocol.Command{Kind: "inspect_git", Data: protocol.CommandInspectGitPayload{Worktree: host.Worktree}}, func(c *ui.Context, out protocol.Output, err error) {
			loading = false
			status, _ = detail[protocol.GitStatus](out.Data)
			host.PublishStatus(status, err == nil)
			if err != nil {
				outcome, label, _ := workspaceReadFailure("git-worktrees", err)
				kit.RecordFailure("desktop", "inspect_git", outcome, err)
				if notice != label && host.LocationNotice() != label {
					kit.Toast(c, outcome, i18n.T(c, label))
				}
				notice = label
				host.PublishInspectionNotice(label)
				return
			}
			notice = ""
			host.PublishInspectionNotice("")
			project.SetGitState(panel.GitState{Known: true, Kind: string(status.Kind), Repositories: status.Repositories, Head: status.Head != nil})
			if !host.TurnReview() {
				projectStatus()
			}
		})
	}
	var unsubscribe func()
	return panel.GitWorkspaceBinding{
		Frame: func(invalidate bool) {
			prepare()
			if unsubscribe == nil {
				unsubscribe, _ = host.Observe(scope, func() { pending = true })
			}
			if !initialized || invalidate || pending {
				initialized, pending = true, false
				inspect()
			}
		},
		Inspect: inspect, Refresh: inspect, ResetChanges: projectStatus,
		Changes: func() *composer.Changes {
			if status.Kind == "" {
				return nil
			}
			return workspaceChanges(status)
		},
		Release: func() {
			if unsubscribe != nil {
				unsubscribe()
				unsubscribe = nil
			}
			if cancel != nil {
				cancel()
				cancel = nil
			}
			loading, initialized, pending = false, false, false
			notice = ""
		},
	}
}

func workspaceChanges(status protocol.GitStatus) *composer.Changes {
	if status.Kind == protocol.RepositoryKindDirectory {
		return nil
	}
	value := &composer.Changes{Files: len(status.Entries)}
	entries := append([]protocol.GitEntry(nil), status.Entries...)
	slices.SortStableFunc(entries, func(a, b protocol.GitEntry) int {
		var am, bm uint64
		if a.ModifiedMs != nil {
			am = *a.ModifiedMs
		}
		if b.ModifiedMs != nil {
			bm = *b.ModifiedMs
		}
		return cmp.Or(cmp.Compare(bm, am), cmp.Compare(a.Path, b.Path))
	})
	for _, entry := range entries {
		value.Added += int(entry.Diff.Additions)
		value.Removed += int(entry.Diff.Deletions)
	}
	for _, entry := range entries[:min(len(entries), composer.RecentChanges)] {
		status := workspaceChange(entry.Unstaged)
		if status == "" {
			status = workspaceChange(entry.Staged)
		}
		if entry.Untracked {
			status = "A"
		}
		value.Recent = append(value.Recent, composer.ChangedFile{Path: entry.Path, Status: status, Added: int(entry.Diff.Additions), Removed: int(entry.Diff.Deletions)})
	}
	return value
}
func workspaceChange(kind *protocol.GitChangeKind) string {
	if kind == nil {
		return ""
	}
	switch *kind {
	case protocol.GitChangeKindAdded:
		return "A"
	case protocol.GitChangeKindDeleted:
		return "D"
	case protocol.GitChangeKindRenamed:
		return "R"
	case protocol.GitChangeKindTypeChanged:
		return "T"
	}
	return "M"
}
