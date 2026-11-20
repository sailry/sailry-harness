package exporter

import (
	"strings"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// DocumentLibrary returns the offline script of a visual library a session
// document's renderer declares: the pinned assets exports render with, in
// load order. MathJax carries its configuration and its preloaded font.
func DocumentLibrary(library protocol.ArtifactLibrary) (string, bool) {
	var names []string
	switch library {
	case protocol.ArtifactLibraryMermaid:
		names = []string{"mermaid.js"}
	case protocol.ArtifactLibraryEcharts:
		names = []string{"echarts.js"}
	case protocol.ArtifactLibraryMathjax:
		names = []string{"config.js", "mathjax.js", "fonts.js"}
	default:
		return "", false
	}
	var script strings.Builder
	for _, name := range names {
		content, err := resources.ReadFile("assets/" + name)
		if err != nil {
			return "", false
		}
		script.Write(content)
		script.WriteByte('\n')
	}
	return script.String(), true
}
