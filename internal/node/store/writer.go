package store

import (
	"context"
	"database/sql"
	"errors"
	"fmt"
	"log/slog"
	"runtime/debug"

	"github.com/sailry/sailry-harness/internal/protocol"
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
	connection *sql.Conn
	jobs       chan *writeJob
	stop       chan struct{}
	done       chan struct{}
	failed     error
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
// busy fault for work another job's failure rolled back; or, only when COMMIT
// itself failed and could not be undone, an unknown outcome that stops the writer.
func (writer *writer) batch(first *writeJob) {
	if writer.failed != nil {
		first.done <- writer.failed
		return
	}
	ctx := context.WithoutCancel(first.context)
	if _, err := writer.connection.ExecContext(ctx, "BEGIN IMMEDIATE"); err != nil {
		first.done <- err
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
		_, cleanup := writer.connection.ExecContext(ctx, "ROLLBACK")
		switch {
		case !committing:
			// No commit was attempted, so none of this batch is durable. An
			// interrupted or failed write statement makes SQLite roll back the
			// whole transaction; its savepoint is then gone and this ROLLBACK
			// only reports that no transaction is active. Callbacks are not
			// replayed, so the jobs that had succeeded are asked to retry.
			failure = errors.Join(&protocol.Fault{Code: protocol.ErrorCodeBusy, Message: "storage transaction was rolled back; retry the operation"}, failure)
		case cleanup != nil:
			writer.failed = &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "storage commit could not be confirmed"}
			failure = errors.Join(writer.failed, failure, cleanup)
		}
	}
	for _, job := range completed {
		if job.result == nil {
			job.result = failure
		}
		job.done <- job.result
	}
}
