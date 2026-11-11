package iroh

import (
	"encoding/json"
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/shipped"
)

func TestReleaseLoadsOnlySealedLibrary(t *testing.T) {
	// A release build ignores the development override: the test binary's
	// directory has no seal, so nothing is loaded.
	t.Setenv("SAILRY_IROH_LIB", filepath.Join(t.TempDir(), "libsailry_iroh.dylib"))
	if err := LoadDefault(false); err == nil || !strings.Contains(err.Error(), "the package has no seal") {
		t.Fatalf("release load beside an unsealed executable: %v", err)
	}

	directory := t.TempDir()
	const name = "libsailry_iroh.test"
	if err := os.WriteFile(filepath.Join(directory, name), []byte("shipped"), 0o600); err != nil {
		t.Fatal(err)
	}
	if err := loadSealed(directory, name); err == nil || !strings.Contains(err.Error(), "the package has no seal") {
		t.Fatalf("unsealed directory: %v", err)
	}
	record, err := shipped.Record(directory, name)
	if err != nil {
		t.Fatal(err)
	}
	seal := func(target string, files ...shipped.File) {
		t.Helper()
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
	other.Name = "libother.test"
	seal(target, other)
	if err := loadSealed(directory, name); err == nil || !strings.Contains(err.Error(), "is not in the package seal") {
		t.Fatalf("unlisted library: %v", err)
	}
	seal("plan9/386", record)
	if err := loadSealed(directory, name); err == nil || !strings.Contains(err.Error(), "another platform") {
		t.Fatalf("seal of another platform: %v", err)
	}
	seal(target, record)
	if err := os.WriteFile(filepath.Join(directory, name), []byte("changed"), 0o600); err != nil {
		t.Fatal(err)
	}
	if err := loadSealed(directory, name); err == nil || !strings.Contains(err.Error(), "does not match the package seal") {
		t.Fatalf("changed library: %v", err)
	}
}
