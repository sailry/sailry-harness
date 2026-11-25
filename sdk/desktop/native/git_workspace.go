package native

import (
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
)

type (
	GitWorkspaceHost     = panel.GitWorkspaceHost
	GitWorkspaceBinding  = panel.GitWorkspaceBinding
	GitComposerContext   = panel.GitComposerContext
	StatusBadge          = composer.StatusBadge
	StatusText           = composer.Text
	StatusSegment        = composer.Segment
	StatusDetail         = composer.Detail
	GitAction            = panel.GitAction
	GitChoice            = panel.Choice
	GitState             = panel.GitState
	GitDocumentFile      = panel.GitDocumentFile
	WorkspaceChange      = viewmodel.FileChange
	WorkspaceDiffState   = viewmodel.DiffState
	WorkspaceHunk        = viewmodel.Hunk
	WorkspaceDiffLine    = viewmodel.DiffLine
	WorkspaceCommit      = viewmodel.Commit
	WorkspaceBranch      = viewmodel.Branch
	WorkspaceRemote      = viewmodel.Remote
	WorkspaceRepository  = viewmodel.Repository
	WorkspaceData        = viewmodel.Workspace
	WorkspaceOperation   = panel.PendingOperation
	WorkspaceChanges     = composer.Changes
	WorkspaceChangedFile = composer.ChangedFile
)

const (
	DiffContext        = viewmodel.Context
	DiffAdded          = viewmodel.Added
	DiffRemoved        = viewmodel.Removed
	WorkspaceBarHeight = panel.WorkspaceBarHeight
	WorkspaceRowHeight = panel.WorkspaceRowHeight
	WorkspaceSideWidth = panel.WorkspaceSideWidth
	RecentChanges      = composer.RecentChanges
)

var PreviewTime = preview.Now

func MutationRetained(err error) bool { return client.Retained(err) }

func ParseDiff(patch string) []WorkspaceHunk        { return viewmodel.ParseDiff(patch) }
func DiffHunkFingerprint(hunk WorkspaceHunk) string { return viewmodel.DiffHunkFingerprint(hunk) }
func WorkspacePair(c *ui.Context, key string, side *float32, least float32, document, details func(bool)) {
	panel.WorkspacePair(c, key, side, least, document, details)
}
func NarrowDiff(width float32) bool             { return panel.NarrowDiff(width) }
func DiffSplit(c *ui.Context, narrow bool) bool { return panel.DiffSplit(c, narrow) }
func DiffStatus(c *ui.Context, path string, narrow bool, counts func()) {
	panel.DiffStatus(c, path, narrow, counts)
}
func DiffTexts(box ui.Element, change WorkspaceChange) (string, string) {
	return panel.DiffTexts(box, change)
}
func BinaryDiff(c *ui.Context, size uint64) string     { return panel.BinaryDiff(c, size) }
func AgeLabel(c *ui.Context, at, now time.Time) string { return panel.AgeLabel(c, at, now) }
func OwnedScroll(state *ui.ScrollState, own bool) *ui.ScrollState {
	return panel.OwnedScroll(state, own)
}
func ComposerContextButton(c *ui.Context, b ui.Element, icon *ui.SVG, label string, inMenu bool) ui.Element {
	return composer.WorkspaceContextButton(c, b, icon, label, inMenu)
}
