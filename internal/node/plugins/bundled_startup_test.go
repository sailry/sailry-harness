package plugins

import (
	"context"
	"embed"
	"errors"
	"io/fs"
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"testing"
	"testing/fstest"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestUnbundledDefaultsAndCatalog(t *testing.T) {
	t.Parallel()
	if defaults := BundledDefaults(); defaults == nil || len(defaults) != 0 {
		t.Fatalf("stock defaults must explicitly select no packages: %+v", defaults)
	}
	entries, err := BundledEntries(t.Context())
	if err != nil || len(entries) != 0 {
		t.Fatalf("stock package catalog: %+v %v", entries, err)
	}
	skills, err := BundledSkillEntries(t.Context())
	if err != nil || len(skills) != 0 {
		t.Fatalf("stock skill catalog: %+v %v", skills, err)
	}
}

func TestOptionalBundledCatalog(t *testing.T) {
	t.Parallel()
	withoutCatalog := bundledFixture(t)
	delete(withoutCatalog, "plugins/catalog.json")
	for _, scenario := range []struct {
		name      string
		files     fs.FS
		wantError bool
	}{
		{"empty embed", embed.FS{}, false},
		{"empty filesystem", fstest.MapFS{}, false},
		{"empty directory", fstest.MapFS{"plugins": {Mode: fs.ModeDir | 0o700}}, false},
		{"missing catalog for supplied packages", withoutCatalog, true},
		{"malformed catalog", fstest.MapFS{"plugins/catalog.json": {Data: []byte(`{`)}}, true},
		{"unsupported catalog", fstest.MapFS{"plugins/catalog.json": {Data: []byte(`{"version":2,"packages":[]}`)}}, true},
	} {
		t.Run(scenario.name, func(t *testing.T) {
			entries, err := bundledEntries(t.Context(), scenario.files)
			if (err != nil) != scenario.wantError || len(entries) != 0 {
				t.Fatalf("optional catalog: %+v %v", entries, err)
			}
		})
	}
	ctx, cancel := context.WithCancel(t.Context())
	cancel()
	if _, err := bundledEntries(ctx, fstest.MapFS{}); !errors.Is(err, context.Canceled) {
		t.Fatalf("empty catalog ignored cancellation: %v", err)
	}
}

func TestInstalledPackageSurvivesMissingOfflineSource(t *testing.T) {
	t.Parallel()
	files := bundledFixture(t)
	profile := t.TempDir()
	manager := NewManager(profile, files)
	info, err := manager.InstallBundled(t.Context(), bundledFixtureName)
	if err != nil {
		t.Fatal(err)
	}
	reference := PackageReference(info)
	manager = NewManager(profile, fstest.MapFS{})
	if err := manager.VerifyPackage(t.Context(), reference); err != nil {
		t.Fatal("healthy installed package requires an offline copy:", err)
	}
	captured, err := manager.CapturedInfo(t.Context(), reference)
	if err != nil || captured.Summary.Digest != reference.Digest || len(captured.Skills) != 1 || captured.Extension == nil {
		t.Fatalf("installed capture without offline source: %+v %v", captured, err)
	}
	const documentPath = "skills/inspect/SKILL.md"
	content, err := manager.ReadResource(t.Context(), reference, documentPath, MaxSkillDocumentBytes)
	if err != nil || string(content) != string(files["plugins/"+bundledFixtureName+"/"+documentPath].Data) {
		t.Fatalf("installed resource without offline source: %q %v", content, err)
	}
	if _, err := manager.RestoreBundled(t.Context(), reference); !errors.Is(err, fs.ErrNotExist) {
		t.Fatalf("absent offline source reported something other than absence: %v", err)
	}
	if err := manager.VerifyPackage(t.Context(), reference); err != nil {
		t.Fatal("unavailable offline recovery changed the installation:", err)
	}
}

func TestUnavailableOfflineRecoveryPreservesLocalContent(t *testing.T) {
	t.Parallel()
	for _, source := range []string{"missing", "different digest"} {
		t.Run(source, func(t *testing.T) {
			files := bundledFixture(t)
			manager := NewManager(t.TempDir(), files)
			info, err := manager.InstallBundled(t.Context(), bundledFixtureName)
			if err != nil {
				t.Fatal(err)
			}
			reference := PackageReference(info)
			root, err := manager.PackageRoot(reference)
			if err != nil {
				t.Fatal(err)
			}
			marker := filepath.Join(root, "local-content")
			if err := os.WriteFile(marker, []byte("retain"), 0o600); err != nil {
				t.Fatal(err)
			}
			if source == "missing" {
				manager = NewManager(manager.profile, fstest.MapFS{})
			} else {
				manifest := files["plugins/"+bundledFixtureName+"/plugin.json"]
				manifest.Data = []byte(strings.Replace(string(manifest.Data), `"version":"1.0.0"`, `"version":"2.0.0"`, 1))
			}
			_, err = manager.RestoreBundled(t.Context(), reference)
			if source == "missing" {
				if !errors.Is(err, fs.ErrNotExist) {
					t.Fatalf("missing source: %v", err)
				}
			} else {
				var fault *protocol.Fault
				if !errors.As(err, &fault) || fault.Code != protocol.ErrorCodeInvalidRequest || fault.Message != "matching bundled plugin package is unavailable" {
					t.Fatalf("different offline digest: %v", err)
				}
			}
			if content, err := os.ReadFile(marker); err != nil || string(content) != "retain" {
				t.Fatalf("unavailable recovery changed local content: %q %v", content, err)
			}
			quarantined, err := filepath.Glob(filepath.Join(filepath.Dir(root), ".quarantine-*"))
			if err != nil || len(quarantined) != 0 {
				t.Fatalf("unavailable recovery quarantined local content: %v %v", quarantined, err)
			}
		})
	}
}

func TestRestoreBundledRepairsOnlyExactContent(t *testing.T) {
	t.Parallel()
	for _, damage := range []string{"missing", "changed"} {
		t.Run(damage, func(t *testing.T) {
			manager := NewManager(t.TempDir(), bundledFixture(t))
			info, err := manager.InstallBundled(t.Context(), bundledFixtureName)
			if err != nil {
				t.Fatal(err)
			}
			reference := PackageReference(info)
			root, err := manager.PackageRoot(reference)
			if err != nil {
				t.Fatal(err)
			}
			const marker = "local-content"
			if damage == "missing" {
				err = os.RemoveAll(root)
			} else {
				err = os.WriteFile(filepath.Join(root, marker), []byte("retain"), 0o600)
			}
			if err != nil {
				t.Fatal(err)
			}
			restored, err := manager.RestoreBundled(t.Context(), reference)
			if err != nil || restored.Summary.Digest != reference.Digest {
				t.Fatalf("exact recovery: %+v %v", restored, err)
			}
			if err := manager.VerifyPackage(t.Context(), reference); err != nil {
				t.Fatal(err)
			}
			if damage == "changed" {
				quarantined, err := filepath.Glob(filepath.Join(filepath.Dir(root), ".quarantine-"+reference.Digest+"-*", marker))
				if err != nil || len(quarantined) != 1 {
					t.Fatalf("changed content was not preserved: %v %v", quarantined, err)
				}
				if content, err := os.ReadFile(quarantined[0]); err != nil || string(content) != "retain" {
					t.Fatalf("recovery lost changed content: %q %v", content, err)
				}
			}
		})
	}
}

func TestRestoreBundledRejectsLinkedPackageStore(t *testing.T) {
	if runtime.GOOS == "windows" {
		t.Skip("symbolic links require additional privileges on Windows")
	}
	profile, target := t.TempDir(), t.TempDir()
	manager := NewManager(profile, bundledFixture(t))
	info, err := manager.InspectBundled(t.Context(), bundledFixtureName)
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
