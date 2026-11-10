package updater

import (
	"os"
	"path/filepath"
	"strings"
)

// installedBundle is the application bundle the running executable belongs to.
func installedBundle() (string, error) {
	executable, err := os.Executable()
	if err != nil {
		return "", ioFailure(err)
	}
	if executable, err = filepath.EvalSymlinks(executable); err != nil {
		return "", ioFailure(err)
	}
	for root := executable; root != filepath.Dir(root); root = filepath.Dir(root) {
		if !strings.HasSuffix(root, ".app") {
			continue
		}
		if filepath.Join(root, "Contents", "MacOS", "sailry-desktop") != executable {
			return "", failure("updates_bundle_required", "the executable is outside the expected application bundle")
		}
		return root, nil
	}
	return "", failure("updates_bundle_required", "updates require a packaged macOS application")
}
