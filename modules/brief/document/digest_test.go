package document

import (
	"io/fs"
	"maps"
	"slices"
	"testing"
	"testing/fstest"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestRendererV1Identity(t *testing.T) {
	want := protocol.PluginReference{
		Name:   "brief",
		Digest: "6f06ad5c3dd8a121f495ec92cb70690be38b8fa0e90f2f1ab45b0e44bc6139b5",
	}
	got, _, _ := ResourcesV1()
	if got != want {
		t.Fatalf("renderer v1 identity = %+v, want %+v; keep v1 immutable and register a new version for changes", got, want)
	}
}

func TestDigestV1TracksDeclarationAndResources(t *testing.T) {
	reference, view, tree := ResourcesV1()
	assertChanged := func(t *testing.T, version string, view protocol.ArtifactView, tree fs.FS) {
		t.Helper()
		got, err := digestV1(version, view, tree)
		if err != nil {
			t.Fatal(err)
		}
		if got == reference.Digest {
			t.Fatal("changed renderer retained the frozen v1 digest")
		}
	}
	t.Run("version", func(t *testing.T) {
		assertChanged(t, "test-version", view, tree)
	})
	for _, tt := range []struct {
		name string
		edit func(*protocol.ArtifactView)
	}{
		{"styles declaration", func(view *protocol.ArtifactView) { view.Styles = nil }},
		{"script order", func(view *protocol.ArtifactView) { view.Scripts[0], view.Scripts[1] = view.Scripts[1], view.Scripts[0] }},
		{"library order", func(view *protocol.ArtifactView) {
			view.Libraries[0], view.Libraries[1] = view.Libraries[1], view.Libraries[0]
		}},
		{"allowed classes", func(view *protocol.ArtifactView) { view.AllowedClasses[0] = "different" }},
	} {
		t.Run(tt.name, func(t *testing.T) {
			_, modified, _ := ResourcesV1()
			tt.edit(&modified)
			assertChanged(t, versionV1, modified, tree)
		})
	}

	paths := slices.Concat([]string{view.Template}, view.Styles, view.Scripts)
	files := make(fstest.MapFS, len(paths))
	for _, path := range paths {
		body, err := fs.ReadFile(tree, path)
		if err != nil {
			t.Fatal(err)
		}
		files[path] = &fstest.MapFile{Data: body}
	}
	if got, err := digestV1(versionV1, view, files); err != nil || got != reference.Digest {
		t.Fatalf("fixture identity = %q, %v; want %q", got, err, reference.Digest)
	}
	t.Run("template path with identical bytes", func(t *testing.T) {
		modified := view
		modified.Template = "renamed.html"
		changed := maps.Clone(files)
		changed[modified.Template] = files[view.Template]
		assertChanged(t, versionV1, modified, changed)
	})
	for _, path := range paths {
		t.Run(path+" bytes", func(t *testing.T) {
			changed := maps.Clone(files)
			changed[path] = &fstest.MapFile{Data: append(slices.Clone(files[path].Data), '\n')}
			assertChanged(t, versionV1, view, changed)
		})
	}
	// Mutating copies in the declaration cases must not affect the embed's view.
	_, current, _ := ResourcesV1()
	if got, err := digestV1(versionV1, current, tree); err != nil || got != reference.Digest {
		t.Fatalf("registered v1 declaration changed: %q, %v", got, err)
	}
}
