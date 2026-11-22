// Package desktop selects the native pages shipped with Harness.
package desktop

import (
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	files "github.com/sailry/sailry-harness/modules/files/desktop"
	git "github.com/sailry/sailry-harness/modules/git/desktop"
	market "github.com/sailry/sailry-harness/modules/market/desktop"
)

// Harness returns the page contributions for a new official Desktop window.
func Harness() []shell.NativePage {
	return []shell.NativePage{files.Page(), git.Page(), market.Page()}
}
