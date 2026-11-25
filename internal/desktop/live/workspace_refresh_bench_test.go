package live

import (
	"fmt"
	"runtime"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func BenchmarkWorkspaceRefreshClose(b *testing.B) {
	data := &viewmodel.Workspace{Files: &viewmodel.FileNode{Children: []*viewmodel.FileNode{}}}
	pr := panel.NewLiveProject("fixture", data, &panel.Actions{})
	p := &workspaceProject{
		data: data, panel: pr,
		files: map[string]protocol.FileContent{},
	}
	entries := make([]protocol.FileEntry, 128)
	for i := range entries {
		path := fmt.Sprintf("file-%03d.txt", i)
		entries[i] = protocol.FileEntry{Name: path, Kind: protocol.EntryKindFile}
	}
	refresh := func(i int) {
		path := entries[i%len(entries)].Name
		p.panel.Files.Show(path)
		p.fileLoaded(path, protocol.FileContent{Path: path, Text: strings.Repeat("x", 128<<10)})
		p.panel.Files.Close(path)
		p.pruneClosedContent()
	}
	refresh(0)
	runtime.GC()
	var before, after runtime.MemStats
	runtime.ReadMemStats(&before)
	b.ReportAllocs()
	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		refresh(i)
	}
	b.StopTimer()
	runtime.GC()
	runtime.ReadMemStats(&after)
	b.ReportMetric(float64(int64(after.HeapAlloc)-int64(before.HeapAlloc)), "live-delta-B")
	if len(p.files) != 0 {
		b.Fatalf("closed files retained: %d", len(p.files))
	}
	runtime.KeepAlive(p)
}
