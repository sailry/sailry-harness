package main

import (
	"os"
	"path/filepath"
	"testing"

	core "github.com/sailry/sailry-harness/internal/acceptance"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/node"
)

func TestAcceptanceRejectedRunKeepsPublishedReport(t *testing.T) {
	for _, reason := range []string{"busy", "uncertain", "uncertain-ui"} {
		t.Run(reason, func(t *testing.T) {
			profile := t.TempDir()
			paths, err := core.WriteReport(filepath.Join(profile, "desktop", "acceptance"), core.Report{
				Version: 1, Suite: "backend", Model: "previous-model",
				Cases: []core.CaseResult{{ID: "previous", Status: "passed"}},
			})
			if err != nil {
				t.Fatal(err)
			}
			before := map[string]string{}
			for _, path := range []string{paths.JSON, paths.HTML} {
				data, err := os.ReadFile(path)
				if err != nil {
					t.Fatal(err)
				}
				before[path] = string(data)
			}
			owner := &desktopAcceptance{backend: &desktopBackend{Path: profile, Node: new(node.Node), Client: new(client.Client)}}
			if reason == "busy" {
				owner.mu.Lock()
			} else {
				owner.uncertain = true
			}
			suite := "backend"
			if reason == "uncertain-ui" {
				suite = "ui"
			}
			report, published, err := owner.run(t.Context(), core.Options{Suite: suite}, nil)
			if reason == "busy" {
				owner.mu.Unlock()
			}
			if err == nil || report.OK() || published != (core.ReportPaths{}) {
				t.Fatalf("rejected acceptance was published: report=%+v, paths=%+v, error=%v", report, published, err)
			}
			if reason != "busy" && (len(report.Cases) != 1 || !report.Cases[0].Uncertain) {
				t.Fatal("unconfirmed work did not block another real-model run")
			}
			for path, expected := range before {
				data, err := os.ReadFile(path)
				if err != nil || string(data) != expected {
					t.Fatalf("rejected run replaced the published report: %s, %v", path, err)
				}
			}
		})
	}
}
