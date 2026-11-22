//go:build !sailry_no_code_index

package codeindex

import (
	"context"
	"database/sql"
	"errors"

	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/turso"
)

// transientStorage pins one connection for a query's lifetime. Each memory
// connection has its own database, and no other goroutine uses this index.
type transientStorage struct {
	db   *sql.DB
	conn *sql.Conn
}

func newTransientIndex(ctx context.Context) (*index, error) {
	connector, err := turso.Connector(":memory:", false)
	if err != nil {
		return nil, failure(err)
	}
	db := sql.OpenDB(connector)
	conn, err := db.Conn(ctx)
	if err != nil {
		return nil, failure(errors.Join(err, db.Close()))
	}
	storage := &transientStorage{db: db, conn: conn}
	if err := storage.Write(ctx, func(conn *sql.Conn) error {
		_, err := conn.ExecContext(ctx, store.CodeSchema)
		return err
	}); err != nil {
		return nil, failure(errors.Join(err, storage.Close()))
	}
	return &index{handle: storage}, nil
}

func (s *transientStorage) Read(ctx context.Context, callback func(*sql.Conn) error) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	return callback(s.conn)
}

func (s *transientStorage) Write(ctx context.Context, callback func(*sql.Conn) error) (result error) {
	if _, err := s.conn.ExecContext(ctx, "BEGIN IMMEDIATE"); err != nil {
		return err
	}
	committed := false
	defer func() {
		if !committed {
			_, err := s.conn.ExecContext(context.WithoutCancel(ctx), "ROLLBACK")
			result = errors.Join(result, err)
		}
	}()
	if err := callback(s.conn); err != nil {
		return err
	}
	_, result = s.conn.ExecContext(ctx, "COMMIT")
	committed = result == nil
	return result
}

func (s *transientStorage) Close() error {
	return errors.Join(s.conn.Close(), s.db.Close())
}
