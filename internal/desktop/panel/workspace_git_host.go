package panel

import (
	"context"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type GitWorkspaceHost struct {
	Context                 context.Context
	Worktree                protocol.WorktreeID
	Read                    func(context.Context, protocol.Command, func(*ui.Context, protocol.Output, error))
	Observe                 func(context.Context, func()) (func(), error)
	Post                    func(func(*ui.Context))
	Mutate                  func(protocol.Command, func(*ui.Context, protocol.Output, error))
	Report                  func(*ui.Context, error)
	Fault                   func(error) (protocol.ErrorCode, string)
	PublishStatus           func(protocol.GitStatus, bool)
	RefreshLocations        func()
	LocationNotice          func() string
	PublishInspectionNotice func(string)
	TurnReview              func() bool
	ReadFile                func(string)
	ReadCheckpointHunks     func(string)
	ApplyCheckpointHunk     func(*ui.Context, string, viewmodel.Hunk, string) bool
	OpenDiff                func(string)
	ShowOverlay             func(func(*ui.Context) bool, func())
	HasWorkspaceAction      func(string) bool
	WorkspaceAction         func(*ui.Context, string, string, string)
	OpenClone               func()
	Refresh                 func()
}

type GitWorkspaceMount func(*Project, GitWorkspaceHost) GitWorkspaceBinding

type GitWorkspaceBinding struct {
	State        any
	Frame        func(invalidated bool)
	Inspect      func()
	Refresh      func()
	Release      func()
	Prune        func()
	ResetChanges func()
	ReadDiff     func(path, scope string)
	Initialize   func(done func(bool))
	HasPending   func() bool
	Checkout     func(*ui.Context, bool, GitComposerContext)
	Changes      func() *composer.Changes
	Badge        func() *composer.StatusBadge
}

func (pr *Project) SetGitWorkspace(binding GitWorkspaceBinding) { pr.gitWorkspace = binding }
func (pr *Project) GitWorkspace() GitWorkspaceBinding           { return pr.gitWorkspace }

func (v *GitView) SetScope(scope string)  { v.scope = scope }
func (pr *Project) GitStatus() GitState   { return pr.Git.live }
func (pr *Project) HasActionPrompt() bool { return pr.prompt.open }

func (s *State) GitProject() *Project      { return s.pr }
func (s *State) OwnsResource(tab Tab) bool { return s.owns(tab) }

func (pr *Project) ResolveFilePath(path string) (string, bool) {
	if file := findFile(pr.Data.Files, path); file != nil {
		return file.Path, true
	}
	return "", false
}

func (s *State) WorkspaceMount(kind Kind) GitWorkspaceMount {
	if resource, ok := s.resource(kind); ok {
		return resource.BindWorkspace
	}
	return nil
}

type GitComposerContext struct {
	Enabled, Draft bool
	Current        func() bool
	Base           string
	SelectCheckout func(string) bool
	SelectBase     func(from, branch, commit string) bool
	ForkBase       func(*ui.Context, string, string)
}
