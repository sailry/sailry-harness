package panel

import (
	"fmt"
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
)

type GitState struct {
	Known        bool
	Kind         string
	Repositories []string
	Conflicts    map[string]bool
	Head         bool
	HeadMessage  string
}

func (pr *Project) SetGitState(state GitState)       { pr.Git.live = state }
func (pr *Project) SetGitHeadMessage(message string) { pr.Git.live.HeadMessage = message }

// binaryDiff says that a file is not compared as text, with its size when
// that is known.
func binaryDiff(c *ui.Context, size uint64) string {
	return i18n.T(c, "git_diff_binary") + binarySize(size)
}

func binarySize(size uint64) string {
	switch {
	case size == 0:
		return ""
	case size >= 1_000_000_000:
		return fmt.Sprintf(" · %.1f GB", float64(size)/1e9)
	case size >= 1_000_000:
		return fmt.Sprintf(" · %.1f MB", float64(size)/1e6)
	case size >= 1_000:
		return fmt.Sprintf(" · %.1f KB", float64(size)/1e3)
	}
	return fmt.Sprintf(" · %d B", size)
}
