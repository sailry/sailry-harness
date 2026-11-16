package turso

import (
	"encoding/json"
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/shipped"
)

func TestReleaseRequiresSealedTurso(t *testing.T) {
	name, _ := libraryPlatform()
	if name == "" {
		t.Skip("Turso SDK has no runtime for this platform")
	}
	t.Setenv("SAILRY_TURSO_LIB", filepath.Join(t.TempDir(), name))
	if _, err := libraryDirectory(name, false); err == nil || !strings.Contains(err.Error(), "no native seal") {
		t.Fatalf("release library from source override: %v", err)
	}
	directory := t.TempDir()
	path := filepath.Join(directory, name)
	if err := os.WriteFile(path, []byte("runtime"), 0o600); err != nil {
		t.Fatal(err)
	}
	if _, err := verifyLibrary(directory, name, false); err != nil {
		t.Fatalf("unsealed development runtime: %v", err)
	}
	if _, err := verifyLibrary(directory, name, true); err == nil || !strings.Contains(err.Error(), "no native seal") {
		t.Fatalf("unsealed release runtime: %v", err)
	}
	record, err := shipped.Record(directory, name)
	if err != nil {
		t.Fatal(err)
	}
	writeSeal := func(target string, files ...shipped.File) {
		t.Helper()
		if runtime.GOOS == "windows" {
			crt := shipped.WindowsCRT{Version: 1, Target: runtime.GOOS + "/" + runtime.GOARCH, Toolset: "fixture", Files: []string{"msvcp140.dll", "vcruntime140.dll", "vcruntime140_1.dll", "windows-crt-Redist.txt", "windows-crt-ThirdPartyNotices.txt"}}
			manifest, _ := json.Marshal(crt)
			for _, name := range append([]string{shipped.WindowsCRTManifest}, crt.Files...) {
				body := []byte(name)
				if name == shipped.WindowsCRTManifest {
					body = manifest
				}
				if err := os.WriteFile(filepath.Join(directory, name), body, 0o600); err != nil {
					t.Fatal(err)
				}
				record, err := shipped.Record(directory, name)
				if err != nil {
					t.Fatal(err)
				}
				files = append(files, record)
			}
		}
		body, err := json.Marshal(shipped.Seal{Version: 1, Target: target, Files: files})
		if err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(filepath.Join(directory, shipped.SealName), body, 0o600); err != nil {
			t.Fatal(err)
		}
	}
	target := runtime.GOOS + "/" + runtime.GOARCH
	other := record
	other.Name = "other-runtime"
	writeSeal(target, other)
	if _, err := verifyLibrary(directory, name, true); err == nil || !strings.Contains(err.Error(), "not in the package seal") {
		t.Fatalf("unlisted runtime: %v", err)
	}
	writeSeal("other/platform", record)
	if _, err := verifyLibrary(directory, name, true); err == nil || !strings.Contains(err.Error(), "another platform") {
		t.Fatalf("runtime for another platform: %v", err)
	}
	writeSeal(target, record)
	if _, err := verifyLibrary(directory, name, true); err != nil {
		t.Fatalf("sealed runtime: %v", err)
	}
	if runtime.GOOS == "windows" {
		if err := os.Remove(filepath.Join(directory, "vcruntime140.dll")); err != nil {
			t.Fatal(err)
		}
		if _, err := verifyLibrary(directory, name, true); err == nil {
			t.Fatal("accepted a packaged runtime without its CRT")
		}
		writeSeal(target, record)
	}
	if err := os.WriteFile(path, []byte("changed"), 0o600); err != nil {
		t.Fatal(err)
	}
	// Even an explicitly selected source runtime must respect an existing seal.
	for _, release := range []bool{false, true} {
		if _, err := verifyLibrary(directory, name, release); err == nil || !strings.Contains(err.Error(), "does not match") {
			t.Fatalf("changed runtime (require seal %v): %v", release, err)
		}
	}
}

func TestTursoRejectsAmbiguousLibraryDirectory(t *testing.T) {
	directory := filepath.Join(t.TempDir(), "native"+string(os.PathListSeparator)+"other")
	if err := os.Mkdir(directory, 0o700); err != nil {
		t.Fatal(err)
	}
	if _, err := verifyLibrary(directory, "unused", false); err == nil || !strings.Contains(err.Error(), "path-list separator") {
		t.Fatalf("ambiguous loader search path: %v", err)
	}
}
