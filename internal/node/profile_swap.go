package node

import (
	"context"
	"database/sql"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"time"

	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// CheckRestoredProfile reports whether the restored profile can take the
// place of current: it is another directory, outside current, no process
// owns it, and this build opens its identity and databases. The restore that
// made it has already decrypted and verified the archive. Nothing in either
// profile changes, so it runs while the Node of current does.
func CheckRestoredProfile(ctx context.Context, current, restored string) error {
	_, _, err := restoredProfile(ctx, current, restored)
	return err
}

// restoredProfile resolves both profiles and opens the restored one once.
func restoredProfile(ctx context.Context, current, restored string) (string, string, error) {
	refuse := func(reason string) (string, string, error) {
		return "", "", &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: reason}
	}
	if !filepath.IsAbs(current) || !filepath.IsAbs(restored) {
		return refuse("restored profile requires an absolute directory")
	}
	current, err := filepath.EvalSymlinks(current)
	if err != nil {
		return "", "", err
	}
	if restored, err = filepath.EvalSymlinks(restored); err != nil {
		return refuse("restored profile does not exist")
	}
	for _, pair := range [][2]string{{current, restored}, {restored, current}} {
		if relative, err := filepath.Rel(pair[0], pair[1]); err == nil && (relative == "." || filepath.IsLocal(relative)) {
			return refuse("restored profile overlaps the profile in use")
		}
	}
	if _, err := os.Lstat(filepath.Join(restored, "storage", "node.sqlite")); err != nil {
		return refuse("restored profile holds no Node database")
	}
	profile, err := OpenProfile(ctx, restored)
	if err != nil {
		return "", "", fmt.Errorf("restored profile cannot be opened: %w", err)
	}
	if err := profile.Close(); err != nil {
		return "", "", err
	}
	return current, restored, nil
}

// SwapProfile makes the restored profile the one at current. The profile
// that was there is kept beside it as "<name>.before-restore-<time>" and is
// never deleted; its path is returned. The Node of current must be stopped.
//
// Everything is checked before either profile is touched. The stored paths
// of the restored profile are then pointed at its new place, and three
// renames follow: the restored profile next to current, which proves both
// are on one volume, current to its kept name, the restored profile to
// current. A failure undoes what was done, so current is either what it was
// or the restored profile, and the restored profile is back where it lay.
func SwapProfile(ctx context.Context, current, restored string, now time.Time) (string, error) {
	current, restored, err := restoredProfile(ctx, current, restored)
	if err != nil {
		return "", err
	}
	// No process owns the profile that moves away.
	ownership, err := core.AcquireProfile(current)
	if err != nil {
		return "", err
	}
	if err := ownership.Close(); err != nil {
		return "", err
	}
	stamp := now.Format("20060102-150405")
	kept, err := unused(current + ".before-restore-" + stamp)
	if err != nil {
		return "", err
	}
	staged, err := unused(current + ".restored-" + stamp)
	if err != nil {
		return "", err
	}
	if err := relocateProfile(ctx, restored, restored, current); err != nil {
		return "", err
	}
	// undo returns the restored profile to its place with its own paths.
	undo := func(failure error) (string, error) {
		return "", errors.Join(failure, relocateProfile(ctx, restored, current, restored))
	}
	if err := os.Rename(restored, staged); err != nil {
		return undo(err)
	}
	if err := os.Rename(current, kept); err != nil {
		return undo(errors.Join(err, os.Rename(staged, restored)))
	}
	if err := os.Rename(staged, current); err != nil {
		return undo(errors.Join(err, os.Rename(kept, current), os.Rename(staged, restored)))
	}
	return kept, nil
}

// unused is path, or path with the first numbered suffix that names nothing.
func unused(path string) (string, error) {
	candidate := path
	for number := 2; ; number++ {
		if _, err := os.Lstat(candidate); errors.Is(err, os.ErrNotExist) {
			return candidate, nil
		} else if err != nil {
			return "", err
		}
		candidate = fmt.Sprintf("%s-%d", path, number)
	}
}

// relocateProfile rewrites the paths the profile at path stores beneath
// original so they lie beneath target.
func relocateProfile(ctx context.Context, path, original, target string) error {
	profile, err := OpenProfile(ctx, path)
	if err != nil {
		return err
	}
	err = profile.Store.Node.Write(ctx, func(db *sql.Conn) error {
		return store.RelocateProfile(ctx, db, original, target)
	})
	return errors.Join(err, profile.Close())
}
