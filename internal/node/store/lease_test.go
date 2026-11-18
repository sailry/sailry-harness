package store

import (
	"context"
	"database/sql"
	"database/sql/driver"
	"errors"
	"fmt"
	"io"
	"testing"
	"testing/synctest"
	"time"

	"golang.org/x/sync/semaphore"
	turso "turso.tech/database/tursogo"
)

type readAdmissionStep struct {
	query  string
	err    error
	rowErr error
	cancel bool
}

type readAdmissionScript struct {
	t           *testing.T
	steps       []readAdmissionStep
	next        int
	opened      int
	closed      bool
	rowsOpened  int
	rowsClosed  int
	cancel      context.CancelFunc
	reopenError error
}

func (script *readAdmissionScript) Driver() driver.Driver { return script }
func (script *readAdmissionScript) Open(string) (driver.Conn, error) {
	return script.Connect(context.Background())
}
func (script *readAdmissionScript) Connect(context.Context) (driver.Conn, error) {
	script.opened++
	if script.opened > 1 && script.reopenError != nil {
		return nil, script.reopenError
	}
	return script, nil
}
func (script *readAdmissionScript) Close() error {
	script.closed = true
	return nil
}
func (script *readAdmissionScript) Prepare(string) (driver.Stmt, error) {
	return nil, errors.New("unexpected prepare")
}
func (script *readAdmissionScript) Begin() (driver.Tx, error) {
	return nil, errors.New("unexpected Begin")
}

func (script *readAdmissionScript) take(ctx context.Context, query string) (readAdmissionStep, error) {
	script.t.Helper()
	if err := ctx.Err(); err != nil {
		return readAdmissionStep{}, err
	}
	if script.next >= len(script.steps) {
		script.t.Errorf("unexpected SQL after script ended: %s", query)
		return readAdmissionStep{}, errors.New("script exhausted")
	}
	step := script.steps[script.next]
	script.next++
	if query != step.query {
		script.t.Errorf("SQL step %d: got %q, want %q", script.next, query, step.query)
		return step, errors.New("unexpected SQL")
	}
	if step.cancel {
		script.cancel()
	}
	return step, nil
}

func (script *readAdmissionScript) ExecContext(ctx context.Context, query string, _ []driver.NamedValue) (driver.Result, error) {
	step, err := script.take(ctx, query)
	if err != nil {
		return nil, err
	}
	return driver.RowsAffected(0), step.err
}

func (script *readAdmissionScript) QueryContext(ctx context.Context, query string, _ []driver.NamedValue) (driver.Rows, error) {
	step, err := script.take(ctx, query)
	if err != nil {
		return nil, err
	}
	if step.err != nil {
		return nil, step.err
	}
	script.rowsOpened++
	return &readAdmissionRows{script: script, err: step.rowErr}, nil
}

type readAdmissionRows struct {
	script  *readAdmissionScript
	err     error
	yielded bool
	closed  bool
}

func (rows *readAdmissionRows) Columns() []string { return []string{"rootpage"} }
func (rows *readAdmissionRows) Close() error {
	if !rows.closed {
		rows.closed = true
		rows.script.rowsClosed++
	}
	return nil
}
func (rows *readAdmissionRows) Next(values []driver.Value) error {
	if rows.err != nil {
		return rows.err
	}
	if rows.yielded {
		return io.EOF
	}
	rows.yielded = true
	values[0] = int64(1)
	return nil
}

func TestReadAdmissionWaitsForWriterCommit(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		const pin = "SELECT rootpage FROM sqlite_schema LIMIT 1"
		busy := fmt.Errorf("%w: database is locked", turso.ErrTursoBusy)
		script := &readAdmissionScript{t: t, steps: []readAdmissionStep{
			{query: "BEGIN"}, {query: pin, rowErr: busy}, {query: "ROLLBACK"},
			{query: "BEGIN"}, {query: pin}, {query: "COMMIT"},
		}}
		database := sql.OpenDB(script)
		defer database.Close()
		done := make(chan struct{})
		writer := new(writer)
		writer.commitDone.Store(&done)
		handle := &Handle{leases: semaphore.NewWeighted(allLeases), state: &connections{readDB: database, writer: writer}}
		ctx, cancel := context.WithCancel(t.Context())
		defer cancel()
		result := make(chan error, 1)
		calls := 0
		go func() {
			result <- handle.Read(ctx, func(*sql.Conn) error {
				calls++
				return nil
			})
		}()
		// A checkpoint can outlast every fallback delay. The failed snapshot
		// must roll back before waiting, and application code runs on success.
		time.Sleep(time.Second)
		synctest.Wait()
		if script.next != 3 || calls != 0 {
			t.Fatalf("read retried before writer commit ended: %d SQL steps, %d callbacks", script.next, calls)
		}
		close(done)
		if err := <-result; err != nil {
			t.Fatal(err)
		}
		if script.next != len(script.steps) || calls != 1 {
			t.Fatalf("read did not complete after writer commit: %d SQL steps, %d callbacks", script.next, calls)
		}
	})
}

func TestReadAdmission(t *testing.T) {
	t.Parallel()
	const pin = "SELECT rootpage FROM sqlite_schema LIMIT 1"
	busy := fmt.Errorf("%w: database is locked", turso.ErrTursoBusy)
	snapshot := fmt.Errorf("%w: database snapshot is stale, rollback and retry the transaction", turso.ErrTursoGeneric)
	dependency := fmt.Errorf("%w: Commit dependency aborted, rollback and retry the whole transaction", turso.ErrTursoGeneric)
	active := fmt.Errorf("%w: cannot start a read transaction - SQL statements in progress", turso.ErrTursoBusy)
	other := errors.New("fixture read failed")
	cleanup := errors.New("fixture rollback failed")
	success := []readAdmissionStep{{query: "BEGIN"}, {query: pin}, {query: "COMMIT"}}
	var exhausted []readAdmissionStep
	for range 9 {
		exhausted = append(exhausted, readAdmissionStep{query: "BEGIN"}, readAdmissionStep{query: pin, rowErr: busy}, readAdmissionStep{query: "ROLLBACK"})
	}
	tests := []struct {
		name          string
		steps         []readAdmissionStep
		callbackError error
		wantErrors    []error
		wantCalls     int
		wantDiscarded bool
	}{
		{name: "busy begin with confirmed rollback", steps: append([]readAdmissionStep{{query: "BEGIN", err: busy}, {query: "ROLLBACK"}}, success...), wantCalls: 1},
		{name: "busy prepare", steps: append([]readAdmissionStep{{query: "BEGIN"}, {query: pin, err: busy}, {query: "ROLLBACK"}}, success...), wantCalls: 1},
		{name: "busy first table read", steps: append([]readAdmissionStep{{query: "BEGIN"}, {query: pin, rowErr: busy}, {query: "ROLLBACK"}}, success...), wantCalls: 1},
		{name: "busy snapshot", steps: append([]readAdmissionStep{{query: "BEGIN"}, {query: pin, rowErr: snapshot}, {query: "ROLLBACK"}}, success...), wantCalls: 1},
		{name: "aborted snapshot dependency", steps: append([]readAdmissionStep{{query: "BEGIN"}, {query: pin, rowErr: dependency}, {query: "ROLLBACK"}}, success...), wantCalls: 1},
		{name: "rollback failed", steps: []readAdmissionStep{{query: "BEGIN"}, {query: pin, rowErr: busy}, {query: "ROLLBACK", err: cleanup}}, wantErrors: []error{busy, cleanup}, wantDiscarded: true},
		{name: "other error", steps: []readAdmissionStep{{query: "BEGIN"}, {query: pin, rowErr: other}, {query: "ROLLBACK"}}, wantErrors: []error{other}},
		{name: "busy with another error", steps: []readAdmissionStep{{query: "BEGIN"}, {query: pin, rowErr: errors.Join(busy, other)}, {query: "ROLLBACK"}}, wantErrors: []error{busy, other}},
		{name: "generic error", steps: []readAdmissionStep{{query: "BEGIN"}, {query: pin, rowErr: turso.ErrTursoGeneric}, {query: "ROLLBACK"}}, wantErrors: []error{turso.ErrTursoGeneric}},
		{name: "unfinished statement", steps: []readAdmissionStep{{query: "BEGIN"}, {query: pin, rowErr: active}, {query: "ROLLBACK"}}, wantErrors: []error{active}},
		{name: "cancelled before retry", steps: []readAdmissionStep{{query: "BEGIN"}, {query: pin, rowErr: busy}, {query: "ROLLBACK", cancel: true}}, wantErrors: []error{context.Canceled}},
		{name: "callback is never retried", steps: []readAdmissionStep{{query: "BEGIN"}, {query: pin}, {query: "ROLLBACK"}}, callbackError: busy, wantErrors: []error{busy}, wantCalls: 1},
		{name: "commit is never retried", steps: []readAdmissionStep{{query: "BEGIN"}, {query: pin}, {query: "COMMIT", err: busy}, {query: "ROLLBACK"}}, wantErrors: []error{busy}, wantCalls: 1},
		{name: "bounded retries", steps: exhausted, wantErrors: []error{busy}},
	}
	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			t.Parallel()
			ctx, cancel := context.WithCancel(t.Context())
			defer cancel()
			script := &readAdmissionScript{t: t, steps: test.steps, cancel: cancel}
			database := sql.OpenDB(script)
			t.Cleanup(func() { _ = database.Close() })
			handle := &Handle{leases: semaphore.NewWeighted(allLeases), state: &connections{readDB: database, writer: new(writer)}}
			called := 0
			err := handle.Read(ctx, func(*sql.Conn) error {
				called++
				return test.callbackError
			})
			if len(test.wantErrors) == 0 && err != nil {
				t.Fatalf("read: %v", err)
			}
			for _, want := range test.wantErrors {
				if !errors.Is(err, want) {
					t.Errorf("read error: got %v, want %v", err, want)
				}
			}
			if called != test.wantCalls || script.next != len(script.steps) {
				t.Errorf("callback calls=%d, want %d; SQL steps=%d, want %d", called, test.wantCalls, script.next, len(script.steps))
			}
			if script.closed != test.wantDiscarded || script.opened != 1 {
				t.Errorf("discarded=%t, want %t; connections=%d, want 1", script.closed, test.wantDiscarded, script.opened)
			}
			if script.rowsOpened != script.rowsClosed {
				t.Errorf("rows opened=%d, closed=%d", script.rowsOpened, script.rowsClosed)
			}
		})
	}
}
