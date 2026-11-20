package plugins

import (
	"slices"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestArtifactViewReadsDeclaredRendererInOrder(t *testing.T) {
	t.Parallel()
	const root = "dev.sailry.platform/document/"
	manifest := `{"$schema":"` + ManifestSchema + `","name":"artifact-view-fixture","version":"1.0.0","extensions":{"dev.sailry.platform":{"api_version":"v1","actions":["artifacts.read","artifacts.write"],` +
		`"artifact_view":{"template":"` + root + `template.html","styles":["` + root + `a.css"],"scripts":["` + root + `b.js","` + root + `a.js"],"libraries":["mermaid"],"allowed_classes":["brief-form"]}}}}`
	entries := []packageEntry{
		{name: "dev.sailry.platform", directory: true},
		{name: "dev.sailry.platform/document", directory: true},
		{name: root + "a.css", data: []byte("main{}")},
		{name: root + "a.js", data: []byte("window.a=1")},
		{name: root + "b.js", data: []byte("window.b=1")},
		{name: root + "template.html", data: []byte("<html><head></head><body><main></main></body></html>")},
		{name: "plugin.json", data: []byte(manifest)},
	}
	manager := NewManager(t.TempDir())
	info, err := manager.publish(t.Context(), entries, nil, "artifact-view-fixture", protocol.PluginOrigin{Kind: "directory"})
	if err != nil {
		t.Fatal(err)
	}
	view, err := manager.ReadArtifactView(t.Context(), PackageReference(info))
	if err != nil || view == nil {
		t.Fatalf("artifact view: %+v %v", view, err)
	}
	if view.Template != "<html><head></head><body><main></main></body></html>" ||
		!slices.Equal(view.Styles, []string{"main{}"}) ||
		!slices.Equal(view.Scripts, []string{"window.b=1", "window.a=1"}) ||
		!slices.Equal(view.Libraries, []protocol.ArtifactLibrary{protocol.ArtifactLibraryMermaid}) {
		t.Fatalf("artifact view content: %+v", view)
	}
}
