//go:build !sailry_no_code_index

package codeindex

import (
	"context"
	"errors"
	"slices"
	"testing"
	"testing/synctest"
	"time"

	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestCodeScansYieldToWaitingOutlineBetweenFiles(t *testing.T) {
	for _, mode := range []string{"live", "persistent"} {
		t.Run(mode, func(t *testing.T) {
			synctest.Test(t, func(t *testing.T) {
				f := newFixture(t)
				f.write("many/a.rs", "fn a() {}", "many/b.rs", "fn b() {}", "target.rs", "fn target() {}")
				first, resume := make(chan struct{}), make(chan struct{})
				visits := make(chan string, 3)
				f.service.inspectText = func(files *core.Files, ctx context.Context, path string, limit int, stamp core.FileStamp, inspect func(*string) error) (core.FileStamp, error) {
					visits <- path
					if path == "many/a.rs" {
						close(first)
						<-resume
					}
					return files.InspectText(ctx, path, limit, stamp, inspect)
				}
				ctx := t.Context()
				scanDone := make(chan error, 1)
				go func() {
					if mode == "live" {
						query := protocol.DefaultCodeIndexSearch()
						query.Path, query.Name = new("many"), new("absent")
						_, err := f.service.SearchOnDemand(ctx, f.id, f.root, query)
						scanDone <- err
						return
					}
					files, err := core.OpenFiles(f.root, core.RestrictedFiles)
					if err == nil {
						defer files.Close()
						var results []inspected
						results, err = f.service.read(ctx, files, []string{"many/a.rs", "many/b.rs"}, inputs{}, defaultLimits().deadline)
						for _, result := range results {
							err = errors.Join(err, result.err)
						}
					}
					scanDone <- err
				}()
				select {
				case <-first:
				case err := <-scanDone:
					t.Fatalf("scan did not inspect its first file: %v", err)
				}
				outlineDone := make(chan error, 1)
				go func() {
					_, err := f.service.OutlineOnDemand(ctx, f.id, f.root, "target.rs")
					outlineDone <- err
				}()
				synctest.Wait()
				if len(visits) != 1 {
					t.Error("second file inspection overlapped the first")
				}
				close(resume)
				if err := errors.Join(<-scanDone, <-outlineDone); err != nil {
					t.Fatal(err)
				}
				close(visits)
				var order []string
				for path := range visits {
					order = append(order, path)
				}
				if !slices.Equal(order, []string{"many/a.rs", "target.rs", "many/b.rs"}) {
					t.Fatalf("reader was not handed to the waiting outline between files: %v", order)
				}
			})
		})
	}
}

func TestLiveReaderWaitBudgetAndCancellation(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		f := newFixture(t)
		f.write("main.rs", "fn run() {}")
		ctx := t.Context()
		release, err := f.service.acquireReader(ctx)
		if err != nil {
			t.Fatal(err)
		}
		search := protocol.DefaultCodeIndexSearch()
		graph := graphQuery("map", "", "main.rs")
		for _, call := range []func() error{
			func() error { _, err := f.service.SearchOnDemand(ctx, f.id, f.root, search); return err },
			func() error { _, err := f.service.GraphOnDemand(ctx, f.id, f.root, graph); return err },
			func() error { _, err := f.service.OutlineOnDemand(ctx, f.id, f.root, "main.rs"); return err },
		} {
			start := time.Now()
			var refusal *protocol.Fault
			if err := call(); !errors.As(err, &refusal) || !ReaderWaitExpired(refusal) || time.Since(start) != queryTime {
				t.Fatalf("first reader wait took %v, returned %v", time.Since(start), err)
			}
		}
		parent, cancel := context.WithCancel(ctx)
		done := make(chan error, 1)
		go func() { _, err := f.service.SearchOnDemand(parent, f.id, f.root, search); done <- err }()
		synctest.Wait()
		cancel()
		if err := <-done; code(err) != protocol.ErrorCodeCancelled {
			t.Fatalf("cancelled waiter returned %v", err)
		}
		deadline, stop := context.WithTimeout(ctx, time.Second)
		defer stop()
		var outer *protocol.Fault
		if _, err := f.service.OutlineOnDemand(deadline, f.id, f.root, "main.rs"); !errors.As(err, &outer) || outer.Code != protocol.ErrorCodeBusy || ReaderWaitExpired(outer) {
			t.Fatalf("caller deadline mistaken for reader contention: %v", err)
		}
		release()
		if outline, err := f.service.OutlineOnDemand(ctx, f.id, f.root, "main.rs"); err != nil || len(outline.Symbols) != 1 || outline.Symbols[0].Name != "run" {
			t.Fatalf("fresh outline after cancelled and expired waiters: %+v, %v", outline, err)
		}
	})
}

func TestLiveReaderWaitPreservesInspectedPrefix(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		f := newFixture(t)
		f.write("a.rs", "fn first() {}", "b.rs", "fn second() {}")
		_, scan, end, err := f.service.liveFiles(t.Context(), f.id, f.root, false)
		if err != nil {
			t.Fatal(err)
		}
		defer end()
		if err := scan.walk("a.rs", nil); err != nil {
			t.Fatal(err)
		}
		release, err := f.service.acquireReader(scan.ctx)
		if err != nil {
			t.Fatal(err)
		}
		defer release()
		if err := scan.walk("b.rs", nil); err != nil || !scan.report.partial || !scan.report.truncated || len(scan.report.problems) != 0 || len(scan.parsed) != 1 || scan.parsed[0].Path != "a.rs" {
			t.Fatalf("lost coverage on reader wait: parsed=%v report=%+v err=%v", scan.parsed, scan.report, err)
		}
	})
}
