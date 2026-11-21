package main

import (
	"slices"
	"testing"

	"github.com/sailry/sailry-harness/internal/desktop/shell"
)

func TestWindowPreferencesCloseWhileBlurSaveIsBlocked(t *testing.T) {
	started, release := make(chan struct{}), make(chan struct{})
	var written []float32
	p := windowPreferences{directory: t.TempDir(), write: func(directory string, prefs shell.Prefs) error {
		if len(written) == 0 {
			close(started)
			<-release
		}
		written = append(written, prefs.NavWidth)
		return shell.SavePrefs(directory, prefs)
	}}
	prefs := shell.DefaultPrefs()
	prefs.NavWidth = 201
	p.save(prefs)
	<-started
	prefs.NavWidth = 202
	p.save(prefs)
	prefs.NavWidth = 203
	prefs.Features = []string{"first"}
	p.save(prefs)
	p.save(prefs) // WM_KILLFOCUS during DestroyWindow sees the same state.
	prefs.Features[0] = "changed"
	opened := p.load()
	close(release)
	p.flush()
	if opened.NavWidth != 203 || !slices.Equal(opened.Features, []string{"first"}) {
		t.Fatal("a new window missed the pending close snapshot")
	}
	if !slices.Equal(written, []float32{201, 203}) {
		t.Fatalf("window writes in order: %v", written)
	}
	if saved := shell.LoadPrefs(p.directory); saved.NavWidth != 203 || !slices.Equal(saved.Features, opened.Features) {
		t.Fatal("shutdown returned before the latest isolated snapshot reached disk")
	}
}
