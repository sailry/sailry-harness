package store

import (
	"context"
	"database/sql"
	"errors"
	"testing"
)

func TestPinnedReadIsolation(t *testing.T) {
	t.Parallel()
	handle, ctx := driverFixture(t)
	insert := func(from, to int) error {
		return handle.Write(ctx, func(connection *sql.Conn) error {
			for id := from; id < to; id++ {
				if _, err := connection.ExecContext(ctx, "INSERT INTO fixture VALUES(?,?)", id, id); err != nil {
					return err
				}
			}
			return nil
		})
	}
	if err := insert(0, 100); err != nil {
		t.Fatal(err)
	}
	exclusive, err := handle.Exclusive(ctx)
	if err != nil {
		t.Fatal(err)
	}
	defer exclusive.Close()
	pinned, err := exclusive.Pin(ctx)
	if err != nil {
		t.Fatal(err)
	}
	defer pinned.Close()
	if _, err := exclusive.Pause(ctx); err == nil {
		t.Fatal("a database with a pinned read transaction was closed")
	}
	if err := exclusive.Close(); err != nil {
		t.Fatal(err)
	}
	if err := insert(100, 150); err != nil {
		t.Fatal(err)
	}
	var rows int
	if err := handle.Read(ctx, func(connection *sql.Conn) error {
		return connection.QueryRowContext(ctx, "SELECT count(*) FROM fixture").Scan(&rows)
	}); err != nil || rows != 150 {
		t.Fatalf("live rows = %d, %v", rows, err)
	}
	if err := pinned.Connection().QueryRowContext(ctx, "SELECT count(*) FROM fixture").Scan(&rows); err != nil || rows != 100 {
		t.Fatalf("pinned rows = %d, %v", rows, err)
	}
	cancelled, cancel := context.WithCancel(ctx)
	cancel()
	if _, err := handle.Exclusive(cancelled); !errors.Is(err, context.Canceled) {
		t.Fatalf("exclusive ownership did not wait for the pinned reader: %v", err)
	}
	if err := pinned.Close(); err != nil {
		t.Fatal(err)
	}
	if err := pinned.Close(); err != nil {
		t.Fatal(err)
	}
	again, err := handle.Exclusive(ctx)
	if err != nil {
		t.Fatal(err)
	}
	if err := again.Close(); err != nil {
		t.Fatal(err)
	}
}
