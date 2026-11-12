package panel

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

type GitState struct {
	Known        bool
	Kind         string
	Repositories []string
	Conflicts    map[string]bool
	Head         bool
	HeadMessage  string
}

func (pr *Project) SetGitHeadMessage(message string) { pr.Git.live.HeadMessage = message }
func (pr *Project) gitHasHead() bool {
	if pr.Actions != nil {
		return pr.Git.live.Head
	}
	return len(pr.Data.Repo.Commits) > 0
}

func (pr *Project) SetGitSwitchPending(operations []PendingOperation) {
	pr.Git.switchPending = operations
}

type GitHistoryState struct {
	Loading, Next, Previous, Partial bool
	Offset                           uint64
}
type GitDiffState struct{ Binary, Partial bool }

func (pr *Project) SetGitDiffState(path, scope string, state GitDiffState) {
	if pr.Git.diffStates == nil {
		pr.Git.diffStates = map[string]GitDiffState{}
	}
	pr.Git.diffStates[scope+"/"+path] = state
}

func (pr *Project) SetGitState(state GitState)          { pr.Git.live = state }
func (pr *Project) SetGitHistory(state GitHistoryState) { pr.Git.log = state }
func (pr *Project) gitConflicted(path string) bool {
	return pr.Actions != nil && pr.Git.live.Conflicts[path]
}
func (pr *Project) gitWritable() bool {
	return pr.Actions == nil || pr.Git.live.Known && pr.Git.live.Kind != "directory"
}
func (pr *Project) gitCanCommit() bool {
	return pr.gitWritable() && len(pr.Git.live.Conflicts) == 0 && !pr.gitBusy()
}

// gitBusy reports a Git action under way: the header shows the spinner and
// Commit waits.
func (pr *Project) gitBusy() bool {
	return pr.Actions != nil && pr.Actions.GitBusy != nil && pr.Actions.GitBusy()
}

// gitVacant reports a live folder whose details show no repository: one
// not read yet, or a plain directory (gitEmpty with details).
func (pr *Project) gitVacant() bool {
	return pr.Actions != nil && (!pr.Git.live.Known || pr.Git.live.Kind == "directory")
}

func (pr *Project) gitEmpty(c *ui.Context, details bool) bool {
	state := pr.Git.live
	if pr.Actions == nil {
		return false
	}
	if !state.Known {
		ui.Column(c).Grow(1)
		return true
	}
	if state.Kind != "directory" && (state.Kind != "unborn" || details) {
		return false
	}
	key := "git_unborn_title"
	if state.Kind == "directory" {
		key = "git_directory_title"
	}
	if len(state.Repositories) > 0 {
		key = "git_repositories"
	}
	ui.Column(c.Key("git-empty")).Grow(1).Center().Padding(16).Gap(12).Children(func() {
		kit.Result(c, kit.ResultProps{Icon: icons.Network, Title: i18n.T(c, key, "count", len(state.Repositories))})
		if len(state.Repositories) > 0 {
			ui.Column(c.Key("repositories")).FillWidth().MaxWidth(384).Border(1, theme.Of(c).Border).Radius(theme.Of(c).Radius).Children(func() {
				for index, name := range state.Repositories {
					ui.Row(c.Key(index)).Padding(8, 12).Gap(8).Children(func() { ui.Icon(c, icons.Folder).FontSize(14); ui.Text(c, name).FontSize(14).SingleLine() })
				}
			})
		}
		if state.Kind == "directory" {
			label := i18n.T(c, "git_initialize")
			kit.Primary(c.Key("initialize"), label, false).Children(func() { ui.Text(c, label).FontSize(14) }).OnClick(func() { pr.gitAction(c, GitAction{Kind: "git_initialize"}) })
		}
	})
	return true
}
