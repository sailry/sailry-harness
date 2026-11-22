package updater

import (
	"os"
	"path/filepath"
	"strings"

	"github.com/sailry/sailry-harness/internal/identity"
	"golang.org/x/sys/unix"
)

// installedBundle is the application bundle the running executable belongs to.
func installedBundle(product identity.DesktopPackage) (string, error) {
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
		if filepath.Join(root, "Contents", "MacOS", product.Executable) != executable {
			return "", failure("updates_bundle_required", "the executable is outside the expected application bundle")
		}
		plist, err := confined(root, filepath.Join(root, "Contents", "Info.plist"))
		if err != nil {
			return "", err
		}
		data, err := readLimited(plist, 128*1024)
		if err != nil {
			return "", err
		}
		values, err := plistStrings(data)
		if err != nil || values["CFBundleIdentifier"] != product.ID || values["CFBundleExecutable"] != product.Executable {
			return "", failure("updates_bundle_required", "the running application bundle does not match the configured identity")
		}
		return root, nil
	}
	return "", failure("updates_bundle_required", "updates require a packaged macOS application")
}

func exchangeBundles(staged, installed string) error {
	return unix.RenamexNp(staged, installed, unix.RENAME_SWAP)
}
