package node

import (
	"bytes"
	"context"
	"database/sql"
	"encoding/hex"
	"io"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/jonboulle/clockwork"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
	"lukechampine.com/blake3"
)

const backupFixturePassphrase = "correct horse battery"

func backupFixtureNode(t *testing.T, path string, clock clockwork.Clock) *Node {
	t.Helper()
	node, err := StartWithOptions(t.Context(), path, Options{DispatchClock: clock})
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = node.Stop(context.Background()) })
	return node
}

func backupFixtureKeep() protocol.PluginSettingsSecretUpdate {
	return protocol.PluginSettingsSecretUpdate{Kind: "keep"}
}

// backupFixtureSave stores settings over the current revision.
func backupFixtureSave(t *testing.T, node *Node, change func(*protocol.BackupUpdate)) link.Response {
	t.Helper()
	current := commandFixture(t, node, protocol.Command{Kind: "read_backup_settings"}).Data.(protocol.BackupSettings)
	update := protocol.BackupUpdate{Settings: current, Identity: backupFixtureKeep(), Secret: backupFixtureKeep(), SessionToken: backupFixtureKeep(), Passphrase: backupFixtureKeep()}
	change(&update)
	return dispatchFixture(t, node, node.ID(), protocol.NewRequest(node.ID(), protocol.Command{Kind: "save_backup_settings", Data: update}))
}

func backupFixtureConfigure(t *testing.T, node *Node, directory string) protocol.BackupSettings {
	t.Helper()
	saved := backupFixtureSave(t, node, func(update *protocol.BackupUpdate) {
		update.Settings.Local = &protocol.BackupLocal{Directory: directory, Enabled: true}
		update.Passphrase = protocol.PluginSettingsSecretUpdate{Kind: "replace", Data: protocol.NewSecret(backupFixturePassphrase)}
	})
	if saved.Fault != nil {
		t.Fatal(saved.Fault)
	}
	return saved.Output.Data.(protocol.BackupSettings)
}

// backupFixtureDispatch reports a refusal before admission like a completed fault.
func backupFixtureDispatch(t *testing.T, node *Node, command protocol.Command) link.Response {
	t.Helper()
	accepted, err := node.Dispatch(t.Context(), node.ID(), protocol.NewRequest(node.ID(), command))
	if err != nil {
		failure, ok := err.(*protocol.Fault)
		if !ok {
			t.Fatal(err)
		}
		return link.Response{Fault: failure}
	}
	return <-accepted.Completion
}

func backupFixtureCode(value link.Response) protocol.ErrorCode {
	if value.Fault == nil {
		return ""
	}
	return value.Fault.Code
}

// Over 1 s: a real export and restore archive and flush the profile's bundled packages (about 690 files).
func TestBackupRoundTrip(t *testing.T) {
	t.Parallel()
	profile := filepath.Join(t.TempDir(), "profile")
	directory := filepath.Join(t.TempDir(), "backups")
	node := backupFixtureNode(t, profile, nil)

	initial := commandFixture(t, node, protocol.Command{Kind: "read_backup_settings"}).Data.(protocol.BackupSettings)
	if initial.Revision != 0 || initial.Local != nil || initial.Destination != nil || initial.PassphraseConfigured {
		t.Fatalf("initial settings = %#v", initial)
	}
	for _, test := range []struct {
		name   string
		change func(*protocol.BackupUpdate)
		code   protocol.ErrorCode
	}{
		{"short passphrase", func(update *protocol.BackupUpdate) {
			update.Passphrase = protocol.PluginSettingsSecretUpdate{Kind: "replace", Data: protocol.NewSecret("short")}
		}, protocol.ErrorCodeInvalidRequest},
		{"relative directory", func(update *protocol.BackupUpdate) {
			update.Settings.Local = &protocol.BackupLocal{Directory: "backups", Enabled: true}
		}, protocol.ErrorCodeInvalidRequest},
		{"directory inside the profile", func(update *protocol.BackupUpdate) {
			update.Settings.Local = &protocol.BackupLocal{Directory: filepath.Join(profile, "backups"), Enabled: true}
		}, protocol.ErrorCodePermissionDenied},
		{"directory around the profile", func(update *protocol.BackupUpdate) {
			update.Settings.Local = &protocol.BackupLocal{Directory: filepath.Dir(profile), Enabled: true}
		}, protocol.ErrorCodePermissionDenied},
		{"stale revision", func(update *protocol.BackupUpdate) { update.Settings.Revision = 7 }, protocol.ErrorCodeRevisionConflict},
		{"endpoint credentials", func(update *protocol.BackupUpdate) {
			update.Settings.Destination = &protocol.BackupDestination{Kind: protocol.BackupKindWebdav, Endpoint: "https://user:secret@example.test/dav"}
		}, protocol.ErrorCodeInvalidRequest},
		{"schedule without destination", func(update *protocol.BackupUpdate) {
			update.Settings.Schedule = &protocol.BackupSchedule{Enabled: true, Timezone: "UTC", Hour: 3}
		}, protocol.ErrorCodeInvalidRequest},
	} {
		if saved := backupFixtureSave(t, node, test.change); backupFixtureCode(saved) != test.code {
			t.Errorf("%s: %#v", test.name, saved)
		}
	}
	exported := backupFixtureDispatch(t, node, protocol.Command{Kind: "export_backup", Data: protocol.CommandExportBackupPayload{}})
	if backupFixtureCode(exported) != protocol.ErrorCodeNotConfigured {
		t.Fatalf("export without a destination = %#v", exported)
	}

	settings := backupFixtureConfigure(t, node, directory)
	if settings.Revision != 1 || !settings.PassphraseConfigured || settings.CredentialsConfigured || settings.Local.Directory != directory {
		t.Fatalf("saved settings = %#v", settings)
	}
	snapshot := snapshotFixture(t, node)
	var body []byte
	if err := node.profile.Store.Node.Read(t.Context(), func(db *sql.Conn) error {
		return db.QueryRowContext(t.Context(), "SELECT body FROM events ORDER BY cursor DESC LIMIT 1").Scan(&body)
	}); err != nil {
		t.Fatal(err)
	}
	if !bytes.Contains(body, []byte("backup_settings_changed")) || bytes.Contains(body, []byte(backupFixturePassphrase)) || snapshot.Cursor == 0 {
		t.Fatalf("settings event = %s", body)
	}

	request := protocol.NewRequest(node.ID(), protocol.Command{Kind: "export_backup", Data: protocol.CommandExportBackupPayload{ExpectedRevision: settings.Revision}})
	first := dispatchFixture(t, node, node.ID(), request)
	if first.Fault != nil {
		t.Fatal(first.Fault)
	}
	record := first.Output.Data.(protocol.BackupRecord)
	if record.ID != request.ID.String() || record.Node != node.ID() || record.SizeBytes == 0 {
		t.Fatalf("record = %#v", record)
	}
	// The same request identifier replays its recorded outcome without a second archive.
	replayed := dispatchFixture(t, node, node.ID(), request)
	entries, err := os.ReadDir(directory)
	if replayed.Fault != nil || err != nil || len(entries) != 2 {
		t.Fatalf("replay = %#v, entries = %v %v", replayed, entries, err)
	}
	page := commandFixture(t, node, protocol.Command{Kind: "list_backups"}).Data.(protocol.BackupPage)
	if len(page.Records) != 1 || page.Records[0] != record || !page.Locations[record.ID].Local || page.Locations[record.ID].Online {
		t.Fatalf("backups = %#v", page)
	}
	// The Node keeps serving its reopened databases after the snapshot.
	if again := commandFixture(t, node, protocol.Command{Kind: "read_backup_settings"}).Data.(protocol.BackupSettings); again != settings && again.Revision != settings.Revision {
		t.Fatalf("settings after export = %#v", again)
	}
	for _, name := range []string{record.ID + ".json", filepath.Join(record.ID, "000000.bin")} {
		content, err := os.ReadFile(filepath.Join(directory, name))
		if err != nil || bytes.Contains(content, []byte(backupFixturePassphrase)) {
			t.Fatalf("%s: %v", name, err)
		}
	}
	leftovers, err := filepath.Glob(filepath.Join(profile, "tmp", "backup-*"))
	if err != nil || len(leftovers) != 0 {
		t.Fatalf("snapshot staging remains: %v %v", leftovers, err)
	}

	target := filepath.Join(t.TempDir(), "restored")
	restored := dispatchFixture(t, node, node.ID(), protocol.NewRequest(node.ID(), protocol.Command{Kind: "restore_backup", Data: protocol.CommandRestoreBackupPayload{
		ID: record.ID, ExpectedRevision: settings.Revision, Target: target, Location: protocol.BackupLocationLocal,
	}}))
	if restored.Fault != nil {
		t.Fatal(restored.Fault)
	}
	if output := restored.Output.Data.(protocol.OutputBackupRestoredPayload); output.Record != record || output.Target != target {
		t.Fatalf("restored = %#v", output)
	}
	// The restored directory is a complete profile of the same Node.
	copy := backupFixtureNode(t, target, nil)
	if copy.ID() != node.ID() {
		t.Fatal("restored identity differs")
	}
	if value := commandFixture(t, copy, protocol.Command{Kind: "read_backup_settings"}).Data.(protocol.BackupSettings); value.Revision != settings.Revision || value.Local == nil || value.Local.Directory != directory || !value.PassphraseConfigured {
		t.Fatalf("restored settings = %#v", value)
	}

	t.Run("a partial backup keeps its local copy and replays its failure", func(t *testing.T) {
		server := httptest.NewServer(http.HandlerFunc(func(writer http.ResponseWriter, request *http.Request) {
			writer.WriteHeader(http.StatusForbidden)
		}))
		t.Cleanup(server.Close)
		saved := backupFixtureSave(t, node, func(update *protocol.BackupUpdate) {
			update.Settings.Destination = &protocol.BackupDestination{Kind: protocol.BackupKindWebdav, Endpoint: server.URL}
			update.Settings.OnlineEnabled, update.Settings.Keep = true, 1
		})
		if saved.Fault != nil {
			t.Fatal(saved.Fault)
		}
		revision := saved.Output.Data.(protocol.BackupSettings).Revision
		request := protocol.NewRequest(node.ID(), protocol.Command{Kind: "export_backup", Data: protocol.CommandExportBackupPayload{ExpectedRevision: revision}})
		first := dispatchFixture(t, node, node.ID(), request)
		if first.Fault == nil || first.Fault.Code != protocol.ErrorCodePermissionDenied || first.Fault.Details == nil || first.Fault.Details.Kind != "backup_partial" {
			t.Fatalf("partial backup lost its outcome: %#v", first)
		}
		// The successful local destination is pruned even though online export
		// failed; its new copy remains available to restore.
		page := commandFixture(t, node, protocol.Command{Kind: "list_backups"}).Data.(protocol.BackupPage)
		if len(page.Records) != 1 || page.Records[0].ID != request.ID.String() || page.Locations[request.ID.String()] != (protocol.BackupLocations{Local: true}) {
			t.Fatalf("partial export skipped local retention: %#v", page)
		}
		replayed := dispatchFixture(t, node, node.ID(), request)
		entries, err := os.ReadDir(directory)
		if replayed.Fault == nil || replayed.Fault.Details == nil || replayed.Fault.Details.Kind != "backup_partial" || err != nil || len(entries) != 2 {
			t.Fatalf("partial replay created another archive: %#v, %v, %v", replayed, entries, err)
		}
		outcome := commandFixture(t, node, protocol.Command{Kind: "read_backup_settings"}).Data.(protocol.BackupSettings).Last
		if outcome == nil || outcome.Error == nil || *outcome.Error != protocol.ErrorCodePermissionDenied || outcome.Details == nil || outcome.Details.Kind != "backup_partial" {
			t.Fatalf("an unattended partial backup lost its saved copy: %#v", outcome)
		}
	})
}

// Over 1 s: a real download and file restore archive and flush the profile's bundled packages (about 690 files).
func TestRestoreIntoEmptyProfile(t *testing.T) {
	t.Parallel()
	profile := filepath.Join(t.TempDir(), "profile")
	node := backupFixtureNode(t, profile, nil)
	settings := backupFixtureConfigure(t, node, filepath.Join(t.TempDir(), "backups"))

	// Download the sealed archive through its single-use stream.
	download := commandFixture(t, node, protocol.Command{Kind: "download_backup", Data: protocol.CommandDownloadBackupPayload{ExpectedRevision: settings.Revision}}).Data.(protocol.BackupDownload)
	stream, err := node.Open(t.Context(), node.ID(), download.Stream)
	if err != nil {
		t.Fatal(err)
	}
	sealed, err := io.ReadAll(stream)
	_ = stream.Close()
	digest := blake3.Sum256(sealed)
	if err != nil || uint64(len(sealed)) != download.Size || hex.EncodeToString(digest[:]) != download.Revision || download.Record.SizeBytes != download.Size {
		t.Fatalf("download = %#v, %d bytes, %v", download, len(sealed), err)
	}
	if !bytes.HasPrefix(sealed, []byte("age-encryption.org/v1\n")) {
		t.Fatal("downloaded archive is not encrypted")
	}
	if _, err := node.Open(t.Context(), node.ID(), download.Stream); err == nil {
		t.Fatal("a backup stream opened twice")
	}

	upload := func() protocol.StreamID {
		t.Helper()
		reserved := commandFixture(t, node, protocol.Command{Kind: "upload_backup", Data: protocol.CommandUploadBackupPayload{Size: download.Size, Revision: download.Revision}}).Data.(protocol.BackupUpload)
		stream, err := node.Open(t.Context(), node.ID(), reserved.Stream)
		if err != nil {
			t.Fatal(err)
		}
		defer stream.Close()
		if _, err := stream.Write(sealed); err != nil {
			t.Fatal(err)
		}
		if err := stream.CloseWrite(); err != nil {
			t.Fatal(err)
		}
		ready, err := io.ReadAll(stream)
		if err != nil || !bytes.Equal(ready, []byte{protocol.FILE_UPLOAD_READY}) {
			t.Fatalf("upload acknowledgement = %v %v", ready, err)
		}
		return reserved.Stream
	}
	restore := func(stream protocol.StreamID, target string, passphrase *protocol.Secret) link.Response {
		t.Helper()
		return backupFixtureDispatch(t, node, protocol.Command{Kind: "restore_backup_file", Data: protocol.CommandRestoreBackupFilePayload{
			Stream: stream, ExpectedRevision: settings.Revision, Target: target, Passphrase: passphrase,
		}})
	}

	occupied := filepath.Join(t.TempDir(), "occupied")
	if err := os.MkdirAll(occupied, 0o700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(occupied, "note"), []byte("mine"), 0o600); err != nil {
		t.Fatal(err)
	}
	file := filepath.Join(t.TempDir(), "file")
	if err := os.WriteFile(file, nil, 0o600); err != nil {
		t.Fatal(err)
	}
	wrong := protocol.NewSecret("incorrect horse battery")
	for _, test := range []struct {
		name       string
		target     string
		passphrase *protocol.Secret
		code       protocol.ErrorCode
	}{
		{"relative target", "restored", nil, protocol.ErrorCodeInvalidRequest},
		{"traversal", filepath.Join(t.TempDir(), "a") + string(filepath.Separator) + ".." + string(filepath.Separator) + "b", nil, protocol.ErrorCodeInvalidRequest},
		{"occupied directory", occupied, nil, protocol.ErrorCodeConflict},
		{"regular file", file, nil, protocol.ErrorCodeConflict},
		{"live profile", profile, nil, protocol.ErrorCodeConflict},
		{"inside the live profile", filepath.Join(profile, "restored"), nil, protocol.ErrorCodePermissionDenied},
		{"around the live profile", filepath.Dir(profile), nil, protocol.ErrorCodeConflict},
	} {
		if refused := restore(protocol.NewStreamID(), test.target, test.passphrase); backupFixtureCode(refused) != test.code {
			t.Errorf("%s: %#v", test.name, refused)
		}
	}
	if content, err := os.ReadFile(filepath.Join(occupied, "note")); err != nil || string(content) != "mine" {
		t.Fatalf("occupied directory changed: %q %v", content, err)
	}

	target := filepath.Join(t.TempDir(), "empty")
	if err := os.Mkdir(target, 0o700); err != nil {
		t.Fatal(err)
	}
	if refused := restore(upload(), target, &wrong); backupFixtureCode(refused) != protocol.ErrorCodeInvalidRequest || strings.Contains(refused.Fault.Message, wrong.Expose()) {
		t.Fatalf("wrong passphrase = %#v", refused)
	}
	if remaining, err := os.ReadDir(target); err != nil || len(remaining) != 0 {
		t.Fatalf("failed restore wrote to its target: %v %v", remaining, err)
	}
	if missing := restore(protocol.NewStreamID(), target, nil); backupFixtureCode(missing) != protocol.ErrorCodeNotFound {
		t.Fatalf("unknown upload = %#v", missing)
	}

	restored := restore(upload(), target, nil)
	if restored.Fault != nil {
		t.Fatal(restored.Fault)
	}
	output := restored.Output.Data.(protocol.OutputBackupRestoredPayload)
	if output.Target != target || output.Record.Node != node.ID() || output.Record.SizeBytes != download.Size || output.Record.Files != download.Record.Files {
		t.Fatalf("restored = %#v", output)
	}
	for _, name := range []string{"link.key", filepath.Join("storage", "node.sqlite")} {
		info, err := os.Stat(filepath.Join(target, name))
		if err != nil || !info.Mode().IsRegular() {
			t.Fatalf("%s: %v", name, err)
		}
	}
	siblings, err := filepath.Glob(filepath.Join(filepath.Dir(target), ".sailry-restore-*"))
	staging, _ := filepath.Glob(filepath.Join(profile, "tmp", "*"))
	if err != nil || len(siblings) != 0 || len(staging) != 0 {
		t.Fatalf("restore staging remains: %v %v %v", siblings, staging, err)
	}
	// The live profile keeps running, and the restored one opens on its own.
	if value := commandFixture(t, node, protocol.Command{Kind: "read_backup_settings"}).Data.(protocol.BackupSettings); value.Revision != settings.Revision {
		t.Fatalf("live settings = %#v", value)
	}
	copy := backupFixtureNode(t, target, nil)
	if value := commandFixture(t, copy, protocol.Command{Kind: "read_backup_settings"}).Data.(protocol.BackupSettings); copy.ID() != node.ID() || value.Revision != settings.Revision || !value.PassphraseConfigured {
		t.Fatalf("restored settings = %#v", value)
	}
	if again := restore(upload(), target, nil); backupFixtureCode(again) != protocol.ErrorCodeConflict {
		t.Fatalf("restore over a profile = %#v", again)
	}
}

// Over 1 s: the scheduled run is a real export of the profile's bundled packages (about 690 files).
func TestScheduleNextRun(t *testing.T) {
	t.Parallel()
	start := time.Date(2026, time.March, 7, 12, 0, 0, 0, time.UTC)
	clock := clockwork.NewFakeClockAt(start)
	directory := filepath.Join(t.TempDir(), "backups")
	node := backupFixtureNode(t, filepath.Join(t.TempDir(), "profile"), clock)
	backupFixtureConfigure(t, node, directory)

	// next reads the due time the dispatcher waits for; nil means nothing is due.
	next := func() *int64 {
		t.Helper()
		var due sql.NullInt64
		if err := node.profile.Store.Node.Read(t.Context(), func(db *sql.Conn) error {
			return db.QueryRowContext(t.Context(), "SELECT next_ms FROM dispatch_schedules WHERE package='' AND id='40ca706e-25c1-45ca-bacf-61ee028fb608'").Scan(&due)
		}); err != nil {
			t.Fatal(err)
		}
		if !due.Valid {
			return nil
		}
		return &due.Int64
	}
	for _, test := range []struct {
		name     string
		schedule protocol.BackupSchedule
		code     protocol.ErrorCode
		next     string
	}{
		{"later today", protocol.BackupSchedule{Enabled: true, Timezone: "UTC", Hour: 13}, "", "2026-03-07T13:00:00Z"},
		{"already passed today", protocol.BackupSchedule{Enabled: true, Timezone: "UTC", Hour: 11, Minute: 45}, "", "2026-03-08T11:45:00Z"},
		{"the current minute", protocol.BackupSchedule{Enabled: true, Timezone: "UTC", Hour: 12}, "", "2026-03-08T12:00:00Z"},
		{"east of UTC", protocol.BackupSchedule{Enabled: true, Timezone: "Asia/Shanghai", Hour: 8, Minute: 30}, "", "2026-03-08T00:30:00Z"},
		{"west of UTC", protocol.BackupSchedule{Enabled: true, Timezone: "America/New_York", Hour: 9}, "", "2026-03-07T14:00:00Z"},
		{"after a daylight saving change", protocol.BackupSchedule{Enabled: true, Timezone: "America/New_York", Hour: 6}, "", "2026-03-08T10:00:00Z"},
		{"disabled", protocol.BackupSchedule{Timezone: "UTC", Hour: 13}, "", ""},
		{"unknown timezone", protocol.BackupSchedule{Enabled: true, Timezone: "Mars/Olympus", Hour: 13}, protocol.ErrorCodeInvalidRequest, ""},
		{"hour out of range", protocol.BackupSchedule{Enabled: true, Timezone: "UTC", Hour: 24}, protocol.ErrorCodeInvalidRequest, ""},
		{"minute out of range", protocol.BackupSchedule{Enabled: true, Timezone: "UTC", Minute: 60}, protocol.ErrorCodeInvalidRequest, ""},
	} {
		saved := backupFixtureSave(t, node, func(update *protocol.BackupUpdate) { update.Settings.Schedule = &test.schedule })
		if backupFixtureCode(saved) != test.code {
			t.Fatalf("%s: %#v", test.name, saved)
		}
		if test.code != "" {
			continue
		}
		due := next()
		switch {
		case test.next == "" && due != nil:
			t.Errorf("%s: due at %s", test.name, time.UnixMilli(*due).UTC())
		case test.next != "" && (due == nil || time.UnixMilli(*due).UTC().Format(time.RFC3339) != test.next):
			t.Errorf("%s: due = %v, want %s", test.name, due, test.next)
		}
	}

	// The scheduled run exports with the saved configuration and advances a day.
	saved := backupFixtureSave(t, node, func(update *protocol.BackupUpdate) {
		update.Settings.Schedule = &protocol.BackupSchedule{Enabled: true, Timezone: "UTC", Hour: 12, Minute: 30}
	})
	if saved.Fault != nil {
		t.Fatal(saved.Fault)
	}
	if page := commandFixture(t, node, protocol.Command{Kind: "list_backups"}).Data.(protocol.BackupPage); len(page.Records) != 0 {
		t.Fatalf("backups before the schedule = %#v", page)
	}
	clock.Advance(29 * time.Minute)
	if page := commandFixture(t, node, protocol.Command{Kind: "list_backups"}).Data.(protocol.BackupPage); len(page.Records) != 0 {
		t.Fatalf("backups before the due time = %#v", page)
	}
	deadline := time.Now().Add(30 * time.Second)
	for {
		// The dispatcher may be between timers; each step lets a new one expire.
		clock.Advance(time.Second)
		page := commandFixture(t, node, protocol.Command{Kind: "list_backups"}).Data.(protocol.BackupPage)
		if len(page.Records) == 1 {
			break
		}
		if len(page.Records) > 1 || time.Now().After(deadline) {
			t.Fatalf("scheduled backups = %#v", page)
		}
		time.Sleep(20 * time.Millisecond)
	}
	if due := next(); due == nil || time.UnixMilli(*due).UTC().Format(time.RFC3339) != "2026-03-08T12:30:00Z" {
		t.Fatalf("next run after the scheduled export = %v", due)
	}

	// The outcome of a backup nobody watched is kept for the Backup page.
	read := func() protocol.BackupSettings {
		t.Helper()
		return commandFixture(t, node, protocol.Command{Kind: "read_backup_settings"}).Data.(protocol.BackupSettings)
	}
	last := func() protocol.BackupOutcome {
		t.Helper()
		deadline := time.Now().Add(30 * time.Second)
		for {
			if settings := read(); settings.Last != nil {
				return *settings.Last
			}
			if time.Now().After(deadline) {
				t.Fatal("the scheduled backup recorded no outcome")
			}
			time.Sleep(20 * time.Millisecond)
		}
	}
	if outcome := last(); outcome.Error != nil || outcome.Reason != "" || outcome.FinishedMs < uint64(start.UnixMilli()) {
		t.Fatalf("outcome of the scheduled backup = %#v", outcome)
	}
	// Retention is a setting; saving one keeps the recorded outcome.
	kept := backupFixtureSave(t, node, func(update *protocol.BackupUpdate) {
		update.Settings.Keep, update.Settings.Last = 1, nil
	})
	if kept.Fault != nil || kept.Output.Data.(protocol.BackupSettings).Keep != 1 || kept.Output.Data.(protocol.BackupSettings).Last == nil {
		t.Fatalf("retention save = %#v", kept)
	}
	export := func() link.Response {
		t.Helper()
		return backupFixtureDispatch(t, node, protocol.Command{Kind: "export_backup", Data: protocol.CommandExportBackupPayload{ExpectedRevision: read().Revision}})
	}
	manual := export()
	if manual.Fault != nil {
		t.Fatal(manual.Fault)
	}
	if page := commandFixture(t, node, protocol.Command{Kind: "list_backups"}).Data.(protocol.BackupPage); len(page.Records) != 1 || page.Records[0].ID != manual.Output.Data.(protocol.BackupRecord).ID {
		t.Fatalf("backups kept with a limit of one = %#v", page)
	}
	// A failed backup says so, with its reason.
	if err := os.RemoveAll(directory); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(directory, []byte("not a folder"), 0o600); err != nil {
		t.Fatal(err)
	}
	if failed := export(); failed.Fault == nil {
		t.Fatalf("export into a file = %#v", failed)
	}
	if outcome := *read().Last; outcome.Error == nil || outcome.Reason == "" {
		t.Fatalf("outcome of the failed backup = %#v", outcome)
	}
}
