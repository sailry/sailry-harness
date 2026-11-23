package plugins

import (
	"encoding/json"
	"io/fs"
	"os"
	"path/filepath"
	"runtime"
	"testing"

	sailry "github.com/sailry/sailry-harness"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestBundledStartupCompatibility(t *testing.T) {
	manager := NewManager(t.TempDir())
	for _, entry := range BundledDefaults() {
		t.Run(entry.Name, func(t *testing.T) {
			info, err := manager.InspectBundled(t.Context(), entry.Name)
			if err != nil {
				t.Fatal(err)
			}
			if compatibility := InfoCompatibility(info); compatibility.Kind != "compatible" {
				data, err := fs.ReadFile(sailry.Plugins, "plugins/"+entry.Name+"/plugin.json")
				if err != nil {
					t.Fatal(err)
				}
				var manifest struct {
					Extensions map[string]json.RawMessage `json:"extensions"`
				}
				if err := json.Unmarshal(data, &manifest); err != nil {
					t.Fatal(err)
				}
				var extension protocol.PluginExtension
				err = json.Unmarshal(manifest.Extensions["dev.sailry.platform"], &extension)
				if err == nil {
					err = validateExtension(extension)
				}
				t.Fatalf("bundled package is incompatible: %s requirements=%+v issues=%+v declaration=%v", entry.Name, info.Requirements, info.Issues, err)
			}
		})
	}
}

func TestRestoreBundledRejectsLinkedPackageStore(t *testing.T) {
	if runtime.GOOS == "windows" {
		t.Skip("symbolic links require additional privileges on Windows")
	}
	profile, target := t.TempDir(), t.TempDir()
	manager := NewManager(profile)
	info, err := manager.InspectBundled(t.Context(), "reminders")
	if err != nil {
		t.Fatal(err)
	}
	if err := os.Mkdir(filepath.Join(profile, "plugins"), 0o700); err != nil {
		t.Fatal(err)
	}
	if err := os.Symlink(target, filepath.Join(profile, "plugins", "packages")); err != nil {
		t.Fatal(err)
	}
	if _, err := manager.RestoreBundled(t.Context(), PackageReference(info)); err == nil {
		t.Fatal("bundled recovery accepted a linked package store")
	}
	if entries, err := os.ReadDir(target); err != nil || len(entries) != 0 {
		t.Fatalf("bundled recovery wrote outside the package store: %v, %v", entries, err)
	}
}
