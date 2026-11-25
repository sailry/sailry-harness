package store

import (
	"context"
	"database/sql"
	"database/sql/driver"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"testing"

	"github.com/sailry/sailry-harness/internal/turso"
)

type journalCancellationConnector struct {
	driver.Connector
	before func(context.Context, string)
}

func (connector journalCancellationConnector) Connect(ctx context.Context) (driver.Conn, error) {
	connection, err := connector.Connector.Connect(ctx)
	if err != nil {
		return nil, err
	}
	return &journalCancellationConnection{Conn: connection, before: connector.before}, nil
}

type journalCancellationConnection struct {
	driver.Conn
	before func(context.Context, string)
}

func (connection *journalCancellationConnection) QueryContext(ctx context.Context, query string, arguments []driver.NamedValue) (driver.Rows, error) {
	connection.before(ctx, query)
	return connection.Conn.(driver.QueryerContext).QueryContext(ctx, query, arguments)
}

func TestJournalInitializationCancellation(t *testing.T) {
	t.Parallel()
	for _, item := range []struct {
		name        string
		initialized bool
		mode        string
	}{
		{"new database", false, "mvcc"},
		{"existing database", true, "wal"},
	} {
		t.Run(item.name, func(t *testing.T) {
			t.Parallel()
			path := filepath.Join(databaseDirectory(t), "fixture.turso")
			connector, err := turso.Connector(path, false)
			if err != nil {
				t.Fatal(err)
			}
			ctx, cancel := context.WithCancel(t.Context())
			defer cancel()
			var operation context.Context
			database := sql.OpenDB(journalCancellationConnector{Connector: connector, before: func(current context.Context, query string) {
				operation = current
				want := "PRAGMA journal_mode"
				if !item.initialized {
					want += "=mvcc"
				}
				if query != want {
					t.Fatalf("journal query %q, want %q", query, want)
				}
				cancel()
			}})
			defer database.Close()
			connection, err := database.Conn(t.Context())
			if err != nil {
				t.Fatal(err)
			}
			defer connection.Close()
			if item.initialized {
				if _, err := connection.ExecContext(t.Context(), "CREATE TABLE preserved(value INTEGER); INSERT INTO preserved VALUES(42)"); err != nil {
					t.Fatal(err)
				}
			}
			mode, err := initializeJournalMode(ctx, connection, item.initialized)
			if !errors.Is(err, context.Canceled) || operation == nil {
				t.Fatalf("cancelled open: mode=%q, operation=%v, error=%v", mode, operation, err)
			}
			if item.initialized {
				if !errors.Is(operation.Err(), context.Canceled) {
					t.Fatalf("reading an existing journal ignored cancellation: %v", operation.Err())
				}
			} else if mode != item.mode || operation.Err() != nil || operation.Done() != nil {
				t.Fatalf("new journal transition was interrupted: mode=%q, error=%v", mode, operation.Err())
			}
			if err := errors.Join(connection.Close(), database.Close()); err != nil {
				t.Fatal(err)
			}
			handle, err := OpenHandle(t.Context(), path)
			if err != nil {
				t.Fatal(err)
			}
			defer handle.Close()
			if err := handle.Write(t.Context(), func(connection *sql.Conn) error {
				var mode string
				if err := connection.QueryRowContext(t.Context(), "PRAGMA journal_mode").Scan(&mode); err != nil {
					return err
				}
				if mode != item.mode {
					return fmt.Errorf("reopened journal %q, want %q", mode, item.mode)
				}
				if item.initialized {
					var preserved int
					if err := connection.QueryRowContext(t.Context(), "SELECT value FROM preserved").Scan(&preserved); err != nil {
						return err
					}
					if preserved != 42 {
						return fmt.Errorf("existing database stored %d, want 42", preserved)
					}
				}
				_, err := connection.ExecContext(t.Context(), "CREATE TABLE fixture(value INTEGER); INSERT INTO fixture VALUES(42)")
				return err
			}); err != nil {
				t.Fatal(err)
			}
			if err := handle.Read(t.Context(), func(connection *sql.Conn) error {
				var value int
				if err := connection.QueryRowContext(t.Context(), "SELECT value FROM fixture").Scan(&value); err != nil {
					return err
				}
				if value != 42 {
					return fmt.Errorf("reopened database stored %d, want 42", value)
				}
				return nil
			}); err != nil {
				t.Fatal(err)
			}
		})
	}
}

func TestCancelledOpenDoesNotCreateDatabase(t *testing.T) {
	t.Parallel()
	path := filepath.Join(databaseDirectory(t), "cancelled.turso")
	ctx, cancel := context.WithCancel(t.Context())
	cancel()
	handle, err := OpenHandle(ctx, path)
	if handle != nil {
		_ = handle.Close()
	}
	if handle != nil || !errors.Is(err, context.Canceled) {
		t.Fatalf("already cancelled open: handle=%v, error=%v", handle, err)
	}
	if _, err := os.Stat(path); !errors.Is(err, os.ErrNotExist) {
		t.Fatalf("already cancelled open created a database: %v", err)
	}
}
