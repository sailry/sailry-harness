package store

import (
	"bytes"
	"context"
	"database/sql"
	"errors"
	"log"
	"log/slog"
	"maps"
	"path/filepath"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/logging"
	"github.com/sailry/sailry-harness/internal/protocol"
	"golang.org/x/sync/semaphore"
)

func writerFailureFixture(t *testing.T, script *readAdmissionScript) (*Handle, func() []logging.Fields) {
	t.Helper()
	var fallback bytes.Buffer
	previous := slog.Default()
	output, flags := log.Writer(), log.Flags()
	slog.SetDefault(slog.New(slog.NewTextHandler(&fallback, nil)))
	t.Cleanup(func() {
		slog.SetDefault(previous)
		log.SetOutput(output)
		log.SetFlags(flags)
	})
	stop, err := logging.Install(filepath.Join(t.TempDir(), "profile"), "host")
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(stop)
	var reports []logging.Fields
	logging.SetReport(func(level slog.Level, fields logging.Fields) {
		if level != slog.LevelError {
			t.Errorf("writer failure report level: %v", level)
		}
		if level >= slog.LevelWarn {
			reports = append(reports, fields)
		}
	})
	t.Cleanup(func() { logging.SetReport(nil) })

	db := sql.OpenDB(script)
	db.SetMaxOpenConns(1)
	conn, err := db.Conn(t.Context())
	if err != nil {
		_ = db.Close()
		t.Fatal(err)
	}
	w := &writer{database: db, connection: conn, jobs: make(chan *writeJob), stop: make(chan struct{}), done: make(chan struct{})}
	handle := &Handle{leases: semaphore.NewWeighted(allLeases), state: &connections{writer: w, writeDB: db, readDB: sql.OpenDB(script)}}
	go w.run()
	t.Cleanup(func() {
		if err := handle.Close(); err != nil {
			t.Error(err)
		}
		if fallback.Len() != 0 {
			t.Error("writer bypassed structured logging")
		}
		if script.next != len(script.steps) {
			t.Errorf("SQL steps executed: got %d, want %d", script.next, len(script.steps))
		}
	})
	return handle, func() []logging.Fields { return reports }
}

func TestWriterPanicStaysWithCaller(t *testing.T) {
	const effect = "INSERT INTO fixture VALUES(1,1)"
	handle, reports := writerFailureFixture(t, &readAdmissionScript{t: t, steps: []readAdmissionStep{
		{query: "BEGIN IMMEDIATE"}, {query: "SAVEPOINT sailry_job"}, {query: effect},
		{query: "ROLLBACK TO sailry_job"}, {query: "RELEASE sailry_job"}, {query: "COMMIT"},
		{query: "BEGIN IMMEDIATE"}, {query: "SAVEPOINT sailry_job"}, {query: "RELEASE sailry_job"}, {query: "COMMIT"},
	}})
	err := handle.Write(t.Context(), func(conn *sql.Conn) error {
		if _, err := conn.ExecContext(t.Context(), effect); err != nil {
			return err
		}
		panic("private panic payload")
	})
	if fault, ok := errors.AsType[*protocol.Fault](err); !ok || fault.Code != protocol.ErrorCodeInternal || fault.Message != "storage write failed unexpectedly" {
		t.Fatalf("panicking callback did not return a safe fault: %v", err)
	}
	if err := handle.Write(t.Context(), func(*sql.Conn) error { return nil }); err != nil {
		t.Fatalf("writer did not recover after rolling back the callback: %v", err)
	}
	if got := reports(); len(got) != 0 {
		t.Fatalf("writer also reported the caller-owned fault: %v", got)
	}
}

func TestWriterReportsFailedReopenOnce(t *testing.T) {
	const private = "private connection failure"
	for _, tc := range []struct {
		name       string
		beginFails bool
		reopenErr  error
	}{
		{name: "recovers"},
		{name: "failed recovery after unknown commit", reopenErr: errors.New(private)},
		{name: "failed recovery during admission", beginFails: true, reopenErr: errors.New(private)},
		{name: "cancelled recovery", reopenErr: context.Canceled},
	} {
		t.Run(tc.name, func(t *testing.T) {
			failure := errors.New("fixture transaction control failed")
			steps := []readAdmissionStep{{query: "BEGIN IMMEDIATE"}, {query: "SAVEPOINT sailry_job"}, {query: "RELEASE sailry_job"}, {query: "COMMIT", err: failure}}
			wantCode := protocol.ErrorCodeOutcomeUnknown
			if tc.beginFails {
				steps = []readAdmissionStep{{query: "BEGIN IMMEDIATE", err: failure}}
				wantCode = protocol.ErrorCodeBusy
			}
			steps = append(steps, readAdmissionStep{query: "ROLLBACK", err: failure}, readAdmissionStep{query: "BEGIN", err: failure})
			if tc.reopenErr == nil {
				steps = append(steps, readAdmissionStep{query: "PRAGMA journal_mode"}, readAdmissionStep{query: "BEGIN IMMEDIATE"}, readAdmissionStep{query: "ROLLBACK"})
			}
			script := &readAdmissionScript{t: t, steps: steps, reopenError: tc.reopenErr}
			handle, reports := writerFailureFixture(t, script)
			ctx := t.Context()
			err := handle.Write(ctx, func(*sql.Conn) error { return nil })
			if fault, ok := errors.AsType[*protocol.Fault](err); !ok || fault.Code != wantCode || !errors.Is(err, failure) {
				t.Fatalf("write lost its original transaction result: %v", err)
			}
			if script.opened != 2 || !script.closed {
				t.Fatalf("writer did not discard and attempt to replace its connection: opened=%d, discarded=%t", script.opened, script.closed)
			}
			if tc.reopenErr != nil {
				cancelled, cancel := context.WithCancel(ctx)
				cancel()
				if err := handle.Write(cancelled, func(*sql.Conn) error { return errors.New("cancelled callback ran") }); !errors.Is(err, context.Canceled) {
					t.Fatalf("cancelled write: %v", err)
				}
				if script.opened != 2 {
					t.Fatalf("cancelled write tried to reopen: %d connections", script.opened)
				}
				if exclusive, err := handle.Exclusive(ctx); err == nil {
					_ = exclusive.Close()
					t.Fatal("exclusive maintenance received a missing writer connection")
				}
				for range 2 {
					err := handle.Write(ctx, func(*sql.Conn) error { return errors.New("callback ran without a writer") })
					if fault, ok := errors.AsType[*protocol.Fault](err); !ok || fault.Code != protocol.ErrorCodeUnavailable || strings.Contains(err.Error(), private) {
						t.Fatalf("write after reopen failure did not return a safe unavailable fault: %v", err)
					}
				}
			}
			if err := handle.Close(); err != nil {
				t.Fatalf("close after connection recovery: %v", err)
			}
			got := reports()
			if tc.reopenErr == nil || errors.Is(tc.reopenErr, context.Canceled) {
				if len(got) != 0 {
					t.Fatalf("unexpected writer reports: %v", got)
				}
				return
			}
			want := logging.Fields{"area": "storage", "operation": "reopen_writer", "phase": logging.Failed, "code": "unavailable"}
			if len(got) != 1 || !maps.Equal(got[0], want) || !maps.Equal(logging.Remote(got[0]), want) {
				t.Fatalf("writer reports: got %v, want one %v", got, want)
			}
		})
	}
}
