package store

import (
	"context"
	"database/sql"
	"errors"
	"os"
	"slices"

	"github.com/sailry/sailry-harness/internal/core"
)

const codeIndexVersion = 2

var errCodeIndexVersion = errors.New("unexpected derived index format")

// OpenCodeIndex opens one worktree's derived cache. The worktree owns the
// opening lock, so a recognized legacy cache can be rebuilt with no handles
// open and no other readers using its files.
func OpenCodeIndex(ctx context.Context, path string) (*Handle, error) {
	handle, err := OpenHandle(ctx, path)
	if err != nil {
		return nil, err
	}
	var rebuild bool
	err = handle.Write(ctx, func(conn *sql.Conn) error {
		objects, err := readSchemaObjects(ctx, conn)
		if err != nil {
			return errors.Join(errCodeIndexVersion, err)
		}
		if len(objects) == 0 {
			_, err := conn.ExecContext(ctx, CodeSchema)
			return err
		}
		var version int
		if err := conn.QueryRowContext(ctx, "SELECT version FROM metadata WHERE singleton=1").Scan(&version); err != nil {
			return errors.Join(errCodeIndexVersion, err)
		}
		switch version {
		case 1:
			legacy, err := knownLegacyCodeIndex(ctx, conn, objects)
			if err != nil {
				return err
			}
			if !legacy {
				return errCodeIndexVersion
			}
			rebuild = true
		case codeIndexVersion:
		default:
			return errCodeIndexVersion
		}
		return nil
	})
	if err != nil {
		return nil, errors.Join(err, handle.Close())
	}
	if !rebuild {
		return handle, nil
	}
	if err := handle.Close(); err != nil {
		return nil, err
	}
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	path = handle.Path()
	if err := checkDatabaseFiles(path); err != nil {
		return nil, err
	}
	// Remove sidecars first so an interrupted rebuild can still recognize the
	// closed legacy database on the next open.
	for _, member := range slices.Backward(core.DatabaseFiles(path)) {
		if err := os.Remove(member); err != nil && !errors.Is(err, os.ErrNotExist) {
			return nil, err
		}
	}
	return OpenCodeIndex(ctx, path)
}

func knownLegacyCodeIndex(ctx context.Context, conn *sql.Conn, objects schemaObjects) (bool, error) {
	var application int
	if err := conn.QueryRowContext(ctx, "PRAGMA application_id").Scan(&application); err != nil || application != 0 {
		return false, err
	}
	for object := range objects {
		switch object {
		case schemaObject{"table", "metadata"}, schemaObject{"table", "files"}, schemaObject{"table", "symbols"},
			schemaObject{"table", "refs"}, schemaObject{"table", "imports"}, schemaObject{"table", "__turso_internal_fts_dir_symbols_fts"},
			schemaObject{"index", "symbols_path"}, schemaObject{"index", "symbols_name"}, schemaObject{"index", "symbols_language"},
			schemaObject{"index", "symbols_fts"}, schemaObject{"index", "__turso_internal_fts_dir_symbols_fts_key"},
			schemaObject{"index", "refs_path"}, schemaObject{"index", "refs_name"}, schemaObject{"index", "imports_path"}:
		default:
			return false, nil
		}
	}
	var legacyShape bool
	err := conn.QueryRowContext(ctx, `SELECT
  (SELECT count(*)=3 AND count(CASE WHEN name IN ('singleton','version','revision') THEN 1 END)=3 FROM pragma_table_info('metadata')) AND
  (SELECT count(*)=4 AND count(CASE WHEN (name='path' AND pk=1) OR name IN ('fingerprint','hash','state') THEN 1 END)=4 FROM pragma_table_info('files')) AND
  (SELECT count(*)=9 AND count(CASE WHEN name IN ('id','path','name','language','body','field0','field1','cjk0','cjk1') THEN 1 END)=9 FROM pragma_table_info('symbols')) AND
  (SELECT count(*)=12 AND count(CASE WHEN name IN ('id','path','name','kind','start_line','start_column','start_byte','receiver','scope','scope_kind','scope_line','scope_end_line') THEN 1 END)=12 FROM pragma_table_info('refs')) AND
  (SELECT count(*)=2 AND count(CASE WHEN name IN ('path','module') THEN 1 END)=2 FROM pragma_table_info('imports'))`).Scan(&legacyShape)
	return legacyShape, err
}
