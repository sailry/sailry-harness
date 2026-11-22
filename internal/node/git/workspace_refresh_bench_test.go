package git

import (
	"context"
	"fmt"
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"testing"
	"time"

	gogit "github.com/go-git/go-git/v5"
	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func BenchmarkWorkspaceResourceRefresh(b *testing.B) {
	home := b.TempDir()
	b.Setenv("HOME", home)
	b.Setenv("XDG_CONFIG_HOME", home)
	b.Setenv("GIT_CONFIG_GLOBAL", os.DevNull)
	b.Setenv("GIT_CONFIG_NOSYSTEM", "1")
	makeRepo := func() string {
		root, err := filepath.EvalSymlinks(b.TempDir())
		if err != nil {
			b.Fatal(err)
		}
		if _, err := gogit.PlainInit(root, false); err != nil {
			b.Fatal(err)
		}
		for i := 0; i < 128; i++ {
			name := fmt.Sprintf("entry-%03d.txt", i)
			data := "abc\n"
			if i == 0 {
				data = strings.Repeat("0123456789abcdef\n", 1024)
			}
			if err := os.WriteFile(filepath.Join(root, name), []byte(data), 0600); err != nil {
				b.Fatal(err)
			}
		}
		return root
	}
	root := makeRepo()
	ctx := context.Background()
	for _, kind := range []string{"directory", "status", "status-hit", "diff", "different-roots"} {
		b.Run(kind, func(b *testing.B) {
			roots := []string{root}
			if kind == "different-roots" {
				for i := 1; i < min(b.N, 64); i++ {
					roots = append(roots, makeRepo())
				}
			}
			now := time.Now()
			s := New(Options{Now: func() time.Time { return now }})
			f, err := core.OpenFiles(root, core.ManualFiles)
			if err != nil {
				b.Fatal(err)
			}
			defer f.Close()
			if kind == "status-hit" {
				if _, err := s.Status(ctx, root); err != nil {
					b.Fatal(err)
				}
			}
			runtime.GC()
			var before, after runtime.MemStats
			runtime.ReadMemStats(&before)
			b.ReportAllocs()
			b.ResetTimer()
			for i := 0; i < b.N; i++ {
				switch kind {
				case "directory":
					v, err := f.ListDirectory(ctx, "", nil)
					if err != nil || len(v.Entries) == 0 {
						b.Fatalf("directory: %d, %v", len(v.Entries), err)
					}
				case "diff":
					v, err := s.Diff(ctx, root, "entry-000.txt", protocol.GitDiffScopeUnstaged)
					if err != nil || len(v.Text) == 0 {
						b.Fatalf("diff: %d, %v", len(v.Text), err)
					}
				default:
					if kind != "status-hit" {
						now = now.Add(2 * time.Second)
					}
					v, err := s.Status(ctx, roots[i%len(roots)])
					if err != nil || len(v.Entries) != 128 {
						b.Fatalf("status: %d, %v", len(v.Entries), err)
					}
				}
			}
			b.StopTimer()
			if kind == "different-roots" {
				time.Sleep(1200 * time.Millisecond)
			}
			runtime.GC()
			runtime.ReadMemStats(&after)
			b.ReportMetric(float64(int64(after.HeapAlloc)-int64(before.HeapAlloc)), "live-delta-B")
			s.mu.Lock()
			b.ReportMetric(float64(len(s.statuses)), "cached-roots")
			s.mu.Unlock()
			runtime.KeepAlive(s)
			runtime.KeepAlive(f)
			runtime.KeepAlive(roots)
		})
	}
}
