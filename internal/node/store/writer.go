package store

import (
	"context"
	"database/sql"
	"database/sql/driver"
	"errors"
	"fmt"
	"log/slog"
	"runtime/debug"

	"github.com/sailry/sailry-harness/internal/protocol"
	"modernc.org/sqlite"
)

type writeJob struct {
	context  context.Context
	callback func(*sql.Conn) error
	done     chan error
	result   error
}

// run reports a panicking callback as an internal fault, so that the batch
// rolls its savepoint back and the writer, and with it the Node, continues.
func (job *writeJob) run(connection *sql.Conn) (err error) {
	defer func() {
		if value := recover(); value != nil {
			slog.Error("storage write panicked", "panic", fmt.Sprint(value), "stack", string(debug.Stack()))
			err = &protocol.Fault{Code: protocol.ErrorCodeInternal, Message: "storage write failed unexpectedly"}
		}
	}()
	return job.callback(connection)
}

type writer struct {
	database   *sql.DB
	connection *sql.Conn
	jobs       chan *writeJob
	stop       chan struct{}
	done       chan struct{}
	// failed is set while the connection could not be replaced after its
	// transaction state became unknown; the next batch tries again.
	failed error
}

// Write submits related SQL without external effects or transaction controls.
// An admitted callback is not replayed, and its caller waits for the commit result.
// A callback returns its first SQL error: SQLite ends the transaction itself
// when a write statement is interrupted or cannot complete.
func (handle *Handle) Write(ctx context.Context, callback func(*sql.Conn) error) error {
	state, err := handle.acquire(ctx)
	if err != nil {
		return err
	}
	defer handle.leases.Release(1)
	job := &writeJob{context: ctx, callback: callback, done: make(chan error, 1)}
	select {
	case state.writer.jobs <- job:
		return <-job.done
	case <-ctx.Done():
		return ctx.Err()
	}
}

func (writer *writer) run() {
	defer close(writer.done)
	for {
		select {
		case job := <-writer.jobs:
			writer.batch(job)
		case <-writer.stop:
			return
		}
	}
}

// batch commits the queued jobs together. Its jobs end in one of three ways:
// committed; known to be uncommitted, with the job's own error or a retryable
// busy fault for work another job's failure or a refused COMMIT rolled back;
// or, only when COMMIT failed and the connection can no longer say whether a
// transaction is open, an unknown outcome. The writer then replaces its
// connection, so later batches are not affected.
func (writer *writer) batch(first *writeJob) {
	ctx := context.WithoutCancel(first.context)
	if writer.failed != nil {
		if writer.failed = writer.reopen(ctx); writer.failed != nil {
			first.done <- errors.Join(&protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "storage connection could not be reopened; restart Sailry if this persists"}, writer.failed)
			return
		}
	}
	if _, err := writer.connection.ExecContext(ctx, "BEGIN IMMEDIATE"); err != nil {
		// Nothing ran. A connection that cannot begin is replaced.
		if writer.settle(ctx) != nil {
			writer.failed = writer.reopen(ctx)
		}
		first.done <- uncommitted(err)
		return
	}
	var completed []*writeJob
	var failure error
	job := first
	for {
		job.result = job.context.Err()
		if job.result == nil {
			_, failure = writer.connection.ExecContext(ctx, "SAVEPOINT sailry_job")
			if failure == nil {
				job.result = job.run(writer.connection)
				if job.result == nil {
					job.result = job.context.Err()
				}
				if job.result != nil {
					job.result = storageFull(job.result)
					_, failure = writer.connection.ExecContext(ctx, "ROLLBACK TO sailry_job")
				}
				if failure == nil {
					_, failure = writer.connection.ExecContext(ctx, "RELEASE sailry_job")
				}
			}
		}
		completed = append(completed, job)
		if failure != nil {
			break
		}
		select {
		case job = <-writer.jobs:
			continue
		default:
		}
		break
	}
	committing := failure == nil
	if committing {
		_, failure = writer.connection.ExecContext(ctx, "COMMIT")
	}
	if failure != nil {
		// Callbacks are not replayed, so the jobs that had succeeded are asked
		// to retry whenever the batch is known to be uncommitted.
		state := writer.settle(ctx)
		switch {
		case state == nil:
			failure = uncommitted(failure)
		case committing:
			failure = errors.Join(&protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "storage commit could not be confirmed; inspect the target before retrying"}, failure, state)
		default:
			// No commit was attempted, and closing the connection discards
			// the transaction it may still hold.
			failure = uncommitted(errors.Join(failure, state))
		}
		if state != nil {
			slog.Error("storage connection state is unknown; reopening it", "error", state.Error())
			writer.failed = writer.reopen(ctx)
		}
	}
	for _, job := range completed {
		if job.result == nil {
			job.result = failure
		}
		job.done <- job.result
	}
}

// settle leaves the connection outside any transaction, or reports that it
// cannot tell. ROLLBACK ends a transaction that is still open. SQLite ends the
// transaction itself when a write statement or a COMMIT cannot complete, and
// ROLLBACK then fails only because none is active; an empty transaction that
// begins and ends proves that state instead of reading it from the error.
func (writer *writer) settle(ctx context.Context) error {
	_, rollback := writer.connection.ExecContext(ctx, "ROLLBACK")
	if rollback == nil {
		return nil
	}
	if _, err := writer.connection.ExecContext(ctx, "BEGIN"); err != nil {
		return errors.Join(rollback, err)
	}
	_, err := writer.connection.ExecContext(ctx, "ROLLBACK")
	return err
}

// reopen replaces the connection. Closing the old one discards a transaction
// it still holds; the new one must be able to take the write lock.
func (writer *writer) reopen(ctx context.Context) error {
	discard := func(connection *sql.Conn) {
		_ = connection.Raw(func(any) error { return driver.ErrBadConn })
		_ = connection.Close()
	}
	discard(writer.connection)
	connection, err := writer.database.Conn(ctx)
	if err != nil {
		return err
	}
	if _, err = connection.ExecContext(ctx, "BEGIN IMMEDIATE"); err == nil {
		_, err = connection.ExecContext(ctx, "ROLLBACK")
	}
	if err != nil {
		discard(connection)
		return err
	}
	writer.connection = connection
	return nil
}

// uncommitted is the result of work that is known not to be stored.
func uncommitted(cause error) error {
	if full := storageFull(cause); full != cause {
		return full
	}
	return errors.Join(&protocol.Fault{Code: protocol.ErrorCodeBusy, Message: "storage transaction was rolled back; retry the operation"}, cause)
}

// storageFull names the disk when SQLite could not store a change for lack of
// space. SQLite has rolled the transaction back by then, so retrying is safe.
func storageFull(cause error) error {
	var failure *sqlite.Error
	if !errors.As(cause, &failure) || failure.Code()&0xff != sqliteFull {
		return cause
	}
	return errors.Join(&protocol.Fault{Code: protocol.ErrorCodeBusy, Message: "storage is full; free disk space and retry the operation"}, cause)
}

// sqliteFull is SQLITE_FULL, the primary result code of a full disk or database.
const sqliteFull = 13
