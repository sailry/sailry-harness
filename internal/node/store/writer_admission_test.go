package store

import (
	"context"
	"database/sql"
	"errors"
	"fmt"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
	turso "turso.tech/database/tursogo"
)

func TestWriteAdmission(t *testing.T) {
	t.Parallel()
	const effect = "INSERT INTO fixture VALUES(1,1)"
	busy := fmt.Errorf("%w: database is locked", turso.ErrTursoBusy)
	active := fmt.Errorf("%w: cannot start a statement - SQL statements in progress", turso.ErrTursoBusy)
	other := errors.New("fixture begin failed")
	cleanup := errors.New("fixture rollback failed")
	noTransaction := errors.New("cannot rollback - no transaction is active")
	reopen := errors.New("fixture connection unavailable")
	success := []readAdmissionStep{{query: "BEGIN IMMEDIATE"}, {query: "SAVEPOINT sailry_job"}, {query: effect}, {query: "RELEASE sailry_job"}, {query: "COMMIT"}}
	var exhausted []readAdmissionStep
	for range 9 {
		exhausted = append(exhausted, readAdmissionStep{query: "BEGIN IMMEDIATE", err: busy}, readAdmissionStep{query: "ROLLBACK"})
	}
	tests := []struct {
		name          string
		steps         []readAdmissionStep
		callbackError error
		reopenError   error
		wantErrors    []error
		wantCalls     int
		wantOpened    int
		wantDiscarded bool
	}{
		{name: "busy then success", steps: append([]readAdmissionStep{{query: "BEGIN IMMEDIATE", err: busy}, {query: "ROLLBACK"}}, success...), wantCalls: 1, wantOpened: 1},
		{name: "failed begin left no transaction", steps: append([]readAdmissionStep{{query: "BEGIN IMMEDIATE", err: busy}, {query: "ROLLBACK", err: noTransaction}, {query: "BEGIN"}, {query: "ROLLBACK"}}, success...), wantCalls: 1, wantOpened: 1},
		{name: "settlement failed", steps: []readAdmissionStep{{query: "BEGIN IMMEDIATE", err: busy}, {query: "ROLLBACK", err: cleanup}, {query: "BEGIN", err: cleanup}}, reopenError: reopen, wantErrors: []error{busy, cleanup}, wantOpened: 2, wantDiscarded: true},
		{name: "cancelled before retry", steps: []readAdmissionStep{{query: "BEGIN IMMEDIATE", err: busy}, {query: "ROLLBACK", cancel: true}}, wantErrors: []error{context.Canceled}, wantOpened: 1},
		{name: "other error", steps: []readAdmissionStep{{query: "BEGIN IMMEDIATE", err: other}, {query: "ROLLBACK"}}, wantErrors: []error{other}, wantOpened: 1},
		{name: "generic error", steps: []readAdmissionStep{{query: "BEGIN IMMEDIATE", err: turso.ErrTursoGeneric}, {query: "ROLLBACK"}}, wantErrors: []error{turso.ErrTursoGeneric}, wantOpened: 1},
		{name: "unfinished statement", steps: []readAdmissionStep{{query: "BEGIN IMMEDIATE", err: active}, {query: "ROLLBACK"}}, wantErrors: []error{active}, wantOpened: 1},
		{name: "callback is never retried", steps: []readAdmissionStep{{query: "BEGIN IMMEDIATE"}, {query: "SAVEPOINT sailry_job"}, {query: effect}, {query: "ROLLBACK TO sailry_job"}, {query: "RELEASE sailry_job"}, {query: "COMMIT"}}, callbackError: busy, wantErrors: []error{busy}, wantCalls: 1, wantOpened: 1},
		{name: "commit is never retried", steps: []readAdmissionStep{{query: "BEGIN IMMEDIATE"}, {query: "SAVEPOINT sailry_job"}, {query: effect}, {query: "RELEASE sailry_job"}, {query: "COMMIT", err: busy}, {query: "ROLLBACK"}}, wantErrors: []error{busy}, wantCalls: 1, wantOpened: 1},
		{name: "bounded retries", steps: exhausted, wantErrors: []error{busy}, wantOpened: 1},
	}
	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			t.Parallel()
			ctx, cancel := context.WithCancel(t.Context())
			defer cancel()
			script := &readAdmissionScript{t: t, steps: test.steps, cancel: cancel, reopenError: test.reopenError}
			database := sql.OpenDB(script)
			database.SetMaxOpenConns(1)
			t.Cleanup(func() { _ = database.Close() })
			connection, err := database.Conn(ctx)
			if err != nil {
				t.Fatal(err)
			}
			t.Cleanup(func() { _ = connection.Close() })
			writer := &writer{database: database, connection: connection}
			called := 0
			job := &writeJob{context: ctx, done: make(chan error, 1), callback: func(connection *sql.Conn) error {
				called++
				if _, err := connection.ExecContext(ctx, effect); err != nil {
					return err
				}
				return test.callbackError
			}}
			writer.batch(job)
			err = <-job.done
			if len(test.wantErrors) == 0 && err != nil {
				t.Fatalf("write: %v", err)
			}
			for _, want := range test.wantErrors {
				if !errors.Is(err, want) {
					t.Errorf("write error: got %v, want %v", err, want)
				}
			}
			if errors.Is(err, context.Canceled) {
				var fault *protocol.Fault
				if errors.As(err, &fault) {
					t.Errorf("cancelled admission returned a retryable fault: %v", err)
				}
			}
			if called != test.wantCalls || script.next != len(script.steps) {
				t.Errorf("callback calls=%d, want %d; SQL steps=%d, want %d", called, test.wantCalls, script.next, len(script.steps))
			}
			if script.closed != test.wantDiscarded || script.opened != test.wantOpened {
				t.Errorf("discarded=%t, want %t; connections=%d, want %d", script.closed, test.wantDiscarded, script.opened, test.wantOpened)
			}
			if !errors.Is(writer.failed, test.reopenError) {
				t.Errorf("reopen error: got %v, want %v", writer.failed, test.reopenError)
			}
		})
	}
}
