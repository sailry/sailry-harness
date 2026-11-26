//go:build darwin || linux

package process

import (
	"os"
	"path/filepath"
	"slices"
	"testing"
)

func TestWorkspaceCredentialLocationsAndToolchainState(t *testing.T) {
	t.Parallel()
	home, config, root, profile := t.TempDir(), t.TempDir(), t.TempDir(), t.TempDir()
	home, err := filepath.EvalSymlinks(home)
	if err != nil {
		t.Fatal(err)
	}
	for _, path := range []string{".cargo/registry/cache", ".npm/_cacache/content-v2", ".pyenv"} {
		if err := os.MkdirAll(filepath.Join(home, path), 0700); err != nil {
			t.Fatal(err)
		}
	}
	env := NewEnvironment([]string{"HOME=" + home, "XDG_CONFIG_HOME=" + config})
	policy, cleanup, err := prepareSandbox(Launch{Root: root, Profile: profile}, env, CaptureWorkspaceBoundary)
	if err != nil {
		t.Fatal(err)
	}
	defer cleanup()
	for _, path := range []string{filepath.Join(config, "git/credentials"), filepath.Join(home, ".config/git/credentials")} {
		if !slices.Contains(policy.deniedReads, path) {
			t.Fatalf("missing credential location %q", path)
		}
	}
	if policy.variables["PYENV_ROOT"] != filepath.Join(home, ".pyenv") {
		t.Fatal("missing manager root")
	}
	for target, want := range map[string]string{filepath.Join(policy.variables["CARGO_HOME"], "registry/cache"): filepath.Join(home, ".cargo/registry/cache"), filepath.Join(policy.variables["npm_config_cache"], "_cacache/content-v2"): filepath.Join(home, ".npm/_cacache/content-v2")} {
		if got, err := os.Readlink(target); err != nil || got != want {
			t.Fatalf("cache link %q = %q, %v", target, got, err)
		}
	}
	if _, err := os.Lstat(filepath.Join(policy.variables["CARGO_HOME"], "credentials.toml")); !os.IsNotExist(err) {
		t.Fatalf("unexpected private cargo credential: %v", err)
	}
}
