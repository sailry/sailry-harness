package store

import (
	"context"
	"errors"
	"os"
	"path/filepath"

	"github.com/sailry/sailry-harness/internal/core"
)

// Only the standard native database families are active. Preview SQLite files
// and historical archives are neither opened nor migrated.
func nodeDatabasePath(ctx context.Context, storage string) (string, error) {
	if err := ctx.Err(); err != nil {
		return "", err
	}
	path := filepath.Join(storage, "node.turso")
	info, err := os.Lstat(path)
	if err == nil {
		if !info.Mode().IsRegular() {
			return "", &core.InvalidProfileError{Path: path, Reason: "database must be a regular file"}
		}
		if err := checkDatabaseFiles(path); err != nil {
			return "", err
		}
		if err := validateDatabasePrivacy(path); err != nil {
			return "", err
		}
		if info.Size() == 0 {
			return "", incompatibleDatabase(path, "the existing Node database is empty")
		}
		return path, nil
	}
	if !errors.Is(err, os.ErrNotExist) {
		return "", err
	}
	remaining := append(core.DatabaseFiles(path)[1:], core.DatabaseFiles(filepath.Join(storage, "knowledge.turso"))...)
	for _, member := range remaining {
		if _, err := os.Lstat(member); err == nil {
			return "", incompatibleDatabase(path, "the Node database is missing but active storage still contains "+filepath.Base(member))
		} else if !errors.Is(err, os.ErrNotExist) {
			return "", err
		}
	}
	return path, nil
}
