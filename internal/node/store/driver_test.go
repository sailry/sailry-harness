package store

import (
	"bytes"
	"context"
	"database/sql"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"runtime"
	"sync"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func databaseDirectory(t *testing.T) string {
	t.Helper()
	directory := filepath.Join(t.TempDir(), "storage")
	if err := core.PrivateDirectory(directory); err != nil {
		t.Fatal(err)
	}
	return directory
}

func assertPrivateDatabase(t *testing.T, path string) {
	t.Helper()
	info, err := os.Lstat(path)
	if err != nil || !info.Mode().IsRegular() {
		t.Fatalf("database is not a regular file: %v, %v", info, err)
	}
	if runtime.GOOS == "windows" {
		if err := validateDatabasePrivacy(path); err != nil {
			t.Fatalf("database storage is not private: %v", err)
		}
	} else if info.Mode().Perm() != 0o600 {
		t.Fatalf("database mode: %v", info)
	}
}

func driverFixture(t *testing.T) (*Handle, context.Context) {
	t.Helper()
	ctx := context.Background()
	handle, err := OpenHandle(ctx, filepath.Join(databaseDirectory(t), "node.sqlite"))
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() {
		if err := handle.Close(); err != nil {
			t.Error(err)
		}
	})
	if err := handle.Write(ctx, func(connection *sql.Conn) error {
		_, err := connection.ExecContext(ctx, "CREATE TABLE fixture(id INTEGER PRIMARY KEY,value INTEGER NOT NULL)")
		return err
	}); err != nil {
		t.Fatal(err)
	}
	return handle, ctx
}

func TestConcurrentWritersCommit(t *testing.T) {
	t.Parallel()
	handle, ctx := driverFixture(t)
	var workers sync.WaitGroup
	results := make(chan error, 20)
	for id := range 20 {
		workers.Add(1)
		go func() {
			defer workers.Done()
			results <- handle.Write(ctx, func(connection *sql.Conn) error {
				_, err := connection.ExecContext(ctx, "INSERT INTO fixture VALUES(?,?)", id, id)
				return err
			})
		}()
	}
	workers.Wait()
	close(results)
	for err := range results {
		if err != nil {
			t.Fatal(err)
		}
	}
	exclusive, err := handle.Exclusive(ctx)
	if err != nil {
		t.Fatal(err)
	}
	defer exclusive.Close()
	failed := errors.New("fixture callback rejected")
	job := func(callback func(*sql.Conn) error) *writeJob {
		return &writeJob{context: ctx, callback: callback, done: make(chan error, 1)}
	}
	first := job(func(connection *sql.Conn) error {
		_, err := connection.ExecContext(ctx, "INSERT INTO fixture VALUES(100,100)")
		return err
	})
	firstContext, cancelFirst := context.WithCancel(ctx)
	defer cancelFirst()
	first.context = firstContext
	rejected := job(func(connection *sql.Conn) error {
		if _, err := connection.ExecContext(ctx, "INSERT INTO fixture VALUES(101,101)"); err != nil {
			return err
		}
		return failed
	})
	last := job(func(connection *sql.Conn) error {
		for _, earlier := range []*writeJob{first, rejected} {
			select {
			case <-earlier.done:
				return errors.New("a job was answered before its batch committed")
			default:
			}
		}
		var committed int
		if err := handle.state.readDB.QueryRowContext(ctx, "SELECT count(*) FROM fixture").Scan(&committed); err != nil {
			return err
		}
		if committed != 20 {
			return fmt.Errorf("uncommitted batch became visible: %d records", committed)
		}
		cancelFirst()
		_, err := connection.ExecContext(ctx, "INSERT INTO fixture VALUES(102,102)")
		return err
	})
	batch := writer{connection: exclusive.Connection(), jobs: make(chan *writeJob, 2)}
	batch.jobs <- rejected
	batch.jobs <- last
	batch.batch(first)
	for _, item := range []struct {
		job  *writeJob
		want error
	}{{first, nil}, {rejected, failed}, {last, nil}} {
		if err := <-item.job.done; !errors.Is(err, item.want) {
			t.Fatalf("batch result: got %v, want %v", err, item.want)
		}
	}
	if !errors.Is(firstContext.Err(), context.Canceled) {
		t.Fatal("commit acknowledgement fixture did not cancel its admitted caller")
	}
	var count, rejectedCount int
	if err := handle.state.readDB.QueryRowContext(ctx, "SELECT count(*),sum(id=101) FROM fixture").Scan(&count, &rejectedCount); err != nil {
		t.Fatal(err)
	}
	if count != 22 || rejectedCount != 0 {
		t.Fatalf("savepoint isolation changed: count=%d rejected=%d", count, rejectedCount)
	}

	// SQLite ends the whole transaction when a write statement is interrupted.
	// The batch is then known to be uncommitted and the writer stays usable.
	earlier := job(func(connection *sql.Conn) error {
		_, err := connection.ExecContext(ctx, "INSERT INTO fixture VALUES(200,200)")
		return err
	})
	interrupted := job(nil)
	statement, cancelStatement := context.WithCancel(ctx)
	defer cancelStatement()
	interrupted.context = statement
	interrupted.callback = func(connection *sql.Conn) error {
		if _, err := connection.ExecContext(ctx, "INSERT INTO fixture VALUES(201,201)"); err != nil {
			return err
		}
		timer := time.AfterFunc(50*time.Millisecond, cancelStatement)
		defer timer.Stop()
		_, err := connection.ExecContext(statement, "WITH RECURSIVE spin(x) AS (SELECT 1 UNION ALL SELECT x+1 FROM spin WHERE x<4000000000) UPDATE fixture SET value=(SELECT count(*) FROM spin) WHERE id=201")
		return err
	}
	later := job(func(connection *sql.Conn) error {
		_, err := connection.ExecContext(ctx, "INSERT INTO fixture VALUES(202,202)")
		return err
	})
	batch.jobs <- interrupted
	batch.batch(earlier)
	var rolledBack *protocol.Fault
	if err := <-earlier.done; !errors.As(err, &rolledBack) || rolledBack.Code != protocol.ErrorCodeBusy {
		t.Fatalf("rolled-back job result: %v", err)
	}
	if err := <-interrupted.done; !errors.Is(err, context.Canceled) || errors.As(err, &rolledBack) && rolledBack.Code == protocol.ErrorCodeOutcomeUnknown {
		t.Fatalf("cancelled job result: %v", err)
	}
	if batch.failed != nil {
		t.Fatalf("a known rollback stopped the writer: %v", batch.failed)
	}
	batch.batch(later)
	if err := <-later.done; err != nil {
		t.Fatalf("write after a cancelled statement: %v", err)
	}
	if err := handle.state.readDB.QueryRowContext(ctx, "SELECT count(*),sum(id IN (200,201)) FROM fixture").Scan(&count, &rejectedCount); err != nil {
		t.Fatal(err)
	}
	if count != 23 || rejectedCount != 0 {
		t.Fatalf("interrupted batch: count=%d rolled back=%d", count, rejectedCount)
	}

	// A panicking callback fails alone: its savepoint is rolled back, the job
	// queued with it commits and the writer stays usable.
	panicking := job(func(connection *sql.Conn) error {
		if _, err := connection.ExecContext(ctx, "INSERT INTO fixture VALUES(300,300)"); err != nil {
			return err
		}
		var missing *writeJob
		return missing.result
	})
	sibling := job(func(connection *sql.Conn) error {
		_, err := connection.ExecContext(ctx, "INSERT INTO fixture VALUES(301,301)")
		return err
	})
	batch.jobs <- sibling
	batch.batch(panicking)
	var internal *protocol.Fault
	if err := <-panicking.done; !errors.As(err, &internal) || internal.Code != protocol.ErrorCodeInternal {
		t.Fatalf("panicking job result: %v", err)
	}
	if err := <-sibling.done; err != nil || batch.failed != nil {
		t.Fatalf("job after a panic: %v, writer: %v", err, batch.failed)
	}
	if err := handle.state.readDB.QueryRowContext(ctx, "SELECT count(*),sum(id=300) FROM fixture").Scan(&count, &rejectedCount); err != nil {
		t.Fatal(err)
	}
	if count != 24 || rejectedCount != 0 {
		t.Fatalf("panicking batch: count=%d kept=%d", count, rejectedCount)
	}
}

func TestReadKeepsSnapshot(t *testing.T) {
	t.Parallel()
	handle, ctx := driverFixture(t)
	if err := handle.Write(ctx, func(connection *sql.Conn) error {
		_, err := connection.ExecContext(ctx, "INSERT INTO fixture VALUES(1,0)")
		return err
	}); err != nil {
		t.Fatal(err)
	}
	pinned := make(chan struct{})
	written := make(chan error, 1)
	read := make(chan error, 1)
	go func() {
		read <- handle.Read(ctx, func(connection *sql.Conn) error {
			var before, after int
			if err := connection.QueryRowContext(ctx, "SELECT value FROM fixture WHERE id=1").Scan(&before); err != nil {
				return err
			}
			close(pinned)
			if err := <-written; err != nil {
				return err
			}
			if err := connection.QueryRowContext(ctx, "SELECT value FROM fixture WHERE id=1").Scan(&after); err != nil {
				return err
			}
			if before != 0 || after != 0 {
				return fmt.Errorf("read snapshot changed: %d to %d", before, after)
			}
			return nil
		})
	}()
	select {
	case <-pinned:
	case err := <-read:
		t.Fatalf("read failed before pinning: %v", err)
	}
	written <- handle.Write(ctx, func(connection *sql.Conn) error {
		_, err := connection.ExecContext(ctx, "UPDATE fixture SET value=1 WHERE id=1")
		return err
	})
	if err := <-read; err != nil {
		t.Fatal(err)
	}
	if err := handle.Read(ctx, func(connection *sql.Conn) error {
		var value int
		if err := connection.QueryRowContext(ctx, "SELECT value FROM fixture WHERE id=1").Scan(&value); err != nil {
			return err
		}
		if value != 1 {
			return fmt.Errorf("fresh snapshot contains %d", value)
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
}

func TestReadLeaseQueryOnly(t *testing.T) {
	t.Parallel()
	handle, ctx := driverFixture(t)
	err := handle.Read(ctx, func(connection *sql.Conn) error {
		for pragma, want := range map[string]int{"query_only": 1, "foreign_keys": 1, "synchronous": 2, "busy_timeout": 0} {
			var actual int
			if err := connection.QueryRowContext(ctx, "PRAGMA "+pragma).Scan(&actual); err != nil {
				return err
			}
			if actual != want {
				return fmt.Errorf("read lease %s=%d, want %d", pragma, actual, want)
			}
		}
		if _, err := connection.ExecContext(ctx, "INSERT INTO fixture VALUES(1,1)"); err == nil {
			return errors.New("read lease accepted a write")
		}
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
}

func TestPauseBlocksLeases(t *testing.T) {
	t.Parallel()
	handle, ctx := driverFixture(t)
	closed, err := handle.Pause(ctx)
	if err != nil {
		t.Fatal(err)
	}
	defer closed.Close()
	cancelled, cancel := context.WithCancel(ctx)
	read := make(chan error, 1)
	called := false
	go func() {
		read <- handle.Read(cancelled, func(*sql.Conn) error {
			called = true
			return nil
		})
	}()
	cancel()
	if err := <-read; !errors.Is(err, context.Canceled) || called {
		t.Fatalf("paused lease ran: called=%v error=%v", called, err)
	}
	if err := closed.Reopen(ctx); err != nil {
		t.Fatal(err)
	}
	if err := handle.Read(ctx, func(connection *sql.Conn) error {
		var count int
		return connection.QueryRowContext(ctx, "SELECT count(*) FROM fixture").Scan(&count)
	}); err != nil {
		t.Fatal(err)
	}
	stopped, err := handle.Pause(ctx)
	if err != nil {
		t.Fatal(err)
	}
	if err := stopped.Close(); err != nil {
		t.Fatal(err)
	}
	if err := handle.Read(ctx, func(*sql.Conn) error { return nil }); !errors.Is(err, core.ErrStopped) {
		t.Fatalf("stopped storage accepted a lease: %v", err)
	}
}

func TestOpenRejectsLinks(t *testing.T) {
	t.Parallel()
	t.Run("existing journal mode stays unchanged", func(t *testing.T) {
		path := filepath.Join(databaseDirectory(t), "other.sqlite")
		database, err := sql.Open("sqlite", path)
		if err != nil {
			t.Fatal(err)
		}
		if _, err := database.Exec("CREATE TABLE fixture(value INTEGER)"); err != nil {
			t.Fatal(err)
		}
		if err := database.Close(); err != nil {
			t.Fatal(err)
		}
		before, err := os.ReadFile(path)
		if err != nil {
			t.Fatal(err)
		}
		handle, err := OpenHandle(context.Background(), path)
		if handle != nil {
			_ = handle.Close()
		}
		if err == nil {
			t.Fatal("existing non-WAL database accepted")
		}
		after, err := os.ReadFile(path)
		if err != nil || !bytes.Equal(before, after) {
			t.Fatalf("rejected database changed: %v", err)
		}
	})
	for _, name := range []string{"", "-wal", "-shm"} {
		for _, kind := range []string{"symlink", "hardlink", "directory"} {
			t.Run(kind+name, func(t *testing.T) {
				t.Parallel()
				path := filepath.Join(databaseDirectory(t), "node.sqlite")
				target := filepath.Join(t.TempDir(), "target")
				if err := os.WriteFile(target, []byte("fixture"), 0o600); err != nil {
					t.Fatal(err)
				}
				var err error
				switch kind {
				case "symlink":
					err = os.Symlink(target, path+name)
				case "hardlink":
					err = os.Link(target, path+name)
				case "directory":
					err = os.Mkdir(path+name, 0o700)
				}
				if err != nil {
					if kind == "symlink" && runtime.GOOS == "windows" {
						t.Skipf("Windows symlink privilege is unavailable: %v", err)
					}
					t.Fatal(err)
				}
				handle, err := OpenHandle(context.Background(), path)
				if handle != nil {
					_ = handle.Close()
				}
				var invalid *core.InvalidProfileError
				if !errors.As(err, &invalid) {
					t.Fatalf("unsafe database file accepted: %v", err)
				}
			})
		}
	}
}
