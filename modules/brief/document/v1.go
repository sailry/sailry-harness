package document

import (
	"embed"
	"io/fs"
	"slices"

	"github.com/sailry/sailry-harness/internal/protocol"
)

const versionV1 = "v1"

//go:embed v1/template.html v1/brief.css v1/charts.js v1/forms.js v1/brief.js
var assetsV1 embed.FS

// This declaration and the files under v1/ are immutable once released.
// Changes belong to a new version registered alongside v1, not a new v1 digest.
var viewV1 = protocol.ArtifactView{
	Template: "v1/template.html",
	Styles:   []string{"v1/brief.css"},
	Scripts:  []string{"v1/charts.js", "v1/forms.js", "v1/brief.js"},
	Libraries: []protocol.ArtifactLibrary{
		protocol.ArtifactLibraryMermaid, protocol.ArtifactLibraryEcharts, protocol.ArtifactLibraryMathjax,
	},
	AllowedClasses: []string{
		"lead", "callout", "info", "warn", "risk", "ok", "mermaid", "math", "math-inline", "chart",
		"steps", "files", "badge", "new", "changed", "removed", "compare", "option", "brief-form",
	},
}

var referenceV1 = func() protocol.PluginReference {
	digest, err := digestV1(versionV1, viewV1, assetsV1)
	if err != nil {
		panic(err) // A native renderer must have all of its embedded resources.
	}
	return protocol.PluginReference{Name: "brief", Digest: digest}
}()

// ResourcesV1 pairs the frozen v1 reference with its declaration and files.
// Return independent slices so a caller cannot change the registered identity.
func ResourcesV1() (protocol.PluginReference, protocol.ArtifactView, fs.FS) {
	view := viewV1
	view.Styles = slices.Clone(view.Styles)
	view.Scripts = slices.Clone(view.Scripts)
	view.Libraries = slices.Clone(view.Libraries)
	view.AllowedClasses = slices.Clone(view.AllowedClasses)
	return referenceV1, view, assetsV1
}
