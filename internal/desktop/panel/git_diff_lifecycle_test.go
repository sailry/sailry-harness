package panel

import (
	"fmt"
	"runtime"
	"testing"

	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
)

func BenchmarkWorkspaceDiffPathChurn(b *testing.B) {
	pr := NewLiveProject("fixture", &viewmodel.Workspace{}, &Actions{})
	runtime.GC()
	var before, after runtime.MemStats
	runtime.ReadMemStats(&before)
	b.ReportAllocs()
	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		path := fmt.Sprintf("folder-%07d/entry.bin", i)
		pr.Git.Show(path)
		pr.Data.Changes = []viewmodel.FileChange{{Path: path, Diff: viewmodel.DiffState{Scope: "all", Binary: true, Size: 4096}}}
		pr.Git.Close(path)
	}
	b.StopTimer()
	runtime.GC()
	runtime.ReadMemStats(&after)
	b.ReportMetric(float64(int64(after.HeapAlloc)-int64(before.HeapAlloc)), "live-delta-B")
	b.ReportMetric(float64(len(pr.Data.Changes)), "current-changes")
	runtime.KeepAlive(pr)
}
