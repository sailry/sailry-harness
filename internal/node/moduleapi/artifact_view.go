package moduleapi

import (
	"io/fs"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// BuiltInArtifactView pairs an immutable native artifact owner with its view
// declaration and embedded resources. It does not grant artifact access.
type BuiltInArtifactView struct {
	Reference protocol.PluginReference
	View      protocol.ArtifactView
	Tree      fs.FS
}

// ArtifactViewProvider is optionally implemented by native modules that ship
// document renderers. A module can keep old versions registered for replay.
type ArtifactViewProvider interface {
	ArtifactViews() []BuiltInArtifactView
}

// ArtifactViewFor only matches the recorded native reference; a package with
// the same name still uses the assets from its own captured revision.
func (a Assembly) ArtifactViewFor(reference protocol.PluginReference) (BuiltInArtifactView, bool) {
	for _, module := range a.Modules {
		if provider, ok := module.(ArtifactViewProvider); ok {
			for _, view := range provider.ArtifactViews() {
				if view.Reference == reference && view.Tree != nil {
					return view, true
				}
			}
		}
	}
	return BuiltInArtifactView{}, false
}
