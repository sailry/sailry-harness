package store

import (
	"context"
	"errors"
	"os"
	"path/filepath"

	"github.com/sailry/sailry-harness/internal/core"
)

// Node uses the native database names shared with the Rust application. Preview
// SQLite files are unrelated; preserved native originals require explicit recovery.
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
			return "", incompatibleDatabase(path, "the existing Node database is empty; existing data was not changed")
		}
		return path, nil
	}
	if !errors.Is(err, os.ErrNotExist) {
		return "", err
	}
	if _, err := os.Lstat(filepath.Join(storage, "rust-archive", "node.turso")); err == nil {
		return "", incompatibleDatabase(path, "the original Turso database is preserved in storage/rust-archive; existing data was not changed")
	} else if !errors.Is(err, os.ErrNotExist) {
		return "", err
	}
	for _, member := range core.DatabaseFiles(path)[1:] {
		if _, err := os.Lstat(member); err == nil {
			return "", incompatibleDatabase(path, "the Node database is missing but its journal remains; existing data was not changed")
		} else if !errors.Is(err, os.ErrNotExist) {
			return "", err
		}
	}
	return path, nil
}
