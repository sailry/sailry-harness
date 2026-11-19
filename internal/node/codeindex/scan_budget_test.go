package codeindex

import (
	"context"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestScanBudgetKeepsFinishedPrefix(t *testing.T) {
	f := newFixture(t)
	f.write("a.rs", "pub fn original() {}\n")
	baseline := f.search("original")
	f.write("a.rs", "pub fn checked() {}\n", "b.rs", "pub fn staged() {}\n", "z.rs", "pub fn pending() {}\n")
	w, err := f.service.worktree(f.id, f.root)
	if err != nil {
		t.Fatal(err)
	}
	progress := <-w.sync
	defer func() { w.sync <- progress }()
	previous := progress.verified["a.rs"]
	l := defaultLimits()
	l.deadline = time.Now().Add(time.Second)
	blocked := false
	f.service.inspectText = func(files *core.Files, ctx context.Context, path string, limit int, stamp core.FileStamp, inspect func(*string) error) (core.FileStamp, error) {
		if path == "z.rs" {
			deadline, ok := ctx.Deadline()
			if !ok || !deadline.Equal(l.deadline) {
				t.Errorf("unexpected inspection deadline: %v, want %v", deadline, l.deadline)
				return core.FileStamp{}, context.Canceled
			}
			blocked = true
			<-ctx.Done()
			return core.FileStamp{}, ctx.Err()
		}
		return files.InspectText(ctx, path, limit, stamp, inspect)
	}
	report, err := f.service.synchronize(t.Context(), w, w.index, progress, l)
	if !blocked || err != nil || !report.partial || !report.truncated || len(report.problems) != 0 || progress.scan == nil || len(progress.scan.ready) != 2 || progress.records.revision != baseline.IndexRevision {
		t.Fatalf("budget did not keep precisely the completed parses: report=%+v scan=%+v err=%v", report, progress.scan, err)
	}
	equal(t, "unfinished work", progress.scan.pending, []string{"a.rs", "b.rs", "z.rs"})
	f.service.inspectText = (*core.Files).InspectText
	// Disk may have changed after the work budget ended. Do not commit the
	// prepared file merely because the watch was silent between segments.
	f.write("b.rs", "pub fn latest() {}\n")
	l = defaultLimits()
	l.batches = 1
	report, err = f.service.synchronize(t.Context(), w, w.index, progress, l)
	if err != nil || !report.partial || progress.scan == nil || len(progress.scan.ready) != 0 || progress.records.revision != baseline.IndexRevision+1 {
		t.Fatalf("prefix did not commit independently: report=%+v scan=%+v err=%v", report, progress.scan, err)
	}
	equal(t, "only interrupted file remains", progress.scan.pending, []string{"z.rs"})
	equal(t, "incomplete pass did not publish new proofs", progress.verified["a.rs"], previous)
	for path, want := range map[string]string{"a.rs": "checked", "b.rs": "latest"} {
		query := protocol.DefaultCodeIndexSearch()
		query.Path = &path
		page, err := w.index.search(t.Context(), &query)
		if err != nil || names2(page.Symbols) != want {
			t.Fatalf("wrong resumed %s: %+v, %v", path, page, err)
		}
	}
	report, err = f.service.synchronize(t.Context(), w, w.index, progress, defaultLimits())
	if err != nil || report.partial || report.truncated || len(report.problems) != 0 || progress.scan != nil || progress.records.revision != baseline.IndexRevision+2 || progress.records.entries["z.rs"].state.Status != StatusParsed {
		t.Fatalf("remaining file did not resume: report=%+v err=%v", report, err)
	}
	if previous.hash != "" && progress.verified["a.rs"].hash != Hash("pub fn checked() {}\n") {
		t.Fatal("completed pass did not publish its new proof")
	}
}

func TestScanParentCancellationIsNotBudget(t *testing.T) {
	f := newFixture(t)
	f.write("a.rs", "pub fn initial() {}\n")
	baseline := f.search("initial")
	f.write("a.rs", "pub fn updated() {}\n", "b.rs", "pub fn appended() {}\n")
	w, err := f.service.worktree(f.id, f.root)
	if err != nil {
		t.Fatal(err)
	}
	progress := <-w.sync
	defer func() { w.sync <- progress }()
	parent, cancel := context.WithCancel(t.Context())
	defer cancel()
	f.service.inspectText = func(files *core.Files, ctx context.Context, path string, limit int, stamp core.FileStamp, inspect func(*string) error) (core.FileStamp, error) {
		if path == "b.rs" {
			cancel()
		}
		return files.InspectText(ctx, path, limit, stamp, inspect)
	}
	_, err = f.service.synchronize(parent, w, w.index, progress, defaultLimits())
	if code(err) != protocol.ErrorCodeCancelled || progress.scan == nil || progress.records.revision != baseline.IndexRevision {
		t.Fatalf("parent cancellation lost cursor or advanced revision: scan=%+v revision=%d err=%v", progress.scan, progress.records.revision, err)
	}
	f.service.inspectText = (*core.Files).InspectText
	report, err := f.service.synchronize(t.Context(), w, w.index, progress, defaultLimits())
	if err != nil || report.partial || report.truncated || len(report.problems) != 0 || progress.scan != nil || progress.records.revision != baseline.IndexRevision+1 {
		t.Fatalf("fresh caller could not resume: report=%+v err=%v", report, err)
	}
}
