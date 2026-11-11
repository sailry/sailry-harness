package externalbrowser

import (
	"os"
	"os/exec"
	"path/filepath"
	"runtime"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// Installed finds a Chrome, Chromium or Edge installed on this host, or
// returns "". Nothing else is ever used as the browser and nothing is
// downloaded. It is only ever started with a session's own user data
// directory.
func Installed() string {
	var candidates []string
	switch runtime.GOOS {
	case "darwin":
		for _, root := range []string{"/Applications", filepath.Join(os.Getenv("HOME"), "Applications")} {
			candidates = append(candidates,
				filepath.Join(root, "Google Chrome.app/Contents/MacOS/Google Chrome"),
				filepath.Join(root, "Chromium.app/Contents/MacOS/Chromium"),
				filepath.Join(root, "Microsoft Edge.app/Contents/MacOS/Microsoft Edge"))
		}
	case "windows":
		for _, root := range []string{os.Getenv("ProgramFiles"), os.Getenv("ProgramFiles(x86)"), os.Getenv("LocalAppData")} {
			if root != "" {
				candidates = append(candidates,
					filepath.Join(root, `Google\Chrome\Application\chrome.exe`),
					filepath.Join(root, `Chromium\Application\chrome.exe`),
					filepath.Join(root, `Microsoft\Edge\Application\msedge.exe`))
			}
		}
	default:
		for _, name := range []string{"google-chrome", "google-chrome-stable", "chromium", "chromium-browser", "microsoft-edge", "microsoft-edge-stable"} {
			if found, err := exec.LookPath(name); err == nil {
				return found
			}
		}
		candidates = append(candidates, "/snap/bin/chromium")
	}
	for _, candidate := range candidates {
		if info, err := os.Stat(candidate); err == nil && info.Mode().IsRegular() {
			return candidate
		}
	}
	return ""
}

// Browser resolves the browser a session starts. Without an installed browser
// every call fails at once with the same fault.
func (browsers *Browsers) Browser() (string, error) {
	if browsers.options.Profile == "" {
		return "", unavailable("external browser is unavailable on the execution Node")
	}
	found := browsers.find()
	if found == "" {
		return "", fault(protocol.ErrorCodeNotConfigured, "no supported browser is installed on the execution Node; install Google Chrome, Chromium or Microsoft Edge")
	}
	return found, nil
}
