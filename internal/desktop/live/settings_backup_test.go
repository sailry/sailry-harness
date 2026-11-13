package live

import (
	"encoding/json"
	"errors"
	"reflect"
	"strings"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// checkBackupSettingsHelpers covers the page's projection of a Node's
// backup settings and the requests its drafts become.
func checkBackupSettingsHelpers(t *testing.T) {
	t.Helper()
	empty := protocol.BackupSettings{Revision: 1}
	if target := backupTarget(empty); target.Tab != 0 || target.Prefix != "sailry-backups" || target.Timezone != "UTC" || target.Time.Hour() != 3 || target.Schedule || target.PassphraseSet {
		t.Errorf("unconfigured target = %+v", target)
	}
	saved := protocol.BackupSettings{Revision: 7, CredentialsConfigured: true, PassphraseConfigured: true, OnlineEnabled: true,
		Destination: &protocol.BackupDestination{Kind: protocol.BackupKindS3, Endpoint: "https://s3.example.com", Root: "backups", Bucket: "profile", Region: "auto"},
		Local:       &protocol.BackupLocal{Directory: "/srv/backups"},
		Schedule:    &protocol.BackupSchedule{Enabled: true, Timezone: "Asia/Shanghai", Hour: 4, Minute: 30}}
	target := backupTarget(saved)
	want := settings.BackupTarget{Tab: 1, Endpoint: "https://s3.example.com", Prefix: "backups", Bucket: "profile", Region: "auto", Directory: "/srv/backups",
		Schedule: true, Time: target.Time, Timezone: "Asia/Shanghai", PassphraseSet: true, CredentialsSet: true}
	if target != want || target.Time.Hour() != 4 || target.Time.Minute() != 30 {
		t.Errorf("target = %+v", target)
	}
	local := saved
	local.OnlineEnabled, local.Local = false, &protocol.BackupLocal{Directory: "/srv/backups", Enabled: true}
	if backupStorageTab(local) != 2 {
		t.Error("a local-only target is not the local tab")
	}

	// An untouched draft closes without a request; each edit is one.
	if backupStorageDirty(target, saved) {
		t.Error("the saved target reads as dirty")
	}
	for name, edit := range map[string]func(*settings.BackupTarget){
		"endpoint":    func(x *settings.BackupTarget) { x.Endpoint = "https://other.example.com" },
		"tab":         func(x *settings.BackupTarget) { x.Tab = 2 },
		"secret":      func(x *settings.BackupTarget) { x.Secret = "typed" },
		"passphrase":  func(x *settings.BackupTarget) { x.Passphrase, x.Confirm = "new passphrase", "new passphrase" },
		"credentials": func(x *settings.BackupTarget) { x.CredentialsSet = false },
		"directory":   func(x *settings.BackupTarget) { x.Directory = "" },
	} {
		draft := target
		edit(&draft)
		if !backupStorageDirty(draft, saved) {
			t.Errorf("%s edit reads as clean", name)
		}
	}

	// Blank secrets keep the stored ones, typed ones replace them, and
	// the schedule is left as saved.
	kind := func(update protocol.BackupUpdate) [4]string {
		return [4]string{update.Identity.Kind, update.Secret.Kind, update.SessionToken.Kind, update.Passphrase.Kind}
	}
	draft := target
	draft.Endpoint, draft.Secret, draft.Passphrase = " https://other.example.com ", "typed", "new passphrase"
	draft.Schedule = false
	update := backupStorageUpdate(draft, saved)
	if kind(update) != [4]string{"keep", "replace", "keep", "replace"} || update.Settings.Revision != 7 ||
		update.Settings.Destination.Endpoint != "https://other.example.com" || update.Settings.Destination.Kind != protocol.BackupKindS3 ||
		!update.Settings.OnlineEnabled || update.Settings.Local == nil || update.Settings.Local.Enabled || !reflect.DeepEqual(update.Settings.Schedule, saved.Schedule) {
		t.Errorf("storage update = %+v", update)
	}
	if secret, ok := update.Secret.Data.(protocol.Secret); !ok || secret.Expose() != "typed" {
		t.Errorf("secret update = %#v", update.Secret.Data)
	}
	if saved.Destination.Endpoint != "https://s3.example.com" {
		t.Error("the update changed the saved settings")
	}
	// Cleared credentials are removed; WebDAV never keeps a session token.
	draft = target
	draft.Tab, draft.CredentialsSet = 0, false
	if update = backupStorageUpdate(draft, saved); kind(update) != [4]string{"clear", "clear", "clear", "keep"} || update.Settings.Destination.Kind != protocol.BackupKindWebdav {
		t.Errorf("cleared update = %+v", update)
	}
	// The local folder alone turns the online target off and keeps it stored.
	draft = target
	draft.Tab = 2
	if update = backupStorageUpdate(draft, saved); update.Settings.OnlineEnabled || update.Settings.Local == nil || !update.Settings.Local.Enabled ||
		!reflect.DeepEqual(update.Settings.Destination, saved.Destination) {
		t.Errorf("local update = %+v", update)
	}
	// The schedule is saved alone with every secret kept, and the request
	// carries no secret.
	schedule := backupScheduleUpdate(backupSchedule{enabled: true, hour: 23, minute: 5, timezone: "Europe/Berlin"}, saved)
	if kind(schedule) != [4]string{"keep", "keep", "keep", "keep"} || *schedule.Settings.Schedule != (protocol.BackupSchedule{Enabled: true, Timezone: "Europe/Berlin", Hour: 23, Minute: 5}) ||
		saved.Schedule.Hour != 4 || !reflect.DeepEqual(schedule.Settings.Destination, saved.Destination) {
		t.Errorf("schedule update = %+v", schedule)
	}
	if encoded, err := json.Marshal(protocol.Command{Kind: "save_backup_settings", Data: schedule}); err != nil || strings.Contains(string(encoded), "replace") {
		t.Errorf("schedule request = %s, %v", encoded, err)
	}
	// Retention is saved alone too; the Node's record of its last backup is
	// shown with the problem a failure's code stands for.
	if retention := backupKeepUpdate(7, saved); kind(retention) != [4]string{"keep", "keep", "keep", "keep"} || retention.Settings.Keep != 7 ||
		retention.Settings.Revision != 7 || saved.Keep != 0 || !reflect.DeepEqual(retention.Settings.Schedule, saved.Schedule) {
		t.Errorf("retention update = %+v", retention)
	}
	denied := protocol.ErrorCodePermissionDenied
	ended := saved
	ended.Keep, ended.Last = 14, &protocol.BackupOutcome{FinishedMs: 1_700_000_000_000, Error: &denied, Reason: "backup folder is not writable"}
	if got := backupTarget(ended); got.Keep != 14 || got.Last != (settings.BackupLast{At: time.UnixMilli(1_700_000_000_000), Problem: "backup_denied", Reason: "backup folder is not writable"}) {
		t.Errorf("failed backup = %+v", got.Last)
	}
	ended.Last = &protocol.BackupOutcome{FinishedMs: 1_700_000_000_000, Skipped: 3}
	if got := backupTarget(ended).Last; got.Problem != "" || got.Skipped != 3 || got.At.IsZero() {
		t.Errorf("finished backup = %+v", got)
	}
	edited := target
	edited.Time, edited.Timezone = time.Date(2026, 1, 1, 23, 5, 0, 0, time.Local), " Europe/Berlin "
	if got := backupScheduleOf(edited); got != (backupSchedule{enabled: true, hour: 23, minute: 5, timezone: "Europe/Berlin"}) {
		t.Errorf("schedule draft = %+v", got)
	}
	if backupPassphrase("") != nil || backupPassphrase("typed").Expose() != "typed" {
		t.Error("restore passphrase is wrong")
	}

	// Stored backups name the copies they can be restored from.
	records := backupRecords(protocol.BackupPage{Records: []protocol.BackupRecord{{ID: "a", CreatedMs: 1_700_000_000_000, SizeBytes: 5}, {ID: "b"}},
		Locations: map[string]protocol.BackupLocations{"a": {Local: true}}})
	if len(records) != 2 || !records[0].Local || records[0].Online || records[1].Local || !records[1].Online ||
		records[0].Size != 5 || !records[0].Created.Equal(time.UnixMilli(1_700_000_000_000)) {
		t.Errorf("records = %+v", records)
	}

	// Sessions: delegated ones follow their parents and are never named.
	finished := int64(1_700_000_000_000)
	delegation := &protocol.Delegation{}
	first, second, third, fourth := protocol.NewSessionID(), protocol.NewSessionID(), protocol.NewSessionID(), protocol.NewSessionID()
	sessions := []protocol.Session{
		{ID: first, Revision: 2},
		{ID: second, Revision: 3, Archived: true, Activity: protocol.ActivitySummary{Title: "Archived", Run: &protocol.ConversationRun{FinishedMs: &finished}}},
		{ID: third, Revision: 4, Archived: true, Project: &protocol.ProjectID{1}},
		{ID: fourth, Revision: 5, Delegation: &delegation},
	}
	archived, active := backupSessions(sessions, "Untitled")
	if active != 1 || len(archived) != 2 || archived[0].ID != second.String() || archived[0].Title != "Archived" || !archived[0].At.Equal(time.UnixMilli(finished)) ||
		archived[1].Title != "Untitled" || !archived[1].At.IsZero() || !archived[0].Private || archived[1].Private {
		t.Errorf("sessions = %+v, %d", archived, active)
	}
	if got := backupRevisions(sessions, false); !reflect.DeepEqual(got, []protocol.SessionRevision{{Session: first, Revision: 2}}) {
		t.Errorf("archive revisions = %+v", got)
	}
	if got := backupRevisions(sessions, true); len(got) != 3 || got[2].Session != third {
		t.Errorf("remove revisions = %+v", got)
	}
	if got := backupRevisions(nil, true); got == nil || len(got) != 0 {
		t.Errorf("empty revisions = %#v", got)
	}

	// Failure, conflict and uncertain outcomes are distinct toasts.
	fault := func(code protocol.ErrorCode) error { return &link.FaultError{Fault: protocol.Fault{Code: code}} }
	partial := func(code protocol.ErrorCode) *protocol.Fault {
		return &protocol.Fault{Code: code, Details: &protocol.FaultDetails{Kind: "backup_partial", Data: protocol.FaultDetailsBackupPartialPayload{}}}
	}
	for _, test := range []struct {
		err      error
		outcome  kit.Outcome
		key      string
		sessions bool
	}{
		{fault(protocol.ErrorCodeRevisionConflict), kit.Conflict, "backup_conflict", false},
		{fault(protocol.ErrorCodeConflict), kit.Conflict, "backup_conflict", false},
		{fault(protocol.ErrorCodeInvalidRequest), kit.Failed, "backup_invalid", false},
		{fault(protocol.ErrorCodePermissionDenied), kit.Failed, "backup_denied", false},
		{fault(protocol.ErrorCodeOutcomeUnknown), kit.Uncertain, "backup_unknown", false},
		{partial(protocol.ErrorCodeUnavailable), kit.Failed, "backup_online_failed", false},
		{partial(protocol.ErrorCodeOutcomeUnknown), kit.Uncertain, "backup_online_unknown", false},
		{&link.FaultError{Fault: *partial(protocol.ErrorCodeUnavailable)}, kit.Failed, "backup_online_failed", false},
		{&link.FaultError{Fault: *partial(protocol.ErrorCodeOutcomeUnknown)}, kit.Uncertain, "backup_online_unknown", false},
		{errors.New("disk"), kit.Failed, "backup_failed", false},
		{fault(protocol.ErrorCodeBusy), kit.Failed, "backup_sessions_busy", true},
		{fault(protocol.ErrorCodeNotFound), kit.Conflict, "session_changed", true},
		{fault(protocol.ErrorCodeOutcomeUnknown), kit.Uncertain, "backup_sessions_unknown", true},
		{fault(protocol.ErrorCodeUnavailable), kit.Failed, "backup_sessions_failed", true},
	} {
		failure := backupFailure
		if test.sessions {
			failure = backupSessionFailure
		}
		if outcome, key := failure(test.err); outcome != test.outcome || key != test.key {
			t.Errorf("%v = %v %q, want %v %q", test.err, outcome, key, test.outcome, test.key)
		}
	}

	// Closing the storage dialog on an unconfirmed save drops the request
	// kept for it, and no other: the same draft sent later is a new request.
	{
		r := &Runtime{App: shell.NewLive(&viewmodel.Data{Workspace: &viewmodel.Workspace{}}), uncertain: map[string]protocol.Request{}}
		state := &r.backupSettings
		state.shown, state.known = r, true
		state.actions = r.backupActions()
		storage, other := protocol.Command{Kind: "save_backup_settings", Data: backupStorageUpdate(target, saved)}, protocol.Command{Kind: "export_backup", Data: protocol.DefaultCommandExportBackupPayload()}
		key := func(command protocol.Command) string {
			encoded, err := json.Marshal(command)
			if err != nil {
				t.Fatal(err)
			}
			return "\x00" + string(encoded)
		}
		r.uncertain[key(storage)], r.uncertain[key(other)] = protocol.Request{}, protocol.Request{}
		state.storage, state.retry, state.actions.Retry = &storage, true, true
		state.actions.Forget()
		if _, kept := r.uncertain[key(storage)]; kept || len(r.uncertain) != 1 || state.retry || state.actions.Retry || state.storage != nil {
			t.Errorf("a closed storage dialog kept its request: %d kept, retry %v", len(r.uncertain), state.retry)
		}
		state.actions.Forget()
		if len(r.uncertain) != 1 {
			t.Error("a dialog without a kept request dropped another")
		}
	}

	// An upload whose publication the Node answered with something else is
	// over: the answer cannot change, so its request is not kept to be
	// asked about again.
	{
		r := &Runtime{App: shell.NewLive(&viewmodel.Data{Workspace: &viewmodel.Workspace{}})}
		item, record := &composer.Attachment{}, &attachmentRecord{running: true}
		r.attachments = &attachmentState{items: map[*composer.Attachment]*attachmentRecord{item: record}}
		pending, expected := protocol.Request{}, protocol.AttachmentSpec{Name: "notes.txt", Size: 4}
		r.finishAttachment(item, record, protocol.Output{Kind: "attachment", Data: protocol.Attachment{Spec: protocol.AttachmentSpec{Name: "other.txt"}}}, nil, &pending, expected)
		ui.NewTesterWith(func(c *ui.Context) {
			queue := r.queue
			r.queue = nil
			for _, f := range queue {
				f(c)
			}
		}, 320, 240, ui.TesterOptions{}).Frame()
		if record.pending != nil || record.running || record.ready != nil || item.Status != composer.Failed {
			t.Errorf("an unexpected publication left pending %v, status %v", record.pending != nil, item.Status)
		}
	}

	// A read answered before an action that was accepted since does not
	// take the page back; a newer one is taken.
	r := &Runtime{App: shell.NewLive(&viewmodel.Data{Workspace: &viewmodel.Workspace{}})}
	r.acceptBackupSettings(saved)
	r.acceptBackupRead(protocol.BackupSettings{Revision: 6})
	if held := r.backupSettings.saved; held.Revision != 7 || held.Local == nil {
		t.Errorf("an older read left %+v", held)
	}
	r.acceptBackupRead(protocol.BackupSettings{Revision: 8})
	if held := r.backupSettings.saved; held.Revision != 8 {
		t.Errorf("a newer read left %+v", held)
	}
}
