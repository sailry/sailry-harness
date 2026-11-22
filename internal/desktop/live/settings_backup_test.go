package live

import (
	"encoding/json"
	"errors"
	"fmt"
	"github.com/sailry/sailry-harness/internal/assembly/desktop"
	"reflect"
	"slices"
	"strings"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestManagedSystemSettings(t *testing.T) {
	app := shell.NewLiveWithAssembly(&viewmodel.Data{Workspace: &viewmodel.Workspace{}}, desktop.Harness())
	app.Page = shell.SettingsPage
	var owners [2]*Runtime
	var transports [2]*conversationTestTransport
	for index := range owners {
		snapshot := protocol.DefaultSnapshot()
		snapshot.Node = protocol.NodeID{byte(index + 1)}
		snapshot.User = &protocol.User{ID: protocol.UserID{byte(index + 1)}, Administrator: index == 0, State: "active"}
		snapshot.Sessions = []protocol.Session{{ID: protocol.NewSessionID(), Revision: uint64(index + 1)}}
		transport := &conversationTestTransport{snapshot: snapshot}
		transports[index] = transport
		controller := client.New(transport)
		if err := controller.Start(t.Context()); err != nil {
			t.Fatal(err)
		}
		t.Cleanup(func() { controller.Close() })
		owners[index] = &Runtime{ctx: t.Context(), App: app, controller: controller, busy: map[string]bool{}, uncertain: map[string]protocol.Request{}}
		stored := protocol.TerminalSettings{Revision: uint64(index + 1)}
		var applied protocol.Output
		inspections := 0
		transport.handle = func(request protocol.Request) (link.Response, bool) {
			out := func(kind string, value any) (link.Response, bool) {
				return link.Response{Output: protocol.Output{Kind: kind, Data: value}}, true
			}
			switch request.Command.Kind {
			case "read_terminal_settings":
				return out("terminal_settings", stored)
			case "list_shells":
				return out("shells", []protocol.TerminalShell{{Path: []string{"/bin/local", "/bin/remote"}[index]}})
			case "save_terminal_settings":
				value, _ := detail[protocol.TerminalSettings](request.Command.Data)
				value.Revision++
				applied = protocol.Output{Kind: "terminal_settings", Data: value}
				stored = protocol.TerminalSettings{Revision: value.Revision + 1, Environment: map[string]string{"OTHER": "newer"}}
				return link.Response{Fault: &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown}}, true
			case "inspect_request":
				value, _ := detail[protocol.CommandInspectRequestPayload](request.Command.Data)
				inspections++
				outcome := protocol.RequestOutcome{Kind: "admitted"}
				if inspections > 1 {
					outcome = protocol.RequestOutcome{Kind: "completed", Data: protocol.Result[protocol.Output, protocol.Fault]{Ok: &applied}}
				}
				return out("request_outcome", protocol.OutputRequestOutcomePayload{ID: value.ID, Outcome: outcome})
			case "read_backup_settings":
				return out("backup_settings", protocol.BackupSettings{Revision: 10, Local: &protocol.BackupLocal{Directory: "/backups/local", Enabled: true}})
			case "archive_sessions":
				return out("sessions_archived", snapshot.Sessions)
			}
			return link.Response{}, false
		}
	}
	root, remote := owners[0], owners[1]
	remote.parent = root
	remoteID := remote.controller.Target().String()
	root.bindings = map[string]*Runtime{remoteID: remote}
	m := app.Settings
	var uncertainToast, actionableToast bool
	tester := ui.NewTester(func(c *ui.Context) {
		theme.Apply(c, "", theme.Light)
		root.mu.Lock()
		queue := root.queue
		root.queue = nil
		root.mu.Unlock()
		for _, finish := range queue {
			finish(c)
		}
		root.terminalSettingsFrame()
		root.backupSettingsFrame(c)
		ui.ToastViewportBase(c, func(_ ui.Element, shown []ui.Toast) {
			for _, toast := range shown {
				if toast.Title == i18n.T(c, settings.TerminalMessages.Uncertain) && toast.Type == "warning" {
					uncertainToast = true
				}
				if toast.Action != "" || toast.OnAction != nil {
					actionableToast = true
				}
			}
		})
	}, 640, 480)
	settle := func() {
		for range 4 {
			tester.Frame()
			root.workers.Wait()
			remote.workers.Wait()
		}
	}
	requests := func(index int, kind string) []protocol.Request {
		transport := transports[index]
		transport.mu.Lock()
		defer transport.mu.Unlock()
		var selected []protocol.Request
		for _, request := range transport.requests {
			if request.Command.Kind == kind {
				selected = append(selected, request)
			}
		}
		return selected
	}

	m.Open(settings.Terminal)
	settle()
	if m.SaveTerminal == nil || m.Host.Revision != 1 || m.Shells[1].Path != "/bin/local" {
		t.Fatalf("local terminal settings: %+v %+v", m.Host, m.Shells)
	}
	m.Host.Draft.Env = []viewmodel.EnvVar{{Key: "LOCAL", Value: "draft"}}
	m.SaveTerminal()
	settle()
	saves := requests(0, "save_terminal_settings")
	if !uncertainToast || actionableToast || len(saves) != 1 {
		t.Fatalf("uncertain save: warning shown %t, actionable toast %t, saves %d", uncertainToast, actionableToast, len(saves))
	}
	newerEnv := []viewmodel.EnvVar{{Key: "OTHER", Value: "newer"}}
	if m.Host.Revision != 3 || !slices.Equal(m.Host.Saved.Env, newerEnv) {
		t.Fatalf("the reread did not observe the intervening controller's save: %+v", m.Host)
	}
	m.ManagedNode = remoteID
	settle()
	if m.Host.Revision != 2 || m.Shells[1].Path != "/bin/remote" || len(m.Host.Draft.Env) != 0 {
		t.Fatalf("terminal settings did not turn to the selected Host: %+v %+v", m.Host, m.Shells)
	}
	m.ManagedNode = ""
	settle()
	if m.SaveTerminal == nil || !slices.Equal(m.Host.Draft.Env, []viewmodel.EnvVar{{Key: "LOCAL", Value: "draft"}}) {
		t.Fatalf("returning to the original Node lost its terminal draft: %+v", m.Host.Draft)
	}
	m.SaveTerminal()
	settle()
	inspections := requests(0, "inspect_request")
	if len(inspections) != 2 || len(requests(0, "save_terminal_settings")) != 1 || len(requests(1, "save_terminal_settings"))+len(requests(1, "inspect_request")) != 0 {
		t.Fatal("resubmitting the terminal draft should only inspect its original Node request")
	}
	first, _ := detail[protocol.CommandInspectRequestPayload](inspections[0].Command.Data)
	second, _ := detail[protocol.CommandInspectRequestPayload](inspections[1].Command.Data)
	if first.ID != saves[0].ID || first.Digest == "" || first != second {
		t.Fatalf("the recovery did not inspect the original ID and payload: %+v, %+v", first, second)
	}
	if m.Host.Revision != 3 || !slices.Equal(m.Host.Saved.Env, newerEnv) || !slices.Equal(m.Host.Draft.Env, newerEnv) || root.terminalSettings.uncertain != nil {
		t.Fatalf("recovering the earlier save did not preserve the newer settings: %+v", m.Host)
	}
	m.ManagedNode = remoteID
	settle()

	m.Open(settings.Backup)
	settle()
	if a := m.BackupActions; a.Status != "permission_denied" || a.ArchiveUnavailable || a.Restartable || m.Sessions != 1 || len(requests(1, "read_backup_settings")) != 0 {
		t.Fatalf("backup authorization and session scope were mixed: %q, sessions %d", a.Status, m.Sessions)
	}
	m.BackupActions.ArchiveAll()
	settle()
	received := requests(1, "archive_sessions")
	if len(received) != 1 || len(requests(0, "archive_sessions")) != 0 {
		t.Fatalf("archive ran against the wrong Node: %+v", received)
	}
	archived, _ := detail[protocol.CommandArchiveSessionsPayload](received[0].Command.Data)
	session := transports[1].snapshot.Sessions[0]
	if !slices.Equal(archived.Expected, []protocol.SessionRevision{{Session: session.ID, Revision: session.Revision}}) {
		t.Fatalf("archive captured the wrong sessions: %+v", archived.Expected)
	}
	m.ManagedNode = "missing-host"
	settle()
	if m.BackupActions.Status != "connections_offline" || !m.BackupActions.ArchiveUnavailable || m.Sessions != 0 || m.SaveTerminal != nil {
		t.Fatal("missing Host retained the previous settings actions")
	}
	m.ManagedNode = ""
	settle()
	if m.BackupActions.Status != "" || m.Backup.Saved.Directory != "/backups/local" || m.Backup.Revision != 10 || len(requests(0, "read_backup_settings")) != 1 {
		t.Fatalf("returning to this device loaded the wrong backup settings: %+v", m.Backup)
	}
}

func TestReleasedTerminalSaveKeepsProposalRevision(t *testing.T) {
	for _, tc := range []struct {
		outcome  string
		revision uint64
		canRetry bool
		edited   bool
	}{
		{outcome: "not_admitted", revision: 3}, {outcome: "unknown", revision: 3},
		{outcome: "not_admitted", revision: 1, canRetry: true}, {outcome: "unknown", revision: 1},
		{outcome: "not_admitted", revision: 3, edited: true},
	} {
		t.Run(fmt.Sprintf("%s/revision_%d/edited_%t", tc.outcome, tc.revision, tc.edited), func(t *testing.T) {
			transport := &conversationTestTransport{snapshot: protocol.DefaultSnapshot()}
			transport.snapshot.Node = protocol.NodeID{1}
			controller := client.New(transport)
			if err := controller.Start(t.Context()); err != nil {
				t.Fatal(err)
			}
			t.Cleanup(func() { controller.Close() })
			r := &Runtime{ctx: t.Context(), controller: controller, App: shell.NewLiveWithAssembly(&viewmodel.Data{Workspace: &viewmodel.Workspace{}}, desktop.Harness())}
			r.terminalSettings.shown, r.terminalSettings.visible = r, true
			r.acceptTerminalSettings(protocol.TerminalSettings{Revision: 1}, nil)
			r.App.Settings.Host.Draft.Env = []viewmodel.EnvVar{{Key: "LOCAL", Value: "draft"}}
			var saves []protocol.Request
			inspections := 0
			transport.handle = func(request protocol.Request) (link.Response, bool) {
				switch request.Command.Kind {
				case "save_terminal_settings":
					saves = append(saves, request)
					if len(saves) == 1 {
						return link.Response{Fault: &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown}}, true
					}
					value, _ := detail[protocol.TerminalSettings](request.Command.Data)
					value.Revision++
					return link.Response{Output: protocol.Output{Kind: "terminal_settings", Data: value}}, true
				case "inspect_request":
					inspections++
					outcome := tc.outcome
					if inspections == 1 {
						outcome = "admitted"
					}
					return link.Response{Output: protocol.Output{Kind: "request_outcome", Data: protocol.OutputRequestOutcomePayload{ID: saves[0].ID, Outcome: protocol.RequestOutcome{Kind: outcome}}}}, true
				}
				return link.Response{}, false
			}
			tester := ui.NewTester(func(c *ui.Context) {
				theme.Apply(c, "", theme.Light)
				queue := r.queue
				r.queue = nil
				for _, apply := range queue {
					apply(c)
				}
			}, 640, 480)
			save := func() {
				r.saveTerminalSettings()
				r.workers.Wait()
				tester.Frame()
			}
			save()
			if tc.revision > 1 {
				r.acceptTerminalRead(protocol.TerminalSettings{Revision: tc.revision, Environment: map[string]string{"OTHER": "newer"}}, nil)
			}
			if tc.edited {
				r.App.Settings.Host.Draft.Env[0].Value = "edited"
			}
			wantEnv := slices.Clone(r.App.Settings.Host.Draft.Env)
			save()
			if len(saves) != 1 || inspections != 2 || r.terminalSettings.uncertain != nil {
				t.Fatal("the original save was not released by inspection alone")
			}
			save()
			if inspections != 2 {
				t.Fatal("the released request was inspected again")
			}
			if tc.canRetry {
				if len(saves) != 2 || saves[0].ID == saves[1].ID || !reflect.DeepEqual(saves[0].Command, saves[1].Command) {
					t.Fatal("an explicitly retried absent save did not keep its original payload and revision")
				}
			} else if len(saves) != 1 || !slices.Equal(r.App.Settings.Host.Draft.Env, wantEnv) {
				t.Fatalf("the released proposal was rewritten or discarded: writes %d, draft %+v", len(saves), r.App.Settings.Host.Draft)
			}
		})
	}
}

func TestEditedTerminalDraftAfterUncertainSave(t *testing.T) {
	for _, tc := range []struct {
		name     string
		revision uint64
		conflict bool
	}{
		{"original revision", 1, false},
		{"newer controller revision", 3, true},
	} {
		t.Run(tc.name, func(t *testing.T) {
			transport := &conversationTestTransport{snapshot: protocol.DefaultSnapshot()}
			transport.snapshot.Node = protocol.NodeID{1}
			r := &Runtime{ctx: t.Context(), controller: client.New(transport)}
			t.Cleanup(func() { r.controller.Close() })
			original := protocol.TerminalSettings{Revision: 1, Shell: "/bin/first"}
			previous := r.controller.Prepare(protocol.Command{Kind: "save_terminal_settings", Data: original})
			applied := original
			applied.Revision++
			edited := protocol.TerminalSettings{Revision: tc.revision, Shell: "/bin/edited"}
			transport.handle = func(request protocol.Request) (link.Response, bool) {
				switch request.Command.Kind {
				case "inspect_request":
					result := protocol.Output{Kind: "terminal_settings", Data: applied}
					outcome := protocol.RequestOutcome{Kind: "completed", Data: protocol.Result[protocol.Output, protocol.Fault]{Ok: &result}}
					return link.Response{Output: protocol.Output{Kind: "request_outcome", Data: protocol.OutputRequestOutcomePayload{ID: previous.ID, Outcome: outcome}}}, true
				case "save_terminal_settings":
					value, _ := detail[protocol.TerminalSettings](request.Command.Data)
					if request.ID == previous.ID || value.Revision != applied.Revision || value.Shell != edited.Shell {
						t.Fatalf("new edit did not build on the confirmed save: %+v", request)
					}
					value.Revision++
					return link.Response{Output: protocol.Output{Kind: "terminal_settings", Data: value}}, true
				}
				return link.Response{}, false
			}
			_, retained, err := r.writeTerminalSettings(edited, &previous)
			if tc.conflict {
				if faultCode(err) != protocol.ErrorCodeRevisionConflict || retained != nil || len(transport.requests) != 1 || transport.requests[0].Command.Kind != "inspect_request" {
					t.Fatalf("the newer revision should conflict without a write: %v, requests %+v", err, transport.requests)
				}
			} else if err != nil || retained != nil || len(transport.requests) != 2 || transport.requests[0].Command.Kind != "inspect_request" || transport.requests[1].Command.Kind != "save_terminal_settings" {
				t.Fatalf("the edited draft should follow the confirmed write: %v, requests %+v", err, transport.requests)
			}
		})
	}
}

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
		r := &Runtime{App: shell.NewLiveWithAssembly(&viewmodel.Data{Workspace: &viewmodel.Workspace{}}, desktop.Harness()), uncertain: map[string]protocol.Request{}}
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
		r := &Runtime{App: shell.NewLiveWithAssembly(&viewmodel.Data{Workspace: &viewmodel.Workspace{}}, desktop.Harness())}
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
	r := &Runtime{App: shell.NewLiveWithAssembly(&viewmodel.Data{Workspace: &viewmodel.Workspace{}}, desktop.Harness())}
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
