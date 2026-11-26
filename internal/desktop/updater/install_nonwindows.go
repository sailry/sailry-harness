//go:build !windows

package updater

func (*Service) installWindows(*staged, *Config, string, string) (bool, error) {
	return false, failure("updates_platform", "a Windows update requires Windows")
}
