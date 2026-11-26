//go:build !darwin && !windows

package updater

import "github.com/sailry/sailry-harness/internal/identity"

const unsupported = "desktop updates are unavailable for this platform"

func systemVersion() (string, error) { return "", failure("updates_platform", unsupported) }

func installedBundle(identity.DesktopPackage) (string, error) {
	return "", failure("updates_platform", unsupported)
}

func exchangeBundles(string, string) error { return failure("updates_platform", unsupported) }
