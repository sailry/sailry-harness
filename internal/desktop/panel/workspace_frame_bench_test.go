package panel

import (
	"fmt"
	"runtime"
	"strings"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

func BenchmarkWorkspacePanelFrame(b *testing.B) {
	for _, kind := range []string{"status", "diff", "diff-close"} {
		b.Run(kind, func(b *testing.B) {
			pr := NewProjects(&preview.Workspace{}).Of("fixture")
			pr.Actions = &Actions{}
			pr.SetGitState(GitState{Known: true, Kind: "ready"})
			root := &preview.FileNode{Children: []*preview.FileNode{}}
			for i := 0; i < 128; i++ {
				path := fmt.Sprintf("entry-%03d.txt", i)
				root.Children = append(root.Children, &preview.FileNode{Name: path, Path: path})
				pr.Data.Changes = append(pr.Data.Changes, preview.FileChange{Path: path, Status: "M", Unstaged: "M"})
			}
			pr.Data.Files = root
			lines := make([]preview.DiffLine, 1024)
			for i := range lines {
				lines[i] = preview.DiffLine{Kind: preview.Added, New: i + 1, Text: fmt.Sprintf("%04d %s", i, strings.Repeat("x", 90))}
			}
			pr.Data.Changes[0].Hunks = []preview.Hunk{{Lines: lines}}
			path := pr.Data.Changes[0].Path
			tt := ui.NewTesterWith(func(c *ui.Context) {
				theme.Apply(c, "", theme.Light)
				switch kind {
				case "status":
					pr.GitRepository(c, true, false, pr.Git.Show, nil)
				default:
					if pr.Git.Change() != "" {
						pr.GitDiff(c, pr.Git.Change(), 900)
					}
				}
			}, 900, 600, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true}})
			pr.Git.Show(path)
			tt.Frame()
			tt.Frame()
			runtime.GC()
			var before, after runtime.MemStats
			runtime.ReadMemStats(&before)
			b.ReportAllocs()
			b.ResetTimer()
			for i := 0; i < b.N; i++ {
				if kind == "diff-close" {
					pr.Git.Close(path)
					tt.Frame()
					pr.Git.Show(path)
				}
				tt.Frame()
			}
			b.StopTimer()
			pr.Git.Close(path)
			tt.Frame()
			tt.Frame()
			runtime.GC()
			runtime.ReadMemStats(&after)
			b.ReportMetric(float64(int64(after.HeapAlloc)-int64(before.HeapAlloc)), "live-delta-B")
			runtime.KeepAlive(tt)
		})
	}
}
