package store

import (
	"context"
	"database/sql"
	"errors"

	"github.com/sailry/sailry-harness/internal/protocol"
)

type writeJob struct {
	context  context.Context
	callback func(*sql.Conn) error
	done     chan error
	result   error
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
				job.result = job.callback(writer.connection)
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
			job.result = nil
			break
		}
		select {
		case job = <-writer.jobs:
			continue
		default:
		}
		break
	}
	if failure == nil {
		_, failure = writer.connection.ExecContext(ctx, "COMMIT")
	}
	if failure != nil {
		if _, cleanup := writer.connection.ExecContext(ctx, "ROLLBACK"); cleanup != nil {
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
