package panel

import (
	"context"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// FileDirectoryHost supplies scoped reads without owning any browser state.
type FileDirectoryHost struct {
	Name       string
	Context    context.Context
	Read       func(context.Context, string, *protocol.DirectoryCursor, func(*ui.Context, protocol.Directory, error))
	Observe    func(context.Context) func()
	Revision   func() uint64
	Fault      func(error) protocol.ErrorCode
	CancelOpen func()
}

// FileBrowserState is owned by the Files contribution and shared by its two surfaces.
func (pr *Project) FileBrowserState(create func() any) any {
	if pr.fileBrowser == nil {
		pr.fileBrowser = create()
	}
	return pr.fileBrowser
}
