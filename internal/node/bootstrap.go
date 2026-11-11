package node

import (
	"context"
	"errors"
	"log/slog"
	"os"
	"path/filepath"
	"strings"

	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/store"
)

// Profile holds the shared Desktop and Host storage lifecycle.
type Profile struct {
	Ownership *core.Profile
	Identity  link.Identity
	Store     *store.Store
}

func OpenProfile(ctx context.Context, path string) (*Profile, error) {
	ownership, err := core.AcquireProfile(path)
	if err != nil {
		return nil, err
	}
	sweepProfile(ownership.Path)
	identity, err := link.OpenIdentity(ownership.Path)
	if err != nil {
		_ = ownership.Close()
		return nil, err
	}
	database, err := store.Open(ctx, ownership.Path, identity.ID())
	if err != nil {
		_ = ownership.Close()
		return nil, err
	}
	return &Profile{Ownership: ownership, Identity: identity, Store: database}, nil
}

func (profile *Profile) Close() error {
	return errors.Join(profile.Store.Close(), profile.Ownership.Close())
}

// Abandon releases the profile after a shutdown that passed its deadline. A
// database that a worker still reads or writes is left open: the process is
// about to exit and SQLite recovers its log at the next start.
func (profile *Profile) Abandon() error {
	return errors.Join(profile.Store.CloseIdle(), profile.Ownership.Close())
}

// processGroupPrefix names the records of running command groups in the
// profile's tmp (process/groups_unix.go). The process service reads them at
// its start to kill the groups a crashed Node left, then removes them.
const processGroupPrefix = "command-group-"

// sweepProfile removes what an earlier run left in the profile's scratch
// space: everything in tmp, which holds database copies and unpacked
// archives while a backup or restore runs, the scratch directories of a
// schema check or update beside the databases, and the staging directories of
// a plugin package or Python environment that was never published. It runs with the profile lock
// held and before anything of this process uses those directories, so
// nothing it removes is in use. A leftover that cannot be removed is logged
// and does not stop the start.
func sweepProfile(profile string) {
	storage := filepath.Join(profile, "storage")
	sweep(filepath.Join(profile, "tmp"), func(entry os.DirEntry) bool {
		return !entry.Type().IsRegular() || !strings.HasPrefix(entry.Name(), processGroupPrefix)
	})
	sweep(storage, func(entry os.DirEntry) bool {
		return entry.IsDir() && (strings.HasPrefix(entry.Name(), ".upgrade-") || strings.HasPrefix(entry.Name(), ".schema-reference-"))
	})
	// The kept pre-update copies beside them are node-before-upgrade-*.
	sweep(filepath.Join(storage, "backups"), func(entry os.DirEntry) bool {
		return entry.IsDir() && strings.HasPrefix(entry.Name(), ".upgrade-")
	})
	// An installation that was killed before it published its directory.
	sweep(filepath.Join(profile, "plugins", "packages"), func(entry os.DirEntry) bool {
		return entry.IsDir() && strings.HasPrefix(entry.Name(), ".staging-")
	})
	sweep(filepath.Join(profile, "python", "staging"), func(entry os.DirEntry) bool {
		return entry.IsDir() && strings.HasPrefix(entry.Name(), "environment-")
	})
}

// sweep removes the entries of directory that leftover selects. A link is
// removed, never followed.
func sweep(directory string, leftover func(os.DirEntry) bool) {
	root, err := os.OpenRoot(directory)
	if err != nil {
		if !errors.Is(err, os.ErrNotExist) {
			slog.Warn("Profile leftovers could not be read", "path", directory, "error", err)
		}
		return
	}
	defer root.Close()
	entries, err := os.ReadDir(directory)
	if err != nil {
		slog.Warn("Profile leftovers could not be read", "path", directory, "error", err)
		return
	}
	for _, entry := range entries {
		if !leftover(entry) {
			continue
		}
		err := root.RemoveAll(entry.Name())
		if err != nil && entry.IsDir() {
			// Staged inputs are read-only trees.
			unlock(root, entry.Name())
			err = root.RemoveAll(entry.Name())
		}
		if err != nil {
			slog.Warn("Profile leftover could not be removed", "path", filepath.Join(directory, entry.Name()), "error", err)
		}
	}
}

// unlock makes a directory tree removable by its owner.
func unlock(root *os.Root, name string) {
	if root.Chmod(name, 0o700) != nil {
		return
	}
	directory, err := root.Open(name)
	if err != nil {
		return
	}
	entries, _ := directory.ReadDir(-1)
	_ = directory.Close()
	for _, entry := range entries {
		if entry.IsDir() {
			unlock(root, filepath.Join(name, entry.Name()))
		}
	}
}
