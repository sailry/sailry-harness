//go:build integration && (darwin || linux)

package process

import (
	"bytes"
	"io"
	"os"
	"os/exec"
	"path/filepath"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/node/git"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func BenchmarkNativeCommandOutput(b *testing.B) {
	root := b.TempDir()
	data := bytes.Repeat([]byte("checking package and handling fixture output\n"), 100000)
	if err := os.WriteFile(filepath.Join(root, "fixture"), data, 0600); err != nil {
		b.Fatal(err)
	}
	for _, fixture := range []struct {
		name, command string
		size          int
	}{{"small", "head -c 5882 fixture", 5882}, {"large", "cat fixture", len(data)}} {
		variables := []string{"PATH=/usr/bin:/bin", "HOME=" + b.TempDir()}
		b.Run(fixture.name+"/direct", func(b *testing.B) {
			for b.Loop() {
				cmd := exec.CommandContext(b.Context(), "/bin/sh", "-c", fixture.command)
				cmd.Dir, cmd.Env, cmd.Stdout = root, variables, io.Discard
				if err := cmd.Run(); err != nil {
					b.Fatal(err)
				}
			}
		})
		for _, access := range []protocol.ProcessAccess{protocol.ProcessAccessUnrestricted, protocol.ProcessAccessWorkspace} {
			b.Run(fixture.name+"/"+string(access), func(b *testing.B) {
				s, err := New(Options{Profile: b.TempDir(), Environment: NewEnvironment(variables), CaptureManaged: git.New(git.Options{}).CaptureManaged})
				if err != nil {
					b.Fatal(err)
				}
				defer s.Close()
				var elapsedMs uint64
				var pageTime time.Duration
				for b.Loop() {
					launch := Launch{ID: protocol.NewRequestID(), Session: protocol.NewSessionID(), Turn: protocol.NewTurnID(), Root: root, Command: fixture.command, Access: access, TimeoutMs: 120000}
					result, err := s.Execute(b.Context(), launch)
					if err != nil || result.Outcome.Kind != "exited" || result.Outcome.Data != int32(0) {
						b.Fatalf("execute: %v, %v", result.Outcome, err)
					}
					elapsedMs += result.ElapsedMs
					start := time.Now()
					page, err := s.Page(launch.Session, launch.ID, protocol.ProcessOutputOutputStreamStdout, 0, 32768)
					pageTime += time.Since(start)
					if err != nil || page.TotalBytes != uint64(fixture.size) || page.Text != string(data[:min(fixture.size, 32768)]) {
						b.Fatalf("read %d of %d bytes, %v", len(page.Text), page.TotalBytes, err)
					}
					b.StopTimer()
					if _, err := s.CloseSession(b.Context(), launch.Session); err != nil {
						b.Fatal(err)
					}
					b.StartTimer()
				}
				b.ReportMetric(float64(elapsedMs)/float64(b.N), "runner-ms/op")
				b.ReportMetric(float64(pageTime.Microseconds())/float64(b.N), "page-us/op")
			})
		}
	}
}
