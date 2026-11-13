package databases

// The pinned modernc database/sql driver cannot forward SQLITE_OPEN_NOFOLLOW,
// report a prepare tail, or expose sqlite3_stmt_readonly and
// sqlite3_stmt_isexplain. This executor uses the same module's public low-level
// package and owns one handle from open through close, so the statement that is
// classified is the statement that runs.

import (
	"bytes"
	"context"
	"encoding/binary"
	"fmt"
	"math"
	"math/bits"
	"strconv"
	"strings"
	"sync"
	"unicode/utf8"
	"unsafe"

	"github.com/sailry/sailry-harness/internal/protocol"
	"modernc.org/libc"
	_ "modernc.org/sqlite" // applies the pinned driver's process-wide SQLite patches
	sqlite3 "modernc.org/sqlite/lib"
)

const pointerBytes = bits.UintSize / 8

// sqliteConnections gives package-level SQLite callbacks their owning operation.
var sqliteConnections sync.Map // uintptr -> *sqliteConnection

type sqliteConnection struct {
	tls     *libc.TLS
	db      uintptr
	ctx     context.Context
	storage string
	denied  error
}

func openSQLite(path string, flags int32) (*sqliteConnection, error) {
	if strings.IndexByte(path, 0) >= 0 {
		return nil, invalid("Database path contains a NUL byte")
	}
	tls := libc.NewTLS()
	name, err := libc.CString(path)
	if err != nil {
		tls.Close()
		return nil, queryError(err)
	}
	slot := tls.Alloc(pointerBytes)
	code := sqlite3.Xsqlite3_open_v2(tls, name, slot, flags, 0)
	connection := &sqliteConnection{tls: tls, db: readPointer(slot)}
	tls.Free(pointerBytes)
	libc.Xfree(tls, name)
	if code != sqlite3.SQLITE_OK {
		// A failed open may still return a handle that carries the diagnostic.
		message := connection.databaseMessage(code)
		connection.close()
		return nil, queryError(message)
	}
	sqlite3.Xsqlite3_extended_result_codes(tls, connection.db, 1)
	sqliteConnections.Store(connection.db, connection)
	return connection, nil
}

func (connection *sqliteConnection) close() {
	if connection.db != 0 {
		sqlite3.Xsqlite3_progress_handler(connection.tls, connection.db, 0, 0, 0)
		sqliteConnections.Delete(connection.db)
		sqlite3.Xsqlite3_close_v2(connection.tls, connection.db)
		connection.db = 0
	}
	connection.tls.Close()
}

func (connection *sqliteConnection) busyTimeout(milliseconds int32) {
	sqlite3.Xsqlite3_busy_timeout(connection.tls, connection.db, milliseconds)
}

// watch interrupts running statements once ctx is cancelled or expires.
func (connection *sqliteConnection) watch(ctx context.Context) {
	connection.ctx = ctx
	sqlite3.Xsqlite3_progress_handler(connection.tls, connection.db, 1000, cFuncPointer(sqliteProgress), connection.db)
}

func (connection *sqliteConnection) denyPragma(deny bool) error {
	var callback uintptr
	if deny {
		callback = cFuncPointer(sqliteDenyPragma)
	}
	if code := sqlite3.Xsqlite3_set_authorizer(connection.tls, connection.db, callback, 0); code != sqlite3.SQLITE_OK {
		return queryError(connection.errorMessage())
	}
	return nil
}

// prepare compiles every statement in sql before any of it runs and returns
// the single statement with whether it writes. When classifying, EXPLAIN is
// not a write and a denied PRAGMA reports a missing database.
func (connection *sqliteConnection) prepare(sql string, classify bool) (uintptr, bool, error) {
	if index := strings.IndexByte(sql, 0); index >= 0 {
		return 0, false, queryError(fmt.Sprintf("nul byte found in provided data at position: %d", index))
	}
	text, err := libc.CString(sql)
	if err != nil {
		return 0, false, queryError(err)
	}
	defer libc.Xfree(connection.tls, text)
	slots := connection.tls.Alloc(2 * pointerBytes)
	defer connection.tls.Free(2 * pointerBytes)
	var first uintptr
	write := false
	for cursor := text; ; {
		code := sqlite3.Xsqlite3_prepare_v3(connection.tls, connection.db, cursor, -1, 0, slots, slots+pointerBytes)
		statement := readPointer(slots)
		if code != sqlite3.SQLITE_OK {
			var failure error
			if connection.denied != nil {
				failure = connection.denied
			} else if classify && code&0xff == sqlite3.SQLITE_AUTH {
				failure = missing()
			} else {
				failure = queryError(connection.errorMessage())
			}
			connection.finalize(first)
			return 0, false, failure
		}
		if statement != 0 {
			if first != 0 {
				connection.finalize(statement)
				connection.finalize(first)
				return 0, false, queryError("Run one SQL statement at a time")
			}
			first = statement
			write = sqlite3.Xsqlite3_stmt_readonly(connection.tls, statement) == 0 &&
				(!classify || sqlite3.Xsqlite3_stmt_isexplain(connection.tls, statement) == 0)
		}
		tail := readPointer(slots + pointerBytes)
		if tail == 0 || libc.GoBytes(tail, 1)[0] == 0 {
			break
		}
		cursor = tail
	}
	if first == 0 {
		return 0, false, queryError("SQL statement is empty")
	}
	return first, write, nil
}

func (connection *sqliteConnection) finalize(statement uintptr) {
	if statement != 0 {
		sqlite3.Xsqlite3_finalize(connection.tls, statement)
	}
}

// collect steps the prepared statement to completion.
func (connection *sqliteConnection) collect(statement uintptr, limit uint32) (protocol.DatabaseResultSet, error) {
	var empty protocol.DatabaseResultSet
	columns := make([]string, sqlite3.Xsqlite3_column_count(connection.tls, statement))
	for index := range columns {
		columns[index] = libc.GoString(sqlite3.Xsqlite3_column_name(connection.tls, statement, int32(index)))
	}
	result, err := newRows(columns, limit)
	if err != nil {
		return empty, err
	}
	for {
		switch code := sqlite3.Xsqlite3_step(connection.tls, statement); code {
		case sqlite3.SQLITE_ROW:
			if !result.retain() {
				continue
			}
			row := make([]protocol.DatabaseValue, len(columns))
			for index := range row {
				row[index] = connection.value(statement, int32(index))
			}
			if err := result.push(row); err != nil {
				return empty, err
			}
		case sqlite3.SQLITE_DONE:
			if len(columns) == 0 {
				result.result.AffectedRows = uint64(sqlite3.Xsqlite3_changes64(connection.tls, connection.db))
			}
			return result.result, nil
		default:
			if connection.denied != nil {
				return empty, connection.denied
			}
			// An interrupted statement may already have changed the database.
			if sqlite3.Xsqlite3_extended_errcode(connection.tls, connection.db) == sqlite3.SQLITE_INTERRUPT {
				return empty, unknown()
			}
			return empty, queryError(connection.databaseMessage(code))
		}
	}
}

func (connection *sqliteConnection) value(statement uintptr, column int32) protocol.DatabaseValue {
	switch sqlite3.Xsqlite3_column_type(connection.tls, statement, column) {
	case sqlite3.SQLITE_NULL:
		return nullValue()
	case sqlite3.SQLITE_INTEGER:
		return integerValue(sqlite3.Xsqlite3_column_int64(connection.tls, statement, column))
	case sqlite3.SQLITE_FLOAT:
		return realValue(sqliteReal(sqlite3.Xsqlite3_column_double(connection.tls, statement, column)))
	case sqlite3.SQLITE_BLOB:
		data := sqlite3.Xsqlite3_column_blob(connection.tls, statement, column)
		return blobValue(copyBytes(data, sqlite3.Xsqlite3_column_bytes(connection.tls, statement, column)))
	default:
		data := sqlite3.Xsqlite3_column_text(connection.tls, statement, column)
		text := copyBytes(data, sqlite3.Xsqlite3_column_bytes(connection.tls, statement, column))
		if utf8.Valid(text) {
			return textValue(string(text))
		}
		return textValue(string(bytes.Runes(text)))
	}
}

func (connection *sqliteConnection) errorMessage() string {
	return libc.GoString(sqlite3.Xsqlite3_errmsg(connection.tls, connection.db))
}

func (connection *sqliteConnection) databaseMessage(code int32) string {
	if connection.db == 0 {
		return fmt.Sprintf("error returned from database: (code: %d) %s", code, libc.GoString(sqlite3.Xsqlite3_errstr(connection.tls, code)))
	}
	return fmt.Sprintf("error returned from database: (code: %d) %s",
		sqlite3.Xsqlite3_extended_errcode(connection.tls, connection.db), connection.errorMessage())
}

// sqliteReal prints the shortest decimal without an exponent.
func sqliteReal(value float64) string {
	switch {
	case math.IsInf(value, 1):
		return "inf"
	case math.IsInf(value, -1):
		return "-inf"
	}
	return strconv.FormatFloat(value, 'f', -1, 64)
}

func sqliteProgress(_ *libc.TLS, db uintptr) int32 {
	if owner, ok := sqliteConnections.Load(db); ok && owner.(*sqliteConnection).ctx.Err() == nil {
		return 0
	}
	return 1
}

func sqliteDenyPragma(_ *libc.TLS, _ uintptr, action int32, _, _, _, _ uintptr) int32 {
	if action == sqlite3.SQLITE_PRAGMA {
		return sqlite3.SQLITE_DENY
	}
	return sqlite3.SQLITE_OK
}

// cFuncPointer converts a function declared at package level to the function
// pointer representation the transpiled SQLite calls, exactly as the pinned
// driver does for its own callbacks. It must never receive a closure.
func cFuncPointer[T any](function T) uintptr {
	return *(*uintptr)(unsafe.Pointer(&struct{ function T }{function}))
}

func readPointer(address uintptr) uintptr {
	raw := libc.GoBytes(address, pointerBytes)
	if pointerBytes == 8 {
		return uintptr(binary.NativeEndian.Uint64(raw))
	}
	return uintptr(binary.NativeEndian.Uint32(raw))
}

func copyBytes(address uintptr, length int32) []byte {
	if address == 0 || length <= 0 {
		return []byte{}
	}
	return bytes.Clone(libc.GoBytes(address, int(length)))
}
