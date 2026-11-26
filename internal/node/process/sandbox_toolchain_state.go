//go:build darwin || linux

package process

import (
	"errors"
	"os"
	"path/filepath"
)

// configureToolchainState points shims at their installations without making
// the host's HOME, package-manager configuration or credentials the defaults.
func (p *sandboxPolicy) configureToolchainState(state string, env Environment) error {
	hostPath := func(variable, fallback string) string {
		if value := env.unrestricted[variable]; filepath.IsAbs(value) {
			return value
		}
		return filepath.Join(p.home, fallback)
	}
	for _, manager := range []struct{ variable, relative string }{
		{"PYENV_ROOT", ".pyenv"}, {"ASDF_DATA_DIR", ".asdf"}, {"VOLTA_HOME", ".volta"}, {"MISE_DATA_DIR", ".local/share/mise"},
	} {
		path := hostPath(manager.variable, manager.relative)
		if info, err := os.Stat(path); err == nil && info.IsDir() {
			p.variables[manager.variable] = path
		}
	}
	// Only expose content caches. Keep credentials, locks and configuration
	// in the private state; the sandbox still controls access to link targets.
	for _, cache := range []struct {
		root, target string
		members      []string
	}{
		{hostPath("CARGO_HOME", ".cargo"), "cargo", []string{"registry/src", "registry/cache", "registry/index", "git/checkouts", "git/db"}},
		{hostPath("npm_config_cache", ".npm"), "npm", []string{"_cacache/content-v2", "_cacache/index-v5"}},
	} {
		for _, member := range cache.members {
			source := filepath.Join(cache.root, member)
			if info, err := os.Stat(source); err != nil || !info.IsDir() {
				continue
			}
			dest := filepath.Join(cache.target, member)
			if err := privateDirectories(state, filepath.Dir(dest)); err != nil {
				return err
			}
			if err := os.Symlink(source, filepath.Join(state, dest)); err != nil {
				return err
			}
		}
	}
	for _, name := range []string{".tool-versions", ".python-version"} {
		path := filepath.Join(p.home, name)
		if regular(path) {
			if err := os.Symlink(path, filepath.Join(state, "home", name)); err != nil && !errors.Is(err, os.ErrExist) {
				return err
			}
		}
	}
	return nil
}
