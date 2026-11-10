//go:build !darwin

package updater

// The Windows out-of-process install helper is not ported, so this build
// checks for no updates there; other systems have no desktop packages.
const unsupported = "desktop update installation is available on macOS only"

func systemVersion() (string, error) { return "", failure("updates_platform", unsupported) }

func installedBundle() (string, error) { return "", failure("updates_platform", unsupported) }
