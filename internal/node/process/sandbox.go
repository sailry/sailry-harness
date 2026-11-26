//go:build darwin || linux

package process

import (
	"errors"
	"fmt"
	"maps"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"slices"
	"strings"
	"syscall"

	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type allowedPath struct {
	path  string
	write bool
}
type sandboxPolicy struct {
	paths         []allowedPath
	variables     map[string]string
	deniedReads   []string
	protected     []string
	home, profile string
}

// sandboxDeniedHomeReads are sensitive credential paths under the user's real
// home directory that sandboxed commands must never read.
var sandboxDeniedHomeReads = []string{
	".ssh",
	".gnupg",
	".aws",
	".azure",
	".kube",
	".docker",
	".gitconfig",
	".git-credentials",
	".config/git/credentials",
	".npmrc",
	".netrc",
	".pypirc",
	".cargo/credentials",
	".cargo/credentials.toml",
	".config/gh",
	".config/gcloud",
	".config/op",
}

func prepareCommand(launch Launch, environment Environment, directory, device string, captureManaged func(string) (string, error)) (*exec.Cmd, func(), error) {
	values := environment.commandValues(launch.Access)
	if values == nil {
		values = map[string]string{}
	}
	maps.Copy(values, launch.Environment)
	if launch.Inputs != "" {
		values["SAILRY_ATTACHMENTS"] = launch.Inputs
	}
	if launch.Scratch != "" {
		for _, key := range []string{"TMPDIR", "TMP", "TEMP"} {
			values[key] = launch.Scratch
		}
	}
	values["PWD"] = directory
	if launch.Pty {
		values["TERM"] = ptyTerm
	}
	program, args := environment.Shell(), []string{"-c", launch.Command}
	cleanup := func() {}
	if launch.Access == protocol.ProcessAccessWorkspace {
		policy, release, err := prepareSandbox(launch, environment, captureManaged)
		if err != nil {
			return nil, cleanup, err
		}
		cleanup = release
		program, args, err = sandboxCommand(policy, program, args, device)
		if err != nil {
			cleanup()
			return nil, func() {}, err
		}
		for key := range values {
			if strings.HasPrefix(key, "LD_") || strings.HasPrefix(key, "DYLD_") {
				delete(values, key)
			}
		}
		maps.Copy(values, policy.variables)
	}
	command := exec.Command(program, args...)
	command.Dir, command.Env = directory, environmentValues(values)
	return command, cleanup, nil
}

func prepareSandbox(launch Launch, environment Environment, captureManaged func(string) (string, error)) (_ sandboxPolicy, cleanup func(), failure error) {
	cleanup = func() {}
	policy := sandboxPolicy{variables: map[string]string{}}
	root, err := filepath.EvalSymlinks(launch.Root)
	if err != nil {
		return policy, cleanup, sandboxFailure(err)
	}
	home := environment.values["HOME"]
	if home == "" {
		home = environment.values["USERPROFILE"]
	}
	if home != "" {
		if resolved, err := filepath.EvalSymlinks(home); err == nil {
			policy.home = resolved
		} else {
			policy.home = filepath.Clean(home)
		}
	}
	if launch.Profile != "" {
		policy.profile, err = filepath.EvalSymlinks(launch.Profile)
		if err != nil {
			return policy, cleanup, sandboxFailure(err)
		}
	}
	if filepath.Dir(root) == root || policy.home != "" && within(root, policy.home) || policy.profile != "" && (within(root, policy.profile) || within(policy.profile, root) && !core.ManagedWorkspace(root, policy.profile)) {
		return policy, cleanup, &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "workspace overlaps protected data; request unrestricted access"}
	}
	scratch := launch.Scratch
	ownScratch := scratch == ""
	if ownScratch {
		parent := ""
		if policy.profile != "" {
			parent = filepath.Join(policy.profile, "tmp")
			if err = os.MkdirAll(parent, 0700); err != nil {
				return policy, cleanup, sandboxFailure(err)
			}
		}
		scratch, err = os.MkdirTemp(parent, "sailry-command-")
		if err != nil {
			return policy, cleanup, sandboxFailure(err)
		}
	}
	scratch, err = filepath.EvalSymlinks(scratch)
	if err != nil {
		return policy, cleanup, sandboxFailure(err)
	}
	state, err := os.MkdirTemp(scratch, "environment-")
	if err != nil {
		if ownScratch {
			_ = os.RemoveAll(scratch)
		}
		return policy, cleanup, sandboxFailure(err)
	}
	cleanup = func() {
		_ = os.RemoveAll(state)
		if ownScratch {
			_ = os.RemoveAll(scratch)
		}
	}
	defer func() {
		if failure != nil {
			cleanup()
		}
	}()
	policy.paths = append(policy.paths, allowedPath{root, !launch.ReadOnly}, allowedPath{scratch, true})
	if launch.Inputs != "" {
		inputs, err := filepath.EvalSymlinks(launch.Inputs)
		if err != nil {
			return policy, cleanup, sandboxFailure(err)
		}
		policy.paths = append(policy.paths, allowedPath{inputs, false})
	}
	for name, relative := range map[string]string{"HOME": "home", "XDG_CACHE_HOME": "cache", "XDG_CONFIG_HOME": "config", "XDG_DATA_HOME": "data", "XDG_STATE_HOME": "state", "CARGO_HOME": "cargo", "npm_config_cache": "npm", "PIP_CACHE_DIR": "pip", "PYTHONPYCACHEPREFIX": "pycache"} {
		if err := privateDirectories(state, relative); err != nil {
			return policy, cleanup, sandboxFailure(err)
		}
		policy.variables[name] = filepath.Join(state, relative)
	}
	for _, name := range []string{"TMPDIR", "TMP", "TEMP"} {
		policy.variables[name] = scratch
	}
	file, err := os.OpenFile(filepath.Join(state, "npmrc"), os.O_CREATE|os.O_EXCL|os.O_WRONLY, 0600)
	if err != nil {
		return policy, cleanup, sandboxFailure(err)
	}
	if err = file.Close(); err != nil {
		return policy, cleanup, sandboxFailure(err)
	}
	policy.variables["npm_config_userconfig"], policy.variables["HISTFILE"] = filepath.Join(state, "npmrc"), filepath.Join(state, "history")
	policy.variables["BASH_ENV"], policy.variables["ENV"] = "/dev/null", "/dev/null"
	if runtime.GOOS == "darwin" {
		policy.variables["xcrun_db"] = filepath.Join(state, "xcrun_db")
	}
	// Preserve read-only references to host toolchains/caches that would
	// otherwise default under the isolated HOME.
	if policy.home != "" {
		if err := policy.configureToolchainState(state, environment); err != nil {
			return policy, cleanup, sandboxFailure(err)
		}
		if environment.unrestricted["RUSTUP_HOME"] != "" && filepath.IsAbs(environment.unrestricted["RUSTUP_HOME"]) {
			policy.variables["RUSTUP_HOME"] = environment.unrestricted["RUSTUP_HOME"]
		} else if regular(filepath.Join(policy.home, ".rustup/settings.toml")) {
			policy.variables["RUSTUP_HOME"] = filepath.Join(policy.home, ".rustup")
		}
		if environment.values["GOMODCACHE"] == "" && environment.values["GOPATH"] == "" {
			if info, err := os.Stat(filepath.Join(policy.home, "go/pkg/mod")); err == nil && info.IsDir() {
				policy.variables["GOMODCACHE"] = filepath.Join(policy.home, "go/pkg/mod")
			}
		}
	}
	// Deny reads to Sailry's own profile (excluding the command's scratch and
	// managed workspace, which are re-granted after denies) and user secrets.
	if policy.profile != "" {
		policy.denyRead(policy.profile)
	}
	if policy.home != "" {
		for _, relative := range sandboxDeniedHomeReads {
			policy.denyRead(filepath.Join(policy.home, filepath.FromSlash(relative)))
		}
	}
	// Git also uses the host's XDG configuration root when it is absolute.
	if config := environment.unrestricted["XDG_CONFIG_HOME"]; filepath.IsAbs(config) {
		policy.denyRead(filepath.Join(config, "git", "credentials"))
	}
	policy.captureRepositories(root)
	if captureManaged == nil {
		return policy, cleanup, unavailable("repository boundary capture is unavailable")
	}
	managed, err := captureManaged(root)
	if err != nil {
		return policy, cleanup, err
	}
	if managed != "" {
		policy.protected = append(policy.protected, managed)
	}
	return policy, cleanup, nil
}

func (policy *sandboxPolicy) denyRead(path string) {
	if !filepath.IsAbs(path) {
		return
	}
	clean := filepath.Clean(path)
	if !slices.Contains(policy.deniedReads, clean) {
		policy.deniedReads = append(policy.deniedReads, clean)
	}
	if resolved, err := filepath.EvalSymlinks(clean); err == nil && filepath.IsAbs(resolved) && resolved != clean {
		if !slices.Contains(policy.deniedReads, resolved) {
			policy.deniedReads = append(policy.deniedReads, resolved)
		}
	}
}

func trustedWrapper(candidates ...string) (string, error) {
	for _, candidate := range candidates {
		path, err := filepath.EvalSymlinks(candidate)
		if err != nil {
			continue
		}
		info, err := os.Stat(path)
		if err != nil {
			return "", sandboxFailure(err)
		}
		if stat, ok := info.Sys().(*syscall.Stat_t); ok && info.Mode().IsRegular() && stat.Uid == 0 && info.Mode().Perm()&0022 == 0 {
			return path, nil
		}
	}
	return "", unavailable("command sandbox is unavailable; request unrestricted access to continue")
}

func privateDirectories(root, relative string) error {
	anchor, err := os.OpenRoot(root)
	if err != nil {
		return err
	}
	defer func() { anchor.Close() }()
	for _, name := range strings.Split(filepath.ToSlash(relative), "/") {
		if err = anchor.Mkdir(name, 0700); err != nil && !errors.Is(err, os.ErrExist) {
			return err
		}
		info, err := anchor.Lstat(name)
		if err != nil {
			return err
		}
		if !info.IsDir() || info.Mode()&os.ModeSymlink != 0 {
			return errors.New("command state parent is not a directory")
		}
		child, err := anchor.OpenRoot(name)
		if err != nil {
			return err
		}
		opened, err := child.Stat(".")
		if err != nil || !os.SameFile(info, opened) {
			child.Close()
			return errors.New("command state parent changed")
		}
		anchor.Close()
		anchor = child
	}
	return nil
}

func regular(path string) bool {
	info, err := os.Stat(path)
	return err == nil && info.Mode().IsRegular()
}
func sandboxFailure(err error) error {
	return unavailable(fmt.Sprintf("command sandbox failed: %v", err))
}

// orderedPaths applies nested grants shallowest first, with write winning a tie.
func orderedPaths(paths []allowedPath) []allowedPath {
	paths = slices.Clone(paths)
	slices.SortStableFunc(paths, func(a, b allowedPath) int {
		x, y := len(strings.Split(a.path, string(filepath.Separator))), len(strings.Split(b.path, string(filepath.Separator)))
		if x != y {
			return x - y
		}
		if a.write == b.write {
			return 0
		}
		if a.write {
			return 1
		}
		return -1
	})
	return paths
}

func effectivePath(paths []allowedPath, path string, strict bool) (bool, bool) {
	found, write, depth := false, false, -1
	for _, entry := range paths {
		if within(entry.path, path) && (!strict || entry.path != path) && (len(entry.path) > depth || len(entry.path) == depth && entry.write) {
			found, write, depth = true, entry.write, len(entry.path)
		}
	}
	return write, found
}

func pinnedPaths(paths []allowedPath) []string {
	pinned := map[string]bool{}
	for _, entry := range paths {
		parent, parentFound := effectivePath(paths, entry.path, true)
		effective, _ := effectivePath(paths, entry.path, false)
		if !entry.write && parentFound && parent && !effective {
			for path := filepath.Dir(entry.path); ; path = filepath.Dir(path) {
				if write, ok := effectivePath(paths, path, false); !ok || !write {
					break
				}
				pinned[path] = true
				if filepath.Dir(path) == path {
					break
				}
			}
		} else if entry.write && parentFound && !parent {
			pinned[entry.path] = true
		}
	}
	return slices.Sorted(maps.Keys(pinned))
}
