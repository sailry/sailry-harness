package git

import (
	"errors"
	"os"
	"os/exec"
	"path/filepath"

	"golang.org/x/sys/windows/registry"
)

func nativeGitCommand(args ...string) (*exec.Cmd, error) {
	path, err := exec.LookPath("git")
	path, err = windowsGitPath(path, err, registeredGitRoot)
	if err != nil {
		return nil, gitLookupFault(err)
	}
	return exec.Command(path, args...), nil
}

func windowsGitPath(path string, lookupErr error, registered func(registry.Key, uint32) (string, error)) (string, error) {
	if !errors.Is(lookupErr, exec.ErrNotFound) {
		if lookupErr == nil && !filepath.IsAbs(path) {
			return "", exec.ErrDot
		}
		return path, lookupErr
	}
	for _, name := range []string{"ProgramW6432", "ProgramFiles", "ProgramFiles(x86)", "LOCALAPPDATA"} {
		root := os.Getenv(name)
		if name == "LOCALAPPDATA" {
			root = filepath.Join(root, "Programs")
		}
		if path, err := installedGitAt(filepath.Join(root, "Git")); !errors.Is(err, exec.ErrNotFound) {
			return path, err
		}
	}
	for _, hive := range []registry.Key{registry.CURRENT_USER, registry.LOCAL_MACHINE} {
		for _, view := range []uint32{registry.WOW64_64KEY, registry.WOW64_32KEY} {
			root, err := registered(hive, view)
			if errors.Is(err, os.ErrNotExist) {
				continue
			}
			if err != nil {
				return "", err
			}
			if path, err := installedGitAt(root); !errors.Is(err, exec.ErrNotFound) {
				return path, err
			}
		}
	}
	return "", lookupErr
}

func installedGitAt(root string) (string, error) {
	if !filepath.IsAbs(root) {
		return "", exec.ErrNotFound
	}
	for _, dir := range []string{"cmd", "bin"} {
		path := filepath.Join(root, dir, "git.exe")
		info, err := os.Stat(path)
		if errors.Is(err, os.ErrNotExist) {
			continue
		}
		if err != nil {
			return "", err
		}
		if !info.Mode().IsRegular() {
			return "", errors.New("Git executable is not a regular file")
		}
		return path, nil
	}
	return "", exec.ErrNotFound
}

func registeredGitRoot(hive registry.Key, view uint32) (string, error) {
	key, err := registry.OpenKey(hive, `SOFTWARE\GitForWindows`, registry.QUERY_VALUE|view)
	if err != nil {
		return "", err
	}
	defer key.Close()
	root, kind, err := key.GetStringValue("InstallPath")
	if err == nil && kind == registry.EXPAND_SZ {
		return registry.ExpandString(root)
	}
	return root, err
}
