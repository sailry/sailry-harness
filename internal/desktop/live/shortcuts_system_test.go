package live

import (
	"os"
	"path/filepath"
	"slices"
	"testing"

	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/preferences"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
)

func TestHelpLogsWithUnavailableWriter(t *testing.T) {
	profile := t.TempDir()
	folder := filepath.Join(profile, "logs")
	r := &Runtime{
		ctx:         t.Context(),
		App:         shell.NewLive(&viewmodel.Data{Workspace: &viewmodel.Workspace{}}),
		Preferences: preferences.Open(filepath.Join(profile, "desktop", "preferences.json")),
	}
	logs := func() *mygo.MenuItem {
		t.Helper()
		for _, group := range r.App.Menu("en", r.App.Do) {
			for _, item := range group.Submenu {
				if item.ID == "help.logs" {
					return item
				}
			}
		}
		t.Fatal("Help › Logs is missing")
		return nil
	}
	// An empty installed path is what startup passes if the writer is unavailable.
	r.SetLogFolder("")
	if item := logs(); !item.Disabled || item.Click != nil {
		t.Fatalf("Help › Logs without a folder: %+v", item)
	}
	desktop := filepath.Join(folder, "desktop")
	if err := os.MkdirAll(desktop, 0o700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(desktop, "current.log"), nil, 0o600); err != nil {
		t.Fatal(err)
	}
	r.SetLogFolder("")
	var opened []string
	r.SetNativeFileActions(NativeFileActions{OpenLocal: func(path string) { opened = append(opened, path) }})
	t.Cleanup(r.closeAttachments)
	item := logs()
	if item.Disabled || item.Click == nil {
		t.Fatalf("Help › Logs with earlier local logs: %+v", item)
	}
	window := ui.NewTesterWith(r.App.View, 1280, 820, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true}})
	item.Click(item, nil)
	window.Frame()
	if !slices.Equal(opened, []string{folder}) {
		t.Fatalf("Help › Logs opened %q, want %q", opened, folder)
	}
}
