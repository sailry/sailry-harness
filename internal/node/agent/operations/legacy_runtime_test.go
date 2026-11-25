package operations

import (
	"archive/zip"
	"io/fs"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestRealLegacyOfficialTurnCaptureRequiresNativeModules(t *testing.T) {
	t.Parallel()
	archive, err := zip.OpenReader("../../plugins/testdata/official-legacy.zip")
	if err != nil {
		t.Fatal(err)
	}
	defer archive.Close()
	for _, name := range []string{"databases", "files", "git", "ssh", "worktrees"} {
		t.Run(name, func(t *testing.T) {
			source, profile := t.TempDir(), t.TempDir()
			for _, file := range archive.File {
				if !strings.HasPrefix(file.Name, name+"/") {
					continue
				}
				target := filepath.Join(source, filepath.FromSlash(strings.TrimPrefix(file.Name, name+"/")))
				if file.FileInfo().IsDir() {
					if err := os.MkdirAll(target, 0o755); err != nil {
						t.Fatal(err)
					}
					continue
				}
				data, err := fs.ReadFile(archive, file.Name)
				if err != nil {
					t.Fatal(err)
				}
				if err := os.MkdirAll(filepath.Dir(target), 0o755); err != nil {
					t.Fatal(err)
				}
				if err := os.WriteFile(target, data, file.Mode().Perm()); err != nil {
					t.Fatal(err)
				}
			}
			allowed := plugins.WithRuntime(t.Context(), plugins.Runtime{ProductID: "harness", Modules: []string{name}})
			blocked := plugins.WithRuntime(t.Context(), plugins.Runtime{ProductID: "harness"})
			info, err := plugins.NewManager(profile).Install(allowed, source, "", name)
			if err != nil {
				t.Fatal(err)
			}
			references := []protocol.PluginReference{plugins.PackageReference(info)}
			if _, err := CaptureCatalog(blocked, profile, references); err == nil {
				t.Fatal("captured turn admitted actual undeclared legacy native tools")
			}
			if _, err := CaptureResources(blocked, profile, references); err == nil {
				t.Fatal("captured turn admitted actual undeclared legacy native resources")
			}
			catalog, err := CaptureCatalog(allowed, profile, references)
			if err != nil {
				t.Fatal(err)
			}
			for _, tool := range catalog.Tools {
				if tool.Owner != references[0] || tool.Name != Alias(name, tool.Exposed) {
					t.Fatal("legacy tool lost its admitted package identity", tool)
				}
			}
			resources, err := CaptureResources(allowed, profile, references)
			if err != nil {
				t.Fatal(err)
			}
			for key, skill := range resources.Skills() {
				want, err := fs.ReadFile(archive, name+"/"+skill.Path)
				if err != nil {
					t.Fatal(err)
				}
				read, err := resources.Read(allowed, key, "SKILL.md")
				if err != nil || read.Content != string(want) {
					t.Fatal("legacy skill did not retain its captured content", key, err)
				}
			}
		})
	}
}
