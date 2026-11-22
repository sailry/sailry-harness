//go:build !darwin

package updater

import "github.com/sailry/sailry-harness/internal/identity"

// The Windows out-of-process install helper is not ported, so this build
// checks for no updates there; other systems have no desktop packages.
const unsupported = "desktop update installation is available on macOS only"

func systemVersion() (string, error) { return "", failure("updates_platform", unsupported) }

func installedBundle(identity.DesktopPackage) (string, error) {
	return "", failure("updates_platform", unsupported)
}

func exchangeBundles(string, string) error { return failure("updates_platform", unsupported) }
