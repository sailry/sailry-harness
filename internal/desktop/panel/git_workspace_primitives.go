package panel

import (
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
)

const WorkspaceBarHeight = barHeight
const WorkspaceRowHeight = rowHeight
const WorkspaceSideWidth = sideWidth

func WorkspacePair(c *ui.Context, key string, side *float32, least float32, document, details func(bool)) {
	pair(c, key, side, least, document, details)
}
func NarrowDiff(width float32) bool             { return width < diffSplitMin }
func DiffSplit(c *ui.Context, narrow bool) bool { return diffSplit(c, narrow) }
func DiffStatus(c *ui.Context, path string, narrow bool, counts func()) {
	diffStatus(c, path, narrow, counts)
}
func DiffTexts(box ui.Element, change viewmodel.FileChange) (string, string) {
	return gitDiffTexts(box, change)
}
func BinaryDiff(c *ui.Context, size uint64) string                { return binaryDiff(c, size) }
func AgeLabel(c *ui.Context, at, now time.Time) string            { return ago(c, at, now) }
func OwnedScroll(state *ui.ScrollState, own bool) *ui.ScrollState { return scroll(state, own) }
func (pr *Project) RenderCapturedDiff(c *ui.Context, id string, narrow bool) bool {
	return pr.gitDocument(c, id, narrow)
}
func (pr *Project) HasCapturedDiff(id string) bool { return pr.gitDocumentKnown(id) }
