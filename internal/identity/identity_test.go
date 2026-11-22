package identity

import (
	"errors"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestDesktopProfileSelection(t *testing.T) {
	home := filepath.Join(t.TempDir(), "user")
	if _, err := os.Stat(home); !errors.Is(err, os.ErrNotExist) {
		t.Fatalf("fixture home already exists: %v", err)
	}
	official := OfficialDesktop()
	if path, err := profileFromHome(official.DefaultProfile, home); err != nil || path != filepath.Join(home, ".sailry-go") {
		t.Fatalf("official source profile: %q, %v", path, err)
	}
	if path, err := profileFromHome(InstalledProfile, home); err != nil || path != filepath.Join(home, ".sailry") {
		t.Fatalf("official release profile: %q, %v", path, err)
	}
	for _, invalidHome := range []string{"", "relative"} {
		if _, err := profileFromHome(official.DefaultProfile, invalidHome); err == nil || !strings.Contains(err.Error(), "--data-dir") {
			t.Fatalf("invalid home accepted: %q, %v", invalidHome, err)
		}
	}
	fork := Desktop{DefaultProfile: filepath.Join(home, ".fixture-desktop"), HostRelease: OfficialHost()}
	if path, err := fork.DataDir(""); err != nil || path != fork.DefaultProfile || !fork.HostRelease.Published() {
		t.Fatalf("fork profile or Host selection: %q, %v", path, err)
	}
	if path, err := fork.DataDir(filepath.Join(home, "override")); err != nil || path != filepath.Join(home, "override") {
		t.Fatalf("explicit profile: %q, %v", path, err)
	}
	if _, err := os.Stat(home); !errors.Is(err, os.ErrNotExist) {
		t.Fatalf("resolving the profile wrote a directory: %v", err)
	}
}
