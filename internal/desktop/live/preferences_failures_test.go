package live

import (
	"log/slog"
	"os"
	"path/filepath"
	"testing"

	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/preferences"
	"github.com/sailry/sailry-harness/internal/node/logging"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func preferencesFailureController(t *testing.T) *client.Client {
	t.Helper()
	snapshot := protocol.DefaultSnapshot()
	snapshot.Node = protocol.NodeID{1}
	controller := client.New(&conversationTestTransport{snapshot: snapshot})
	if err := controller.Start(t.Context()); err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = controller.Close() })
	return controller
}

func TestStartupPreferencesSaveFailureIsNotReplayedByAnotherWindow(t *testing.T) {
	profile := t.TempDir()
	stop, err := logging.Install(filepath.Join(profile, "log-profile"), "desktop")
	if err != nil {
		t.Fatal(err)
	}
	defer stop()
	var records []logging.Fields
	logging.SetReport(func(_ slog.Level, fields logging.Fields) {
		if fields["operation"] == "preferences_save_failed" || fields["operation"] == "preferences_read_failed" {
			records = append(records, fields)
		}
	})
	defer logging.SetReport(nil)
	prefs := preferences.ForStartup(profile)
	prefs.Data.ArtifactDrafts = &map[string]preferences.ArtifactDraft{"draft": {Form: "form", Fields: []preferences.ArtifactField{{Name: "text", Values: []string{"unsaved input"}}}}}
	wantDrafts := encoded(prefs.Data.ArtifactDrafts)
	if err := os.WriteFile(filepath.Join(profile, "desktop"), []byte("block directory creation"), 0o600); err != nil {
		t.Fatal(err)
	}
	controller := preferencesFailureController(t)
	version := uint64(0)
	for index := range 2 {
		r, err := NewWithPreferences(t.Context(), controller, prefs)
		if err != nil {
			t.Fatal(err)
		}
		t.Cleanup(func() { _ = r.Close() })
		window := closeTestWindow(r)
		window.Frame()
		window.Frame()
		if window.HasText(i18n.In("en", "preferences_read_failed")) || window.HasText(i18n.In("en", "preferences_save_failed")) != (index == 0) {
			t.Fatalf("window %d showed the wrong preferences error: %q", index+1, window.Texts())
		}
		if index == 0 {
			version = prefs.Revision()
		} else if prefs.Revision() != version {
			t.Fatal("the second window retried the failed startup write")
		}
	}
	if !prefs.Readable || encoded(prefs.Data.ArtifactDrafts) != wantDrafts {
		t.Fatal("a failed startup save discarded editable drafts or made settings unreadable")
	}
	if len(records) != 1 || records[0]["operation"] != "preferences_save_failed" || records[0]["stage"] != "create_directory" {
		t.Fatalf("startup preference failure records = %v", records)
	}
}

func TestUnreadablePreferencesErrorRemainsInEachWindow(t *testing.T) {
	path := filepath.Join(t.TempDir(), "preferences.json")
	original := []byte("unreadable preferences")
	if err := os.WriteFile(path, original, 0o600); err != nil {
		t.Fatal(err)
	}
	prefs := preferences.Open(path)
	readErr := prefs.Err
	if prefs.Readable || readErr == nil {
		t.Fatal("fixture is readable")
	}
	prefs.Data.Terminal.FontSize = 20
	controller := preferencesFailureController(t)
	for index := range 2 {
		r, err := NewWithPreferences(t.Context(), controller, prefs)
		if err != nil {
			t.Fatal(err)
		}
		t.Cleanup(func() { _ = r.Close() })
		window := closeTestWindow(r)
		window.Frame()
		window.Frame()
		if !window.HasText(i18n.In("en", "preferences_read_failed")) || window.HasText(i18n.In("en", "preferences_save_failed")) {
			t.Fatalf("window %d lost the true preferences read error: %q", index+1, window.Texts())
		}
	}
	if prefs.Readable || prefs.Err != readErr || prefs.Data.Terminal.FontSize != 20 {
		t.Fatal("opening windows lost the original read error or editable draft")
	}
	got, err := os.ReadFile(path)
	if err != nil || string(got) != string(original) {
		t.Fatalf("opening windows overwrote unreadable preferences: %q, %v", got, err)
	}
}

func TestPreferenceSaveDetectsNestedEdits(t *testing.T) {
	store := preferences.Open(filepath.Join(t.TempDir(), "preferences.json"))
	fields := []preferences.ArtifactField{{Name: "text", Values: []string{"initial"}}}
	store.Data.ArtifactDrafts = &map[string]preferences.ArtifactDraft{"draft": {Form: "form", Fields: fields}}
	r, err := NewWithPreferences(t.Context(), preferencesFailureController(t), store)
	if err != nil {
		t.Fatal(err)
	}
	defer r.Close()
	r.savePreferences()
	r.workers.Wait()
	before := store.Revision()
	fields[0].Values[0] = "edited"
	r.savePreferences()
	r.workers.Wait()
	reopened := preferences.Open(store.Path)
	if reopened.Err != nil || reopened.Data.ArtifactDrafts == nil || (*reopened.Data.ArtifactDrafts)["draft"].Fields[0].Values[0] != "edited" {
		t.Fatalf("nested preference edit was not persisted: %v", reopened.Err)
	}
	for range 3 {
		r.savePreferences()
	}
	if store.Revision() != before+1 {
		t.Fatalf("one nested edit advanced revision from %d to %d", before, store.Revision())
	}
}
