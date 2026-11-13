package databases

import (
	"context"
	"errors"
	"fmt"
	"io/fs"
	"path/filepath"
	"strings"

	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/protocol"
	sqlite3 "modernc.org/sqlite/lib"
)

func runSQLite(ctx context.Context, operation Operation, requested string, query Query) (protocol.DatabaseResultSet, error) {
	var empty protocol.DatabaseResultSet
	if !filepath.IsAbs(requested) {
		return empty, invalid("Database path must be absolute")
	}
	storage := operation.Storage
	if resolved, err := filepath.EvalSymlinks(storage); storage != "" && err == nil {
		storage = resolved
	}
	create := false
	path, err := filepath.EvalSymlinks(requested)
	switch {
	case err == nil:
	case errors.Is(err, fs.ErrNotExist):
		// Only a connection check or a schema-changing statement may create a
		// missing database; a read or a read-only profile never does.
		if operation.Profile.ReadOnly {
			return empty, missing()
		}
		if operation.Query != nil {
			initializes, err := sqliteInitializes(query.SQL)
			if err != nil {
				return empty, err
			}
			if !initializes {
				return empty, missing()
			}
		}
		if path, err = sqliteCreationPath(requested, storage); err != nil {
			return empty, err
		}
		create = true
	default:
		return empty, ioFault(err)
	}
	if err := checkStorage(path, storage); err != nil {
		return empty, err
	}
	flags := int32(sqlite3.SQLITE_OPEN_FULLMUTEX)
	switch {
	case operation.Profile.ReadOnly:
		flags |= sqlite3.SQLITE_OPEN_READONLY
	case create:
		// SQLite itself refuses a symbolic link substituted after the checks.
		flags |= sqlite3.SQLITE_OPEN_READWRITE | sqlite3.SQLITE_OPEN_CREATE | sqlite3.SQLITE_OPEN_NOFOLLOW
	default:
		flags |= sqlite3.SQLITE_OPEN_READWRITE
	}
	connection, err := openSQLite(path, flags)
	if err != nil {
		return empty, err
	}
	defer connection.close()
	connection.busyTimeout(int32(min(query.TimeoutMS, 2000)))
	connection.watch(ctx)
	statement, write, err := connection.prepare(query.SQL, false)
	if err != nil {
		return empty, err
	}
	defer connection.finalize(statement)
	if write && operation.Profile.ReadOnly {
		return empty, fault(protocol.ErrorCodePermissionDenied, "Database profile is read-only")
	}
	result, err := connection.collect(statement, query.RowLimit)
	if err != nil && ctx.Err() != nil {
		return empty, unknown()
	}
	return result, err
}

// sqliteInitializes classifies the statement on an empty in-memory database.
// PRAGMA is denied there, and EXPLAIN never initializes a file.
func sqliteInitializes(sql string) (bool, error) {
	connection, err := openSQLite(":memory:", sqlite3.SQLITE_OPEN_READWRITE|sqlite3.SQLITE_OPEN_FULLMUTEX)
	if err != nil {
		return false, err
	}
	defer connection.close()
	if err := connection.denyPragma(true); err != nil {
		return false, err
	}
	statement, write, err := connection.prepare(sql, true)
	_ = connection.denyPragma(false)
	connection.finalize(statement)
	return write, err
}

// sqliteCreationPath resolves the nearest existing ancestor and creates the
// missing parents beneath it without following links.
func sqliteCreationPath(requested, storage string) (string, error) {
	for _, component := range strings.FieldsFunc(requested, func(character rune) bool {
		return character == '/' || character == filepath.Separator
	}) {
		if component == "." || component == ".." {
			return "", invalid("Database path must be normalized")
		}
	}
	parent := filepath.Dir(requested)
	if parent == requested {
		return "", invalid("Database filename is missing")
	}
	relative := []string{filepath.Base(requested)}
	var root string
	for {
		resolved, err := filepath.EvalSymlinks(parent)
		if err == nil {
			root = resolved
			break
		}
		if !errors.Is(err, fs.ErrNotExist) {
			return "", ioFault(err)
		}
		next := filepath.Dir(parent)
		if next == parent {
			return "", invalid("Database parent is missing")
		}
		relative = append([]string{filepath.Base(parent)}, relative...)
		parent = next
	}
	path := filepath.Join(append([]string{root}, relative...)...)
	if err := checkStorage(path, storage); err != nil {
		return "", err
	}
	if len(relative) == 1 {
		return path, nil
	}
	files, err := core.OpenFiles(root, core.RestrictedFiles)
	if err != nil {
		return "", err
	}
	defer files.Close()
	for depth := 1; depth < len(relative); depth++ {
		if err := files.CreateDirectory(strings.Join(relative[:depth], "/")); err != nil {
			return "", err
		}
	}
	return path, nil
}

func checkStorage(path, storage string) error {
	if storage == "" {
		return nil
	}
	relative, err := filepath.Rel(storage, path)
	if err != nil || relative == ".." || strings.HasPrefix(relative, ".."+string(filepath.Separator)) {
		return nil
	}
	return fault(protocol.ErrorCodePermissionDenied, "Node storage cannot be opened as a query database")
}

func missing() error {
	return fault(protocol.ErrorCodeNotFound, "Database file does not exist")
}

func ioFault(err error) error {
	code := protocol.ErrorCodeInvalidRequest
	if errors.Is(err, fs.ErrNotExist) {
		code = protocol.ErrorCodeNotFound
	} else if errors.Is(err, fs.ErrPermission) {
		code = protocol.ErrorCodePermissionDenied
	}
	return fault(code, fmt.Sprintf("worktree filesystem operation failed: %v", err))
}
