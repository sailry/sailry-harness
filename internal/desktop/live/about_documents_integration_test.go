//go:build integration

package live

import (
	"errors"
	"os"
	"path/filepath"
	"sync"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/desktop/preferences"
	"github.com/sailry/sailry-harness/internal/desktop/updater"
	"github.com/sailry/sailry-harness/internal/link"
)

func TestUpdateLocksEveryWindowDocuments(t *testing.T) {
	root, err := filepath.EvalSymlinks(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	profile := filepath.Join(root, "profile")
	installed := filepath.Join(root, "Applications", "Sailry.app")
	if err := updater.WriteFixtureBundle(installed, link.ProductVersion); err != nil {
		t.Fatal(err)
	}
	feed := updater.NewFixtureFeed()
	t.Cleanup(feed.Server.Close)
	feed.Publish("9.1.0", false)
	stopping, proceed, quit := make(chan struct{}), make(chan struct{}), make(chan struct{})
	updates := NewUpdates(preferences.Open(filepath.Join(profile, "preferences.json")), profile, func() error {
		close(stopping)
		<-proceed
		return nil
	}, func() { close(quit) })
	t.Cleanup(feed.Bind(updates.Service, installed, func(string) {}))
	t.Cleanup(updates.Service.Close)
	var release sync.Once
	t.Cleanup(func() { release.Do(func() { close(proceed) }) })
	first, second := newPluginLocationTestFixture(t), newPluginLocationTestFixture(t)
	for _, fixture := range []pluginLocationTestFixture{first, second} {
		fixture.root.SetUpdates(updates, func() bool { return true })
	}
	wait := func(kind updater.Kind) {
		t.Helper()
		for deadline := time.Now().Add(10 * time.Second); time.Now().Before(deadline); time.Sleep(time.Millisecond) {
			state, failure := updates.Service.Snapshot()
			if state.Kind == kind {
				return
			}
			if failure != nil {
				t.Fatalf("waiting for %d: %+v", kind, failure)
			}
		}
		t.Fatalf("updater did not reach %d", kind)
	}
	updates.Service.Check()
	wait(updater.Available)
	updates.Service.Download()
	wait(updater.Ready)
	other := closeTestDraft(second)
	install := first.root.App.Settings.UpdateActions.Install
	install()
	if state, _ := updates.Service.Snapshot(); state.Kind != updater.Ready || first.root.documentsLocked() {
		t.Fatal("installation ignored another window's unsaved document")
	}
	other.panel.SavedFile("a.go", "two\n")
	other.panel.SetFileStatus("a.go", true, false)
	install()
	if state, _ := updates.Service.Snapshot(); state.Kind != updater.Ready {
		t.Fatal("installation ignored a pending document save")
	}
	other.panel.SetFileStatus("a.go", false, false)
	frame := func(runtime *Runtime) {
		runtime.beforeUpdateFrame()
		runtime.afterUpdateFrame()
	}
	// One window may still receive input while another has built its locked
	// frame. Preserve that draft and cancel before the Node can stop.
	install()
	frame(first.root)
	if !first.root.documentsLocked() || second.root.documentsLocked() {
		t.Fatal("a lock request did not follow each window's own frame")
	}
	other.panel.FileChanged("a.go", "pending input")
	frame(second.root)
	if updates.install != nil || first.root.documentsLocked() || second.root.documentsLocked() {
		t.Fatal("new input did not cancel the pending installation")
	}
	if draft, dirty := other.panel.FileDraft("a.go"); draft != "pending input\n" || !dirty {
		t.Fatal("pending input was discarded", draft, dirty)
	}
	other.panel.SavedFile("a.go", "pending input\n")
	// A failed hidden-window refresh must not install, even if the refresh
	// built its lock frame before reporting the failure.
	install()
	frame(first.root)
	second.root.updatesFlush = func() error {
		frame(second.root)
		return errors.New("fixture capture failed")
	}
	updates.flushInstall(updates.install)
	if state, _ := updates.Service.Snapshot(); state.Kind != updater.Ready || updates.install != nil || first.root.documentsLocked() || second.root.documentsLocked() {
		t.Fatal("failed hidden-window refresh did not abort and unlock")
	}
	second.root.updatesFlush = nil
	// Failure before shutdown returns to Ready and must restore editing.
	receipt := filepath.Join(profile, "updater", "receipt.json")
	if err := os.WriteFile(receipt, []byte("fixture receipt"), 0o600); err != nil {
		t.Fatal(err)
	}
	install()
	// New windows join before their first frame; closed windows no longer
	// hold up installation.
	late := newPluginLocationTestFixture(t)
	late.root.SetUpdates(updates, func() bool { return false })
	frame(first.root)
	frame(second.root)
	if state, _ := updates.Service.Snapshot(); state.Kind != updater.Ready || updates.install == nil {
		t.Fatal("installation ignored a newly registered window")
	}
	late.root.mu.Lock()
	late.root.closed = true
	late.root.mu.Unlock()
	frame(first.root)
	wait(updater.Ready)
	if _, failure := updates.Service.Snapshot(); failure == nil || failure.Key != "updates_receipt_unread" {
		t.Fatalf("expected pre-shutdown failure: %+v", failure)
	}
	if first.root.documentsLocked() || second.root.documentsLocked() {
		t.Fatal("pre-shutdown failure left documents locked")
	}
	other.panel.FileChanged("a.go", "recovered edit")
	if _, dirty := other.panel.FileDraft("a.go"); !dirty {
		t.Fatal("pre-shutdown failure did not restore editing")
	}
	other.panel.SavedFile("a.go", "recovered edit\n")
	if err := os.Remove(receipt); err != nil {
		t.Fatal(err)
	}
	install()
	frame(first.root)
	frame(second.root)
	select {
	case <-stopping:
	case <-time.After(10 * time.Second):
		t.Fatal("installation did not reach shutdown")
	}
	// Includes a workspace first opened after installation began. All panels
	// resolve the shared updater dynamically.
	for _, fixture := range []pluginLocationTestFixture{first, second} {
		project := fixture.root.workspaceFor(fixture.main)
		project.panel.SetFile("late.txt", "saved")
		project.panel.SetFileWritable("late.txt", true)
		project.panel.FileChanged("late.txt", "new unsaved edit")
		if !fixture.root.documentsLocked() {
			t.Fatal("a window was editable after shutdown started")
		}
		if draft, dirty := project.panel.FileDraft("late.txt"); draft != "saved" || dirty {
			t.Fatal("an installation accepted a draft it could not save")
		}
	}
	release.Do(func() { close(proceed) })
	select {
	case <-quit:
	case <-time.After(10 * time.Second):
		t.Fatal("the fixture installation did not hand off")
	}
	if len(first.root.UnsavedDocuments())+len(second.root.UnsavedDocuments()) != 0 {
		t.Fatal("the updater quit with unsaved documents")
	}
}
