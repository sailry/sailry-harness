package store

import (
	"context"
	"database/sql"
	"errors"
	"fmt"
	"net/url"
	"os"
	"path/filepath"
	"strings"

	"github.com/sailry/sailry-harness/internal/core"
	"modernc.org/sqlite"
)

// An existing database keeps its name and journal mode. Only a profile without
// active storage creates the native default; preserved originals are not selected.
func nodeDatabasePath(ctx context.Context, storage string) (string, error) {
	for _, name := range []string{"node.turso", "node.sqlite"} {
		path := filepath.Join(storage, name)
		info, err := os.Lstat(path)
		if errors.Is(err, os.ErrNotExist) {
			continue
		}
		if err != nil {
			return "", err
		}
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
		if err := ctx.Err(); err != nil {
			return "", err
		}
		if name == "node.sqlite" {
			if err := inspectSQLiteNode(ctx, path); err != nil {
				return "", err
			}
		}
		return path, nil
	}
	path := filepath.Join(storage, "node.turso")
	for _, name := range []string{"node.sqlite3", "node.db"} {
		if _, err := os.Lstat(filepath.Join(storage, name)); err == nil {
			return "", incompatibleDatabase(path, "the profile contains an unsupported Node database; existing data was not changed")
		} else if !errors.Is(err, os.ErrNotExist) {
			return "", err
		}
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

// Read-only SQLite inspection can read committed schema from an existing WAL
// before native Turso opens it. It does not rewrite storage or run Node queries.
func inspectSQLiteNode(ctx context.Context, path string) error {
	shared := path + "-shm"
	if info, err := os.Lstat(shared); err == nil {
		if !info.Mode().IsRegular() {
			return &core.InvalidProfileError{Path: shared, Reason: "database coordination must be a regular file"}
		}
		if err := checkDatabaseLinks(shared, info); err != nil {
			return err
		}
	} else if !errors.Is(err, os.ErrNotExist) {
		return err
	}
	absolute, err := filepath.Abs(path)
	if err != nil {
		return err
	}
	uriPath := filepath.ToSlash(absolute)
	if !strings.HasPrefix(uriPath, "/") {
		uriPath = "/" + uriPath
	}
	uri := url.URL{Scheme: "file", Path: uriPath, RawQuery: "mode=ro"}
	connector, err := sqlite.NewConnector(uri.String())
	if err != nil {
		return err
	}
	database := sql.OpenDB(connector)
	defer database.Close()
	database.SetMaxOpenConns(1)
	// Native FTS declarations are not SQLite syntax. Read their metadata without
	// parsing those declarations; mode=ro still forbids every database write.
	if _, err := database.ExecContext(ctx, "PRAGMA writable_schema=1"); err != nil {
		return incompatibleDatabase(path, fmt.Sprintf("the existing SQLite schema could not be inspected: %v; existing data was not changed", err))
	}
	rows, err := database.QueryContext(ctx, "SELECT name,sql FROM sqlite_schema WHERE type='table' AND sql IS NOT NULL")
	if err != nil {
		return incompatibleDatabase(path, fmt.Sprintf("the existing SQLite schema could not be read: %v; existing data was not changed", err))
	}
	defer rows.Close()
	for rows.Next() {
		var name, declaration string
		if err := rows.Scan(&name, &declaration); err != nil {
			return err
		}
		if strings.HasPrefix(normalizeSQL(declaration), "CREATE VIRTUAL TABLE") {
			return incompatibleDatabase(path, fmt.Sprintf("SQLite virtual table %q is unsupported by native Turso; existing data was not changed", name))
		}
	}
	return rows.Err()
}
