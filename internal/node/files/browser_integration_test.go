//go:build integration

package files

import (
	"context"
	"os"
	"regexp"
	"runtime"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// TestBrowseLocationsOnThisSystem checks the places and devices the Node
// reports for the machine and the account it runs on: the parts of the
// resolver that only that system can answer (the known folders and drive
// details of Windows, the mounts of Linux, the volumes of macOS).
func TestBrowseLocationsOnThisSystem(t *testing.T) {
	home, err := os.UserHomeDir()
	if err != nil {
		t.Fatal(err)
	}
	listing, err := Browse(context.Background(), nil, nil)
	if err != nil {
		t.Fatal(err)
	}
	t.Logf("%s reports %+v", runtime.GOOS, listing.Locations)
	if len(listing.Locations) == 0 || listing.Locations[0].Kind != protocol.FileBrowserLocationKindHome || listing.Locations[0].Path != home {
		t.Fatalf("the home does not come first: %+v", listing.Locations)
	}
	paths := map[string]bool{}
	devices := 0
	drive := regexp.MustCompile(`^[A-Z]:\\$`)
	for _, location := range listing.Locations {
		if paths[location.Path] {
			t.Errorf("%s is listed twice", location.Path)
		}
		paths[location.Path] = true
		if info, err := os.Stat(location.Path); err != nil || !info.IsDir() {
			t.Errorf("%s is no directory: %v", location.Path, err)
		}
		device := location.Kind == protocol.FileBrowserLocationKindVolume || location.Kind == protocol.FileBrowserLocationKindRoot
		if device {
			devices++
		} else if devices > 0 {
			t.Errorf("the place %+v follows a device", location)
		}
		switch {
		case !device && location.Medium != nil:
			t.Errorf("the place %+v has a medium", location)
		case device && runtime.GOOS == "windows":
			if !drive.MatchString(location.Path) || !strings.Contains(location.Name, location.Path[:2]) {
				t.Errorf("the drive %+v is not a named drive letter", location)
			}
		case device && location.Kind == protocol.FileBrowserLocationKindVolume && runtime.GOOS == "darwin":
			if !strings.HasPrefix(location.Path, "/Volumes/") || location.Name == "" {
				t.Errorf("the volume %+v is not one of /Volumes", location)
			}
		case device && location.Kind == protocol.FileBrowserLocationKindVolume && runtime.GOOS == "linux":
			if !strings.HasPrefix(location.Path, "/media/") && !strings.HasPrefix(location.Path, "/run/media/") && !strings.HasPrefix(location.Path, "/mnt/") {
				t.Errorf("the volume %+v is no user media", location)
			}
		}
	}
	if devices == 0 {
		t.Error("no device is listed")
	}
	if runtime.GOOS != "windows" && !paths["/"] {
		t.Error("the root is not listed")
	}
}
