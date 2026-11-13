package databases

import (
	"errors"
	"io/fs"
	"os"
	"path/filepath"
	"strings"

	"github.com/sailry/sailry-harness/internal/protocol"
	"modernc.org/libc"
	sqlite3 "modernc.org/sqlite/lib"
)

// SQLite opens ATTACH and VACUUM INTO targets separately from the main database.
// Keep the existing Node storage exclusion on those opens too. VACUUM prepares
// its internal ATTACH at execution time, so the authorizer stays until close.
func (connection *sqliteConnection) protectStorage(storage string) error {
	if storage == "" {
		return nil
	}
	connection.storage = storage
	if code := sqlite3.Xsqlite3_set_authorizer(connection.tls, connection.db, cFuncPointer(sqliteAuthorize), connection.db); code != sqlite3.SQLITE_OK {
		return queryError(connection.errorMessage())
	}
	return nil
}

func sqliteAuthorize(_ *libc.TLS, db uintptr, action int32, filename, _, _, _ uintptr) int32 {
	if action != sqlite3.SQLITE_ATTACH {
		return sqlite3.SQLITE_OK
	}
	owner, ok := sqliteConnections.Load(db)
	if !ok {
		return sqlite3.SQLITE_DENY
	}
	connection := owner.(*sqliteConnection)
	if filename == 0 {
		// SQLite supplies no filename for an ATTACH expression. It cannot be
		// checked against the protected profile before that expression runs.
		connection.denied = fault(protocol.ErrorCodePermissionDenied, "SQLite ATTACH requires a literal filename")
	} else {
		name := libc.GoString(filename)
		if name == "" || name == ":memory:" {
			return sqlite3.SQLITE_OK
		}
		path, err := sqliteAttachedPath(name)
		if err == nil {
			err = checkStorage(path, connection.storage)
		}
		connection.denied = err
	}
	if connection.denied != nil {
		return sqlite3.SQLITE_DENY
	}
	return sqlite3.SQLITE_OK
}

// Resolve missing destinations without creating their parents. A dangling link
// is rejected rather than treated as a missing regular file outside the profile.
func sqliteAttachedPath(name string) (string, error) {
	// These connections leave SQLite URI handling disabled. Preserve dot
	// segments until symlinks are resolved, just as the filesystem does.
	path := filepath.FromSlash(name)
	if !filepath.IsAbs(path) {
		cwd, err := os.Getwd()
		if err != nil {
			return "", ioFault(err)
		}
		path = cwd + string(filepath.Separator) + path
	}
	var missing []string
	for {
		resolved, err := filepath.EvalSymlinks(path)
		if err == nil {
			return filepath.Join(append([]string{resolved}, missing...)...), nil
		}
		if !errors.Is(err, fs.ErrNotExist) {
			return "", ioFault(err)
		}
		if info, statErr := os.Lstat(path); statErr == nil && info.Mode()&os.ModeSymlink != 0 {
			return "", ioFault(err)
		}
		index := strings.LastIndexByte(path, byte(filepath.Separator))
		if index < 0 || index == len(path)-1 {
			return "", ioFault(err)
		}
		missing = append([]string{path[index+1:]}, missing...)
		path = path[:index+1]
		if index > 0 {
			path = path[:index]
		}
	}
}
