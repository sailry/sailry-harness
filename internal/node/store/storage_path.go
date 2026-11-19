package store

import (
	"context"
	"errors"
	"fmt"
	"os"
	"path/filepath"

	"github.com/sailry/sailry-harness/internal/core"
)

// CheckIdentityCreation requires a fresh database family before a missing
// link.key can be created. Call it while holding the profile lock.
func CheckIdentityCreation(ctx context.Context, profile string) error {
	path, exists, err := nodeDatabasePath(ctx, filepath.Join(profile, "storage"))
	if err != nil {
		return err
	}
	if exists {
		return identityDatabaseError(path, "link.key is missing")
	}
	return nil
}

func identityDatabaseError(path, reason string) error {
	return fmt.Errorf("Node database at %q: %s; restore the original link.key for this database or restore a complete profile backup", path, reason)
}

// Only the standard native database families are active. Preview SQLite files
// and historical archives are neither opened nor migrated.
func nodeDatabasePath(ctx context.Context, storage string) (string, bool, error) {
	if err := ctx.Err(); err != nil {
		return "", false, err
	}
	path := filepath.Join(storage, "node.turso")
	info, err := os.Lstat(path)
	if err == nil {
		if !info.Mode().IsRegular() {
			return "", false, &core.InvalidProfileError{Path: path, Reason: "database must be a regular file"}
		}
		if err := checkDatabaseFiles(path); err != nil {
			return "", false, err
		}
		if err := validateDatabasePrivacy(path); err != nil {
			return "", false, err
		}
		if info.Size() == 0 {
			return "", false, incompatibleDatabase(path, "the existing Node database is empty")
		}
		return path, true, nil
	}
	if !errors.Is(err, os.ErrNotExist) {
		return "", false, err
	}
	remaining := append(core.DatabaseFiles(path)[1:], core.DatabaseFiles(filepath.Join(storage, "knowledge.turso"))...)
	for _, member := range remaining {
		if _, err := os.Lstat(member); err == nil {
			return "", false, incompatibleDatabase(path, "the Node database is missing but active storage still contains "+filepath.Base(member))
		} else if !errors.Is(err, os.ErrNotExist) {
			return "", false, err
		}
	}
	return path, false, nil
}
