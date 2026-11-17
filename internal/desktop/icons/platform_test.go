package icons

import "testing"

// A host shows one mark whichever name of its system a view was told: an
// OS, a system's name or a Linux distribution's ID.
func TestPlatformFamily(t *testing.T) {
	for value, want := range map[string]string{
		"darwin": "macos", "macOS": "macos", "ios": "ios", "windows": "windows", "Microsoft Windows 11 Pro": "windows",
		"linux": "linux", "debian": "linux", "ubuntu": "linux", "Arch": "linux", "opensuse-leap": "linux", "android": "android",
		"": "", "plan9": "",
	} {
		if got := Family(value); got != want {
			t.Errorf("Family(%q) = %q, want %q", value, got, want)
		}
	}
	if Platform("debian") != Linux || Platform("darwin") != Apple || Platform("plan9") != Device {
		t.Error("a family's mark differs between its names")
	}
}
