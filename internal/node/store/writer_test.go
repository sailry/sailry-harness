package store

import (
	"context"
	"database/sql"
	"database/sql/driver"
	"errors"
	"fmt"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
	"modernc.org/sqlite"
)

// refuseCommits makes SQLite turn every COMMIT of the connection into a
// rollback, as it does itself when the commit cannot be written.
func refuseCommits(t *testing.T, connection *sql.Conn, refuse bool) error {
	t.Helper()
	return connection.Raw(func(raw any) error {
		var hook sqlite.CommitHookFn
		if refuse {
			hook = func() int32 { return 1 }
		}
		raw.(sqlite.HookRegisterer).RegisterCommitHook(hook)
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
			t.Fatalf("a commit SQLite rolled back must be retryable: %v", err)
		}
	}

	t.Run("SQLite rolled the commit back", func(t *testing.T) {
		t.Parallel()
		handle, ctx := driverFixture(t)
		err := handle.Write(ctx, func(connection *sql.Conn) error {
			if err := refuseCommits(t, connection, true); err != nil {
				return err
			}
			_, err := connection.ExecContext(ctx, "INSERT INTO fixture VALUES(1,1)")
			return err
		})
		retryable(t, err)
		if err := refuseCommits(t, handle.state.writer.connection, false); err != nil {
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
		err := handle.Write(ctx, func(connection *sql.Conn) error {
			if _, err := connection.ExecContext(ctx, "INSERT INTO fixture VALUES(1,1)"); err != nil {
				return err
			}
			// COMMIT, ROLLBACK and BEGIN are all refused from here on.
			return connection.Raw(func(raw any) error {
				return raw.(sqlite.AuthorizerRegisterer).RegisterAuthorizer(func(action sqlite.AuthorizerActionCode, _, _, _, _ string) sqlite.AuthorizerReturnCode {
					if action == sqlite.AuthTransaction {
						return sqlite.AuthorizerDeny
					}
					return sqlite.AuthorizerOK
				})
			})
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
		if err := handle.Write(ctx, func(connection *sql.Conn) error {
			var pages int
			if err := connection.QueryRowContext(ctx, "PRAGMA page_count").Scan(&pages); err != nil {
				return err
			}
			return connection.QueryRowContext(ctx, fmt.Sprintf("PRAGMA max_page_count=%d", pages+2)).Scan(&pages)
		}); err != nil {
			t.Fatal(err)
		}
		// The statement fails with SQLITE_FULL, and the job that preceded it in
		// the transaction is known to be uncommitted.
		err := handle.Write(ctx, func(connection *sql.Conn) error {
			_, err := connection.ExecContext(ctx, "CREATE TABLE filler(body BLOB); INSERT INTO filler VALUES(zeroblob(1048576))")
			return err
		})
		var fault *protocol.Fault
		if !errors.As(err, &fault) || fault.Code != protocol.ErrorCodeBusy || !strings.Contains(fault.Message, "storage is full") {
			t.Fatalf("full storage: %v", err)
		}
		if err := insert(ctx, handle, 2); err != nil {
			t.Fatalf("write after full storage: %v", err)
		}
	})
}
