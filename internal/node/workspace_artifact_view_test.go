package node

import (
	"fmt"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/files"
	"github.com/sailry/sailry-harness/internal/node/moduleapi"
	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/protocol"
	briefnode "github.com/sailry/sailry-harness/modules/brief/node"
)

func TestArtifactViewMatchesNativeAndCapturedReferences(t *testing.T) {
	ctx := t.Context()
	owner := serviceTestNode(t, moduleapi.Assembly{Modules: []moduleapi.Module{briefnode.Module()}})
	native := briefnode.ArtifactReference()
	view, err := owner.readArtifactView(ctx, native)
	if err != nil || view == nil {
		t.Fatalf("native renderer on a clean profile: %v", err)
	}
	wantLibraries := []protocol.ArtifactLibrary{protocol.ArtifactLibraryMermaid, protocol.ArtifactLibraryEcharts, protocol.ArtifactLibraryMathjax}
	if !strings.Contains(view.Template, `<head>`) || !strings.Contains(view.Template, `<main id="brief">`) ||
		len(view.Styles) != 1 || len(view.Scripts) != 3 || !slices.Equal(view.Libraries, wantLibraries) {
		t.Fatal("native renderer is missing its template, styles, scripts or library declarations")
	}
	for index, name := range []string{"sailryCharts", "sailryForms", "sailryDocument"} {
		if !strings.Contains(view.Scripts[index], name) {
			t.Fatalf("native script %d should declare %s", index, name)
		}
	}
	fragment, _, err := files.PrepareArtifactHTML(protocol.ArtifactKindDocument,
		`<pre class="mermaid unrelated">graph LR</pre><form class="brief-form" data-name="choices"><button>Submit</button></form><script>fail()</script>`,
		owner.artifactClasses(ctx, native))
	if err != nil || !strings.Contains(fragment, `class="mermaid"`) || !strings.Contains(fragment, `class="brief-form"`) ||
		strings.Contains(fragment, "unrelated") || strings.Contains(fragment, "fail()") {
		t.Fatalf("native fragment %q: %v", fragment, err)
	}

	install := func(version, template, class string) protocol.PluginReference {
		t.Helper()
		root := t.TempDir()
		const path = "dev.sailry.platform/document/template.html"
		if err := os.MkdirAll(filepath.Join(root, filepath.Dir(path)), 0o700); err != nil {
			t.Fatal(err)
		}
		manifest := fmt.Sprintf(`{"$schema":%q,"name":"brief","version":%q,"extensions":{"dev.sailry.platform":{"api_version":"v1","actions":["artifacts.read","artifacts.write"],"artifact_view":{"template":%q,"allowed_classes":[%q]}}}}`, plugins.ManifestSchema, version, path, class)
		for file, content := range map[string]string{"plugin.json": manifest, path: template} {
			if err := os.WriteFile(filepath.Join(root, file), []byte(content), 0o600); err != nil {
				t.Fatal(err)
			}
		}
		info, err := owner.plugins.manager.Install(ctx, root, "", "brief")
		if err != nil {
			t.Fatal(err)
		}
		return plugins.PackageReference(info)
	}
	const oldTemplate = "<html><head></head><body>old</body></html>"
	legacy := install("1.0.0", oldTemplate, "old-class")
	install("1.0.1", "<html><head></head><body>new</body></html>", "new-class")
	if _, ok := owner.modules.ArtifactViewFor(legacy); ok || native.Digest == legacy.Digest {
		t.Fatal("legacy package digest must not resolve as a native renderer")
	}
	old, err := owner.readArtifactView(ctx, legacy)
	if err != nil || old == nil || old.Template != oldTemplate || !slices.Equal(owner.artifactClasses(ctx, legacy), []string{"old-class"}) {
		t.Fatalf("captured renderer: %+v, %v", old, err)
	}
	legacy.Digest = strings.Repeat("a", 64)
	if view, err := owner.readArtifactView(ctx, legacy); view != nil || err == nil {
		t.Fatalf("missing revision must not substitute a different renderer: %+v, %v", view, err)
	}
}
