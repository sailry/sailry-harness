package live

import (
	"errors"
	"strings"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/panel"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/desktop/updater"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestUpdateDocumentLock(t *testing.T) {
	for kind := updater.Idle; kind <= updater.Stopped; kind++ {
		want := kind == updater.Installing || kind == updater.Restarting || kind == updater.Stopped
		if got := updateLocksDocuments(kind); got != want {
			t.Fatalf("document lock for state %d = %v, want %v", kind, got, want)
		}
	}
}

func updateEditorWindow(fixture pluginLocationTestFixture, path string) (*ui.Tester, *workspaceProject) {
	root := fixture.root
	project := root.workspaceFor(fixture.main)
	project.panel.Data = &viewmodel.Workspace{Files: &viewmodel.FileNode{Children: []*viewmodel.FileNode{{Path: path, Name: path, Text: "Original text"}}}}
	project.panel.Actions = &panel.Actions{}
	project.panel.SetFile(path, "Original text")
	project.panel.SetFileWritable(path, true)
	project.panel.Files.Show(path)
	tester := ui.NewTester(func(c *ui.Context) {
		root.beforeUpdateFrame()
		theme.Apply(c, "", theme.Light)
		ui.Column(c).Fill().Children(func() { panel.Page(c, panel.Files, project.panel, func() {}) })
		root.afterUpdateFrame()
	}, 1000, 650)
	tester.SetLocale("en")
	return tester, project
}

func TestUpdatePendingNativeInput(t *testing.T) {
	for _, path := range []string{"note.txt", "note.md"} {
		t.Run(path, func(t *testing.T) {
			first, second := newPluginLocationTestFixture(t), newPluginLocationTestFixture(t)
			updates := NewUpdates(first.root.Preferences, t.TempDir(), nil, nil)
			t.Cleanup(updates.Service.Close)
			first.root.SetUpdates(updates, func() bool { return true })
			second.root.SetUpdates(updates, func() bool { return false })
			window, project := updateEditorWindow(second, path)
			label := "Code editor"
			if path == "note.md" {
				label = "Rich text"
			}
			field, ok := window.Access(label)
			if !ok {
				t.Fatal("editor is missing", label)
			}
			window.ClickAt(field.Bounds.X+40, field.Bounds.Y+8)
			// The first window has applied its lock, while the second has
			// not rendered since the request. Type delivers native input
			// before its next frame invokes beforeUpdateFrame.
			updates.install = &updateInstall{requester: first.root, windows: map[*Runtime]uint8{first.root: 2, second.root: 0}}
			window.Type("late input")
			if updates.install != nil || first.root.documentsLocked() || second.root.documentsLocked() {
				t.Fatal("pending native input did not cancel installation")
			}
			if draft, dirty := project.panel.FileDraft(path); !dirty || !strings.Contains(draft, "late input") {
				t.Fatal("pending native input was not retained", draft, dirty)
			}
		})
	}
}

func TestUpdateFlushPendingWindows(t *testing.T) {
	for _, hidden := range []bool{false, true} {
		name := "already rendered"
		if hidden {
			name = "hidden"
		}
		t.Run(name, func(t *testing.T) {
			first, second := newPluginLocationTestFixture(t), newPluginLocationTestFixture(t)
			updates := NewUpdates(first.root.Preferences, t.TempDir(), nil, nil)
			t.Cleanup(updates.Service.Close)
			first.root.SetUpdates(updates, func() bool { return true })
			second.root.SetUpdates(updates, func() bool { return false })
			visible, _ := updateEditorWindow(first, "first.txt")
			other, _ := updateEditorWindow(second, "second.md")
			var callbacks []func()
			first.root.SetUpdate(func(fn func()) { callbacks = append(callbacks, fn) })
			callbacks = nil
			flushes := 0
			first.root.updatesFlush = func() error {
				t.Fatal("the active frame's window was captured recursively")
				return nil
			}
			second.root.updatesFlush = func() error {
				flushes++
				other.Frame()
				return nil
			}
			updates.install = &updateInstall{requester: first.root, windows: map[*Runtime]uint8{first.root: 0, second.root: 0}}
			if !hidden {
				other.Frame()
			}
			visible.Frame()
			for len(callbacks) > 0 {
				fn := callbacks[0]
				callbacks = callbacks[1:]
				fn()
			}
			want := 0
			if hidden {
				want = 1
			}
			if flushes != want || updates.install != nil {
				t.Fatalf("flushes=%d want=%d pending=%v", flushes, want, updates.install != nil)
			}
		})
	}
}

func TestUpdateFlushWithoutLockFrame(t *testing.T) {
	first, second := newPluginLocationTestFixture(t), newPluginLocationTestFixture(t)
	updates := NewUpdates(first.root.Preferences, t.TempDir(), nil, nil)
	t.Cleanup(updates.Service.Close)
	first.root.SetUpdates(updates, func() bool { return true })
	second.root.SetUpdates(updates, func() bool { return false }, func() error { return nil })
	updates.install = &updateInstall{requester: first.root, windows: map[*Runtime]uint8{first.root: 2, second.root: 0}}
	updates.flushInstall(updates.install)
	if updates.install != nil || first.root.documentsLocked() || second.root.documentsLocked() {
		t.Fatal("an unacknowledged refresh left installation waiting or editors locked")
	}
	window := closeTestWindow(first.root)
	if !window.HasText(i18n.In("en", "updates_install_failed")) {
		t.Fatal("the rejected installation did not report its failure")
	}
}

// checkSystemHelpers covers the About, diagnostics and selection mappings.
func checkSystemHelpers(t *testing.T) {
	var u settings.Updater
	updateView(&u, updater.State{Kind: updater.Downloading, Version: "0.2.0", Progress: updater.Progress{Copied: 5, Total: 10}}, nil)
	if u.State != settings.UpdateDownloading || u.Version != "0.2.0" || u.Copied != 5 || u.Total != 10 || u.Failure != nil {
		t.Fatalf("download view = %+v", u)
	}
	// SABOUT-005: progress is bytes copied of the total, then verifying at 100.
	if u.Percent() != 50 || u.Verifying {
		t.Fatalf("download percent = %v, verifying %v", u.Percent(), u.Verifying)
	}
	updateView(&u, updater.State{Kind: updater.Downloading, Version: "0.2.0", Progress: updater.Progress{Copied: 10, Total: 10, Verifying: true}}, nil)
	if u.State != settings.UpdateDownloading || !u.Verifying || u.Percent() != 100 || !u.Locked() {
		t.Fatalf("verifying view = %+v", u)
	}
	failure := &updater.Failure{Key: "updates_signature_invalid", Detail: "bad signature"}
	updateView(&u, updater.State{Kind: updater.Idle}, failure)
	first := u.Failure
	updateView(&u, updater.State{Kind: updater.Idle}, &updater.Failure{Key: failure.Key, Detail: failure.Detail})
	if u.State != settings.UpdateIdle || u.Copied != 0 || u.Verifying || first == nil || u.Failure != first || *first != (settings.UpdateFailure{Key: failure.Key, Detail: failure.Detail}) {
		t.Fatalf("failure view = %+v", u)
	}
	for kind, want := range map[updater.Kind]settings.UpdateState{updater.Checking: settings.UpdateChecking, updater.Available: settings.UpdateAvailable, updater.Ready: settings.UpdateReady,
		updater.Installing: settings.UpdateInstalling, updater.Restarting: settings.UpdateRestarting, updater.Stopped: settings.UpdateStopped} {
		if updateView(&u, updater.State{Kind: kind}, nil); u.State != want || u.Failure != nil {
			t.Fatalf("state %d = %+v", kind, u)
		}
	}

	for _, test := range []struct {
		notice updater.Notice
		kind   string
		action bool
	}{
		{updater.Notice{Key: "updates_ready", Version: "0.2.0"}, "success", true},
		{updater.Notice{Key: "updates_current"}, "success", false},
		{updater.Notice{Key: "updates_network_failed", Error: true, Detail: "refused"}, "error", true},
		{updater.Notice{Key: "updates_install_uncertain", Error: true, Detail: "kept"}, "warning", true},
	} {
		toast := updateToast(test.notice, "title", "action", 1, func() {})
		if toast.Type != test.kind || (toast.OnAction != nil) != test.action || toast.Timeout != 1 {
			t.Fatalf("toast for %+v = %+v", test.notice, toast)
		}
	}

	for _, test := range []struct {
		err         error
		diagnostics bool
		outcome     kit.Outcome
		key         string
	}{
		{nil, false, kit.Done, "telemetry_reset"},
		{nil, true, kit.Done, "telemetry_diagnostics_sent"},
		{&protocol.Fault{Code: protocol.ErrorCodeRevisionConflict}, false, kit.Conflict, "telemetry_conflict"},
		{&protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown}, false, kit.Uncertain, "telemetry_unknown"},
		{errors.New("closed"), false, kit.Failed, "telemetry_failed"},
		{&protocol.Fault{Code: protocol.ErrorCodeUnavailable}, true, kit.Failed, "telemetry_diagnostics_failed"},
	} {
		if outcome, key := telemetryOutcome(test.err, test.diagnostics); outcome != test.outcome || key != test.key {
			t.Fatalf("outcome for %v = %v %q", test.err, outcome, key)
		}
	}

	for text, want := range map[string]string{"selected": "", " \n": "selection_empty", "": "selection_empty", "a\x00b": "selection_large", strings.Repeat("a", 65537): "selection_large"} {
		if got, key := boundedSelection(text); key != want || (want == "") != (got == text && got != "") {
			t.Fatalf("selection %q = %q %q", text[:min(len(text), 8)], got[:min(len(got), 8)], key)
		}
	}
}
