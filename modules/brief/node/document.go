package briefnode

import (
	"github.com/sailry/sailry-harness/internal/node/moduleapi"
	"github.com/sailry/sailry-harness/modules/brief/document"
)

func (module) ArtifactViews() []moduleapi.BuiltInArtifactView {
	reference, view, tree := document.ResourcesV1()
	return []moduleapi.BuiltInArtifactView{{Reference: reference, View: view, Tree: tree}}
}
