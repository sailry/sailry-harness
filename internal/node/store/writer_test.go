package store

import (
	"context"
	"database/sql"
	"database/sql/driver"
	"errors"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
	"github.com/sailry/sailry-harness/internal/turso"
	sdk "turso.tech/database/tursogo"
)

type writerFaultConnector struct{ driver.Connector }

func (connector writerFaultConnector) Connect(ctx context.Context) (driver.Conn, error) {
	connection, err := connector.Connector.Connect(ctx)
	if err != nil {
		return nil, err
	}
	return &writerFaultConnection{Conn: connection}, nil
}

type writerFaultConnection struct {
	driver.Conn
	mode string
}

func (connection *writerFaultConnection) ExecContext(ctx context.Context, query string, arguments []driver.NamedValue) (driver.Result, error) {
	execer := connection.Conn.(driver.ExecerContext)
	command := strings.ToUpper(strings.TrimSpace(query))
	if connection.mode == "full storage" && strings.HasPrefix(command, "INSERT INTO FIXTURE") {
		return nil, sdk.ErrTursoDatabaseFull
	}
	if connection.mode == "rollback commit" && command == "COMMIT" {
		_, err := execer.ExecContext(ctx, "ROLLBACK", nil)
		return nil, errors.Join(errors.New("fixture commit failed after rollback"), err)
	}
	if connection.mode == "refuse transaction control" && (command == "COMMIT" || command == "ROLLBACK" || strings.HasPrefix(command, "BEGIN")) {
		return nil, errors.New("fixture transaction control unavailable")
	}
	return execer.ExecContext(ctx, query, arguments)
}

func (connection *writerFaultConnection) QueryContext(ctx context.Context, query string, arguments []driver.NamedValue) (driver.Rows, error) {
	return connection.Conn.(driver.QueryerContext).QueryContext(ctx, query, arguments)
}

func (connection *writerFaultConnection) PrepareContext(ctx context.Context, query string) (driver.Stmt, error) {
	return connection.Conn.(driver.ConnPrepareContext).PrepareContext(ctx, query)
}

func installWriterFaults(t *testing.T, handle *Handle) {
	t.Helper()
	connector, err := turso.Connector(handle.Path(), false)
	if err != nil {
		t.Fatal(err)
	}
	database := sql.OpenDB(writerFaultConnector{connector})
	database.SetMaxOpenConns(1)
	database.SetMaxIdleConns(1)
	connection, err := database.Conn(t.Context())
	if err != nil {
		_ = database.Close()
		t.Fatal(err)
	}
	exclusive, err := handle.Exclusive(t.Context())
	if err != nil {
		_ = connection.Close()
		_ = database.Close()
		t.Fatal(err)
	}
	previous, previousDB := handle.state.writer.connection, handle.state.writeDB
	handle.state.writeDB = database
	handle.state.writer.database = database
	handle.state.writer.connection = connection
	if err := errors.Join(previous.Close(), previousDB.Close(), exclusive.Close()); err != nil {
		t.Fatal(err)
	}
}

func setWriterFault(t *testing.T, connection *sql.Conn, mode string) error {
	t.Helper()
	return connection.Raw(func(raw any) error {
		raw.(*writerFaultConnection).mode = mode
		return nil
	})
}

func TestWriterAfterFailedCommit(t *testing.T) {
	t.Parallel()
	insert := func(ctx context.Context, handle *Handle, id int) error {
		return handle.Write(ctx, func(connection *sql.Conn) error {
			_, err := connection.ExecContext(ctx, "INSERT INTO fixture VALUES(?,?)", id, id)
			return err
		})
	}
	stored := func(t *testing.T, ctx context.Context, handle *Handle) (ids []int) {
		t.Helper()
		if err := handle.Read(ctx, func(connection *sql.Conn) error {
			rows, err := connection.QueryContext(ctx, "SELECT id FROM fixture ORDER BY id")
			if err != nil {
				return err
			}
			defer rows.Close()
			for rows.Next() {
				var id int
				if err := rows.Scan(&id); err != nil {
					return err
				}
				ids = append(ids, id)
			}
			return rows.Err()
		}); err != nil {
			t.Fatal(err)
		}
		return ids
	}
	retryable := func(t *testing.T, err error) {
		t.Helper()
		var fault *protocol.Fault
		if !errors.As(err, &fault) || fault.Code != protocol.ErrorCodeBusy {
			t.Fatalf("a commit that rolled back must be retryable: %v", err)
		}
	}

	t.Run("engine rolled the commit back", func(t *testing.T) {
		t.Parallel()
		handle, ctx := driverFixture(t)
		installWriterFaults(t, handle)
		err := handle.Write(ctx, func(connection *sql.Conn) error {
			if err := setWriterFault(t, connection, "rollback commit"); err != nil {
				return err
			}
			_, err := connection.ExecContext(ctx, "INSERT INTO fixture VALUES(1,1)")
			return err
		})
		retryable(t, err)
		if err := setWriterFault(t, handle.state.writer.connection, ""); err != nil {
			t.Fatal(err)
		}
		if err := insert(ctx, handle, 2); err != nil {
			t.Fatalf("write after a rolled-back commit: %v", err)
		}
		if ids := stored(t, ctx, handle); len(ids) != 1 || ids[0] != 2 {
			t.Fatalf("stored records: %v", ids)
		}
	})

	t.Run("commit refused with the transaction left open", func(t *testing.T) {
		t.Parallel()
		handle, ctx := driverFixture(t)
		if err := handle.Write(ctx, func(connection *sql.Conn) error {
			_, err := connection.ExecContext(ctx, "CREATE TABLE child(parent INTEGER NOT NULL REFERENCES fixture(id) DEFERRABLE INITIALLY DEFERRED)")
			return err
		}); err != nil {
			t.Fatal(err)
		}
		err := handle.Write(ctx, func(connection *sql.Conn) error {
			if _, err := connection.ExecContext(ctx, "INSERT INTO fixture VALUES(1,1)"); err != nil {
				return err
			}
			_, err := connection.ExecContext(ctx, "INSERT INTO child VALUES(9)")
			return err
		})
		retryable(t, err)
		if err := insert(ctx, handle, 2); err != nil {
			t.Fatalf("write after a refused commit: %v", err)
		}
		if ids := stored(t, ctx, handle); len(ids) != 1 || ids[0] != 2 {
			t.Fatalf("stored records: %v", ids)
		}
	})

	t.Run("connection lost before the commit", func(t *testing.T) {
		t.Parallel()
		handle, ctx := driverFixture(t)
		err := handle.Write(ctx, func(connection *sql.Conn) error {
			if _, err := connection.ExecContext(ctx, "INSERT INTO fixture VALUES(1,1)"); err != nil {
				return err
			}
			// database/sql discards the connection and every later statement
			// on it fails, ROLLBACK included.
			_ = connection.Raw(func(any) error { return driver.ErrBadConn })
			return nil
		})
		retryable(t, err)
		if err := insert(ctx, handle, 2); err != nil {
			t.Fatalf("write after the writer reopened its connection: %v", err)
		}
		if ids := stored(t, ctx, handle); len(ids) != 1 || ids[0] != 2 {
			t.Fatalf("stored records: %v", ids)
		}
		exclusive, err := handle.Exclusive(ctx)
		if err != nil {
			t.Fatal(err)
		}
		defer exclusive.Close()
		if _, err := exclusive.Connection().ExecContext(ctx, "INSERT INTO fixture VALUES(3,3)"); err != nil {
			t.Fatalf("exclusive owner must receive the reopened connection: %v", err)
		}
	})

	t.Run("commit failed and the connection cannot say why", func(t *testing.T) {
		t.Parallel()
		handle, ctx := driverFixture(t)
		installWriterFaults(t, handle)
		err := handle.Write(ctx, func(connection *sql.Conn) error {
			if _, err := connection.ExecContext(ctx, "INSERT INTO fixture VALUES(1,1)"); err != nil {
				return err
			}
			// COMMIT, ROLLBACK and BEGIN are all refused from here on.
			return setWriterFault(t, connection, "refuse transaction control")
		})
		var fault *protocol.Fault
		if !errors.As(err, &fault) || fault.Code != protocol.ErrorCodeOutcomeUnknown {
			t.Fatalf("a commit that could not be confirmed or undone: %v", err)
		}
		if err := insert(ctx, handle, 2); err != nil {
			t.Fatalf("an unknown outcome must not stop later writes: %v", err)
		}
		if ids := stored(t, ctx, handle); len(ids) != 1 || ids[0] != 2 {
			t.Fatalf("stored records: %v", ids)
		}
	})

	t.Run("full storage is named", func(t *testing.T) {
		t.Parallel()
		handle, ctx := driverFixture(t)
		installWriterFaults(t, handle)
		// MVCC does not allocate pages during the write, so max_page_count
		// cannot simulate a full disk. Inject the official error after a real
		// change and verify that no partial job survives its rollback.
		err := handle.Write(ctx, func(connection *sql.Conn) error {
			if _, err := connection.ExecContext(ctx, "INSERT INTO fixture VALUES(1,1)"); err != nil {
				return err
			}
			if err := setWriterFault(t, connection, "full storage"); err != nil {
				return err
			}
			_, err := connection.ExecContext(ctx, "INSERT INTO fixture VALUES(3,3)")
			return err
		})
		var fault *protocol.Fault
		if !errors.As(err, &fault) || fault.Code != protocol.ErrorCodeBusy || !strings.Contains(fault.Message, "storage is full") {
			t.Fatalf("full storage: %v", err)
		}
		if err := setWriterFault(t, handle.state.writer.connection, ""); err != nil {
			t.Fatal(err)
		}
		if err := insert(ctx, handle, 2); err != nil {
			t.Fatalf("write after full storage: %v", err)
		}
		if ids := stored(t, ctx, handle); len(ids) != 1 || ids[0] != 2 {
			t.Fatalf("partial job survived full storage: %v", ids)
		}
	})
}
