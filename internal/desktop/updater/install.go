package updater

import (
	"fmt"
	"io"
	"os"
	"os/exec"
	"path/filepath"
)

// Whole-application installation, never a binary-only swap.

// separateData refuses a replacement that would include the data profile or
// the updater's own cache.
func separateData(installed, profile, cache string) error {
	installed, err := filepath.EvalSymlinks(installed)
	if err != nil {
		return ioFailure(err)
	}
	profile, err = filepath.EvalSymlinks(profile)
	if err != nil {
		return ioFailure(err)
	}
	// The cache may not exist yet, and its parent can be a symlink. Resolve
	// its nearest existing ancestor without creating anything inside a bundle.
	ancestor := cache
	for {
		if _, err := os.Lstat(ancestor); err == nil {
			break
		}
		parent := filepath.Dir(ancestor)
		if parent == ancestor {
			return failure("updates_io_failed", "the update cache has no existing parent")
		}
		ancestor = parent
	}
	ancestor, err = filepath.EvalSymlinks(ancestor)
	if err != nil {
		return ioFailure(err)
	}
	if inside(installed, profile) || inside(installed, ancestor) {
		return failure("updates_profile_inside", "application replacement cannot include the data profile or controller recovery cache")
	}
	return nil
}

func inside(root, path string) bool {
	relative, err := filepath.Rel(root, path)
	return err == nil && (relative == "." || filepath.IsLocal(relative))
}

// replace installs the staged bundle over the installed one. It first keeps
// a complete, independent copy of the original beside it, and returns that
// copy's directory. A failure after the copy exists is an uncertain outcome
// naming where the original is retained.
func replace(update *staged, config *Config, installed string) (string, error) {
	if err := config.revalidate(update.selection); err != nil {
		return "", err
	}
	release := update.selection.Release
	if err := verifyArchive(update.archive, release, config.Keys); err != nil {
		return "", err
	}
	parent := filepath.Dir(installed)
	backup, err := os.MkdirTemp(parent, ".sailry-update-backup-")
	if err != nil {
		return "", ioFailure(err)
	}
	if err := copyTree(installed, filepath.Join(backup, "Sailry.app")); err != nil {
		_ = os.RemoveAll(backup)
		return "", err
	}
	if err := swap(update, installed, parent); err != nil {
		return "", failure("updates_install_uncertain", fmt.Sprintf("application installation failed; original retained at %s: %v", backup, err))
	}
	return backup, nil
}

// swap extracts and validates the bundle on the installed volume, then
// exchanges the two by rename and restores the original if that fails.
func swap(update *staged, installed, parent string) error {
	work, err := os.MkdirTemp(parent, ".sailry-update-")
	if err != nil {
		return err
	}
	defer os.RemoveAll(work)
	bundle, err := extract(update.archive, update.selection.Release, work)
	if err != nil {
		return err
	}
	previous := filepath.Join(work, "previous")
	if err := os.Rename(installed, previous); err != nil {
		return err
	}
	if err := os.Rename(bundle, installed); err != nil {
		if restore := os.Rename(previous, installed); restore != nil {
			return fmt.Errorf("%w; the original could not be restored: %v", err, restore)
		}
		return err
	}
	return nil
}

func copyTree(source, destination string) error {
	info, err := os.Lstat(source)
	if err != nil {
		return ioFailure(err)
	}
	switch {
	case info.Mode()&os.ModeSymlink != 0:
		target, err := os.Readlink(source)
		if err != nil {
			return ioFailure(err)
		}
		if err := os.Symlink(target, destination); err != nil {
			return ioFailure(err)
		}
	case info.IsDir():
		if err := os.Mkdir(destination, 0o700); err != nil {
			return ioFailure(err)
		}
		entries, err := os.ReadDir(source)
		if err != nil {
			return ioFailure(err)
		}
		for _, entry := range entries {
			if err := copyTree(filepath.Join(source, entry.Name()), filepath.Join(destination, entry.Name())); err != nil {
				return err
			}
		}
		if err := os.Chmod(destination, info.Mode().Perm()); err != nil {
			return ioFailure(err)
		}
	case info.Mode().IsRegular():
		input, err := os.Open(source)
		if err != nil {
			return ioFailure(err)
		}
		defer input.Close()
		output, err := os.OpenFile(destination, os.O_WRONLY|os.O_CREATE|os.O_EXCL, info.Mode().Perm())
		if err != nil {
			return ioFailure(err)
		}
		_, err = io.Copy(output, input)
		if closeErr := output.Close(); err == nil {
			err = closeErr
		}
		if err != nil {
			return ioFailure(err)
		}
	default:
		return failure("updates_package_invalid", "the installed bundle contains a non-file resource")
	}
	return nil
}

// relaunch starts the installed executable with this process's arguments.
func relaunch(executable string, arguments []string) error {
	if err := exec.Command(executable, arguments...).Start(); err != nil {
		return failure("updates_restart_failed", err.Error())
	}
	return nil
}
