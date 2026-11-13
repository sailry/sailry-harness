package acceptance

import (
	"encoding/json"
	"os"
	"path/filepath"
	"reflect"
	"runtime"
	"strings"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestWriteReportPreservesPrivateEvidence(t *testing.T) {
	raw := `<script>alert("raw evidence")</script><img src="https://invalid.example/test"> & "quoted"`
	report := Report{
		Version: 1, Suite: "backend", Provider: "provider-fixture", Model: "model-fixture",
		Effort:  &protocol.Effort{Kind: "medium"},
		Started: time.Date(2026, 11, 13, 1, 2, 3, 0, time.UTC), Finished: time.Date(2026, 11, 13, 1, 2, 5, 0, time.UTC),
		Cases: []CaseResult{{ID: "F01", Group: "backend", Title: "Command output", Status: "passed", Detail: raw, Duration: time.Second,
			Session: "session-fixture", Turns: []string{"turn-fixture"}, Evidence: []Evidence{{Kind: "tool_output", Value: raw}}}},
	}
	directory := filepath.Join(t.TempDir(), "reports")
	paths, err := WriteReport(directory, report)
	if err != nil {
		t.Fatal(err)
	}
	if paths.JSON != filepath.Join(directory, "latest.json") || paths.HTML != filepath.Join(directory, "latest.html") {
		t.Fatalf("report paths: %#v", paths)
	}
	data, err := os.ReadFile(paths.JSON)
	if err != nil {
		t.Fatal(err)
	}
	var decoded Report
	if err := json.Unmarshal(data, &decoded); err != nil {
		t.Fatal(err)
	}
	if !reflect.DeepEqual(decoded, report) {
		t.Fatalf("JSON changed the report: %#v", decoded)
	}
	page, err := os.ReadFile(paths.HTML)
	if err != nil {
		t.Fatal(err)
	}
	if strings.Contains(string(page), "<script>") || strings.Contains(string(page), "<img src=") ||
		!strings.Contains(string(page), "&lt;script&gt;") || !strings.Contains(string(page), "Content-Security-Policy") {
		t.Fatal("HTML did not retain evidence as escaped, inert text")
	}
	if !strings.Contains(string(page), "<dt>Reasoning effort</dt><dd>medium</dd>") {
		t.Fatal("HTML lost the selected reasoning effort")
	}
	if runtime.GOOS != "windows" {
		for path, permission := range map[string]os.FileMode{directory: 0o700, paths.JSON: 0o600, paths.HTML: 0o600} {
			info, err := os.Stat(path)
			if err != nil || info.Mode().Perm() != permission {
				t.Fatalf("private permissions for %s: %v, %v", path, info, err)
			}
		}
	}
}

func TestWriteReportReplacesLatestFiles(t *testing.T) {
	directory := filepath.Join(t.TempDir(), "reports")
	first := Report{Version: 1, Suite: "quick", Model: "first-model", Cases: []CaseResult{{ID: "A01", Status: "failed", Detail: "old-report-marker"}}}
	paths, err := WriteReport(directory, first)
	if err != nil {
		t.Fatal(err)
	}
	second := Report{Version: 1, Suite: "backend", Model: "second-model", Cases: []CaseResult{{ID: "D04", Status: "passed", Detail: "new-report-marker"}}}
	if _, err := WriteReport(directory, second); err != nil {
		t.Fatal(err)
	}
	for _, path := range []string{paths.JSON, paths.HTML} {
		data, err := os.ReadFile(path)
		if err != nil || strings.Contains(string(data), "old-report-marker") || !strings.Contains(string(data), "new-report-marker") {
			t.Fatalf("latest report was not replaced at %s: %v", path, err)
		}
	}
	entries, err := os.ReadDir(directory)
	if err != nil || len(entries) != 2 {
		t.Fatalf("report publication left temporary files: %v, %v", entries, err)
	}
}

func TestWriteReportRequiresDirectory(t *testing.T) {
	if _, err := WriteReport("", Report{}); err == nil {
		t.Fatal("an empty report directory was accepted")
	}
}

func TestWriteReportRejectsSharedDirectory(t *testing.T) {
	if runtime.GOOS == "windows" {
		t.Skip("Windows storage privacy is enforced through its inherited ACL")
	}
	directory := filepath.Join(t.TempDir(), "shared")
	if err := os.Mkdir(directory, 0o700); err != nil {
		t.Fatal(err)
	}
	if err := os.Chmod(directory, 0o755); err != nil {
		t.Fatal(err)
	}
	if _, err := WriteReport(directory, Report{}); err == nil {
		t.Fatal("an existing shared report directory was accepted")
	}
	entries, err := os.ReadDir(directory)
	if err != nil || len(entries) != 0 {
		t.Fatalf("private evidence was written to a shared directory: %v, %v", entries, err)
	}
}
