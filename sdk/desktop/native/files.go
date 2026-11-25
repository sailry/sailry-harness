package native

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

const UsagePageFiles = protocol.FeatureUsePageFiles

var FilesIcon = icons.Folder

type FileAction = panel.FileAction
type FileDirectoryHost = panel.FileDirectoryHost
type FileNode = viewmodel.FileNode
type FileSearchQuery = panel.SearchQuery
type FileSearchResult = panel.SearchResult

func RenderFilePanelResource(c *ui.Context, state *PanelState, tab PanelTab, tree func(*Project, *ui.Context, bool, func(string))) {
	panel.RenderFilePanelResource(c, state, tab, tree)
}
