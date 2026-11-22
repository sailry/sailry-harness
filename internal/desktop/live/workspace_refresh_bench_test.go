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
		files: map[string]protocol.FileContent{}, directories: map[string]bool{},
		diffContents: map[string][]viewmodel.Hunk{}, diffs: map[string]bool{},
	}
	entries := make([]protocol.FileEntry, 128)
	status := protocol.GitStatus{Kind: protocol.RepositoryKindReady, Entries: make([]protocol.GitEntry, 128)}
	for i := range entries {
		path := fmt.Sprintf("file-%03d.txt", i)
		entries[i] = protocol.FileEntry{Name: path, Kind: protocol.EntryKindFile}
		status.Entries[i] = protocol.GitEntry{Path: path, Untracked: true}
	}
	patch := "@@ -0,0 +1,1024 @@\n" + strings.Repeat("+"+strings.Repeat("x", 95)+"\n", 1024)
	refresh := func(i int) {
		p.directoryPage(nil, "", protocol.Directory{Entries: entries}, entries)
		p.projectStatus(status)
		path := entries[i%len(entries)].Name
		p.panel.Files.Show(path)
		p.panel.Git.Show(path)
		p.fileLoaded(path, protocol.FileContent{Path: path, Text: strings.Repeat("x", 128<<10)})
		hunks := workspaceHunks(patch)
		p.diffContents["all/"+path] = hunks
		p.diffs["all/"+path] = true
		p.projectDiff(path, "all", hunks, protocol.GitDiff{})
		p.panel.Git.Close(path)
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
	if len(p.files) != 0 || len(p.diffContents) != 0 || len(p.diffs) != 0 {
		b.Fatalf("closed file/diff retained: %d/%d/%d", len(p.files), len(p.diffContents), len(p.diffs))
	}
	runtime.KeepAlive(p)
}
