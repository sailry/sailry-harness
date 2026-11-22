package live

import (
	"slices"
	"sync"
	"testing"

	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestBackupUnarchiveManySettlesSelectedSessions(t *testing.T) {
	snapshot := selectionSnapshot("Changed", "Pending", "Restored", "Unselected")
	for i := range snapshot.Sessions {
		snapshot.Sessions[i].Archived = true
		snapshot.Sessions[i].Revision += uint64(i)
	}
	conflict, pending, success := snapshot.Sessions[0], snapshot.Sessions[1], snapshot.Sessions[2]
	untouched := snapshot.Sessions[3]
	w := selectionWindow(t, snapshot)
	restored := success
	restored.Archived, restored.Revision = false, success.Revision+1
	inspections := 0
	w.source.mu.Lock()
	w.source.handle = func(request protocol.Request) (link.Response, bool) {
		switch request.Command.Kind {
		case "read_backup_settings":
			return link.Response{Output: protocol.Output{Kind: "backup_settings", Data: protocol.DefaultBackupSettings()}}, true
		case "set_session_archived":
			input := request.Command.Data.(protocol.CommandSetSessionArchivedPayload)
			switch input.Session {
			case conflict.ID:
				return link.Response{Fault: &protocol.Fault{Code: protocol.ErrorCodeRevisionConflict}}, true
			case pending.ID:
				return link.Response{Fault: &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown}}, true
			case success.ID:
				return link.Response{Output: protocol.Output{Kind: "session", Data: restored}}, true
			}
		case "inspect_request":
			inspections++
			query := request.Command.Data.(protocol.CommandInspectRequestPayload)
			outcome := protocol.RequestOutcome{Kind: "admitted"}
			if inspections == 2 {
				result := protocol.Result[protocol.Output, protocol.Fault]{Err: &protocol.Fault{Code: protocol.ErrorCodeRevisionConflict}}
				outcome = protocol.RequestOutcome{Kind: "completed", Data: result}
			}
			return link.Response{Output: protocol.Output{Kind: "request_outcome", Data: protocol.OutputRequestOutcomePayload{ID: query.ID, Outcome: outcome}}}, true
		}
		return link.Response{}, false
	}
	w.source.mu.Unlock()
	w.r.openNodeSettings(settings.Backup)
	w.window.Frame()
	w.r.workers.Wait()
	w.window.Frame()
	m := w.r.App.Settings
	m.BackupActions.UnarchiveMany([]string{conflict.ID.String(), pending.ID.String(), success.ID.String(), pending.ID.String()})
	if w.r.backupArchiveWork() == "" || m.BackupActions.PrepareDeleteMany([]string{untouched.ID.String()}) != nil {
		t.Fatal("another archive action could start while the restore batch was running")
	}
	w.r.workers.Wait()
	w.window.Frame()
	if m.BackupActions.ArchiveBusy {
		t.Fatal("archive actions stayed busy after the batch settled")
	}
	if !w.window.HasText(i18n.In("en", "backup_sessions_unknown")) {
		t.Fatalf("the pending restore was not reported as uncertain: %q", w.window.Texts())
	}
	if w.window.HasText(i18n.In("en", "backup_unarchived")) {
		t.Fatal("a partially successful batch was reported as fully restored")
	}
	if len(m.Archived) != 4 {
		t.Fatal("the archive changed before the Node projected its result")
	}
	updated := snapshot
	updated.Cursor++
	updated.Sessions = slices.Clone(snapshot.Sessions)
	updated.Sessions[1].Revision++
	updated.Sessions[2] = restored
	w.update(t, updated.Cursor, protocol.Update{Kind: "snapshot", Data: updated})
	if len(m.Archived) != 3 || slices.ContainsFunc(m.Archived, func(row viewmodel.Archived) bool { return row.ID == success.ID.String() }) {
		t.Fatalf("the archive did not reflect the Node's partial success: %+v", m.Archived)
	}
	m.BackupActions.UnarchiveMany([]string{pending.ID.String()})
	w.r.workers.Wait()
	w.window.Frame()
	if !w.window.HasText(i18n.In("en", "session_changed")) {
		t.Fatalf("the settled conflict was not reported: %q", w.window.Texts())
	}

	mutations := backupArchiveRequests(w.source, "set_session_archived")
	if len(mutations) != 3 {
		t.Fatalf("restore sent %d archive commands, want three selected sessions", len(mutations))
	}
	for index, request := range mutations {
		s := snapshot.Sessions[index]
		want := protocol.CommandSetSessionArchivedPayload{Session: s.ID, ExpectedRevision: s.Revision}
		if request.Target != snapshot.Node || request.Command.Data != want {
			t.Fatalf("restore %d sent %+v to %s, want %+v on %s", index, request.Command.Data, request.Target, want, snapshot.Node)
		}
	}
	queries := backupArchiveRequests(w.source, "inspect_request")
	if len(queries) != 2 {
		t.Fatalf("inspected %d requests, want two inspections of the original pending request", len(queries))
	}
	first := queries[0].Command.Data.(protocol.CommandInspectRequestPayload)
	if first.ID != mutations[1].ID || first.Digest == "" || queries[1].Command.Data != first {
		t.Fatalf("retry did not inspect the original restore after its revision changed: %+v", queries)
	}
}

func TestBackupPrepareDeleteManyCapturesNodeAndSelectedRevisions(t *testing.T) {
	w := selectionWindow(t, selectionSnapshot("Local"))
	snapshot := selectionSnapshot("First", "Active", "Third", "Unselected")
	snapshot.Node = protocol.NodeID{2}
	snapshot.Sessions[0].Archived, snapshot.Sessions[0].Revision = true, 3
	snapshot.Sessions[2].Archived, snapshot.Sessions[2].Revision = true, 7
	snapshot.Sessions[3].Archived = true
	remote := &selectionTestTransport{
		conversationTestTransport: conversationTestTransport{snapshot: snapshot},
		node:                      &conversationTestSubscription{updates: make(chan protocol.Update, 1), closed: make(chan struct{})},
	}
	remote.handle = func(request protocol.Request) (link.Response, bool) {
		switch request.Command.Kind {
		case "read_backup_settings":
			return link.Response{Output: protocol.Output{Kind: "backup_settings", Data: protocol.DefaultBackupSettings()}}, true
		case "remove_sessions":
			return link.Response{Fault: &protocol.Fault{Code: protocol.ErrorCodeRevisionConflict}}, true
		}
		return link.Response{}, false
	}
	remote.node.updates <- protocol.Update{Kind: "snapshot", Data: snapshot}
	controller := client.New(remote)
	if err := controller.Start(w.r.ctx); err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = controller.Close() })
	owner := w.r.newBinding(controller)
	host := snapshot.Node.String()
	w.r.bindings = map[string]*Runtime{host: owner}
	owner.openNodeSettings(settings.Backup)
	w.window.Frame()
	actions := w.r.App.Settings.BackupActions
	if actions.PrepareDeleteMany(nil) != nil {
		t.Fatal("an empty selection prepared a deletion")
	}
	first, third := snapshot.Sessions[0], snapshot.Sessions[2]
	if actions.PrepareDeleteMany([]string{first.ID.String(), snapshot.Sessions[1].ID.String()}) != nil {
		t.Fatal("a selection including an active session prepared an archive deletion")
	}
	ids := []string{third.ID.String(), first.ID.String(), third.ID.String()}
	confirm := actions.PrepareDeleteMany(ids)
	if confirm == nil || len(backupArchiveRequests(remote, "remove_sessions")) != 0 {
		t.Fatal("the selection was not captured without submitting a deletion")
	}
	ids[0], ids[1], ids[2] = snapshot.Sessions[3].ID.String(), "", ""
	updated := snapshot
	updated.Cursor++
	updated.Sessions = slices.Clone(snapshot.Sessions)
	updated.Sessions[0].Revision++
	changed := make(chan struct{}, 1)
	unwatch := controller.OnChange(func() {
		if controller.Snapshot().Cursor == updated.Cursor {
			select {
			case changed <- struct{}{}:
			default:
			}
		}
	})
	defer unwatch()
	remote.node.updates <- protocol.Update{Kind: "snapshot", Data: updated}
	select {
	case <-changed:
	case <-w.r.ctx.Done():
		t.Fatal("the remote revision was not projected")
	}
	w.r.openNodeSettings(settings.Backup)
	w.window.Frame()
	confirm()
	w.r.workers.Wait()
	w.window.Frame()
	requests := backupArchiveRequests(remote, "remove_sessions")
	if len(requests) != 1 || len(backupArchiveRequests(w.source, "remove_session", "remove_sessions")) != 0 {
		t.Fatal("confirmation did not run exactly one batch on the original Node")
	}
	got := requests[0].Command.Data.(protocol.CommandRemoveSessionsPayload)
	want := []protocol.SessionRevision{{Session: first.ID, Revision: first.Revision}, {Session: third.ID, Revision: third.Revision}}
	if requests[0].Target != snapshot.Node || !slices.Equal(got.Expected, want) {
		t.Fatalf("confirmation sent %+v to %s, want %+v on %s", got.Expected, requests[0].Target, want, snapshot.Node)
	}
	owner.openNodeSettings(settings.Backup)
	w.window.Frame()
	if m := w.r.App.Settings; m.BackupActions.ArchiveBusy || len(m.Archived) != 3 {
		t.Fatalf("a rejected delete changed the archive or left it busy: %+v", m.Archived)
	}
}

func TestBackupConfirmedDeletionProjectsArchive(t *testing.T) {
	for _, test := range []struct {
		name    string
		failure protocol.ErrorCode
	}{
		{name: "single", failure: protocol.ErrorCodeRevisionConflict},
		{name: "batch", failure: protocol.ErrorCodeOutcomeUnknown},
		{name: "all"},
	} {
		t.Run(test.name, func(t *testing.T) {
			before := []string{"First archive", "Second archive", "Unselected archive"}
			snapshot := selectionSnapshot(before[0], before[1], before[2], "Active conversation")
			for i := range before {
				snapshot.Sessions[i].Archived = true
			}
			first, second := snapshot.Sessions[0].ID.String(), snapshot.Sessions[1].ID.String()
			w := selectionWindow(t, snapshot)
			reply := make(chan struct{})
			release := sync.OnceFunc(func() { close(reply) })
			defer release()
			removed := make([]protocol.SessionID, len(snapshot.Sessions))
			for i, session := range snapshot.Sessions {
				removed[i] = session.ID
			}
			w.source.mu.Lock()
			w.source.handle = func(request protocol.Request) (link.Response, bool) {
				switch request.Command.Kind {
				case "read_backup_settings":
					return link.Response{Output: protocol.Output{Kind: "backup_settings", Data: protocol.DefaultBackupSettings()}}, true
				case "remove_session", "remove_sessions":
					<-reply
					if test.failure != "" {
						return link.Response{Fault: &protocol.Fault{Code: test.failure}}, true
					}
					return link.Response{Output: protocol.Output{Kind: "sessions_removed", Data: removed}}, true
				case "inspect_request":
					query := request.Command.Data.(protocol.CommandInspectRequestPayload)
					return link.Response{Output: protocol.Output{Kind: "request_outcome", Data: protocol.OutputRequestOutcomePayload{
						ID: query.ID, Outcome: protocol.RequestOutcome{Kind: "admitted"},
					}}}, true
				}
				return link.Response{}, false
			}
			w.source.mu.Unlock()
			w.r.openNodeSettings(settings.Backup)
			w.window.Frame()
			w.r.workers.Wait()
			w.window.Frame()
			m := w.r.App.Settings
			var prepare func() func()
			var remaining []string
			active := 1
			switch test.name {
			case "single":
				prepare = func() func() { return m.BackupActions.PrepareDelete(first) }
				remaining = before[1:]
			case "batch":
				prepare = func() func() { return m.BackupActions.PrepareDeleteMany([]string{first, second}) }
				remaining = before[2:]
			case "all":
				prepare = func() func() { return m.BackupActions.PrepareDelete("") }
				active = 0
			}
			assertArchive := func(want []string, sessions int, busy bool) {
				t.Helper()
				if !slices.EqualFunc(m.Archived, want, func(row viewmodel.Archived, title string) bool { return row.Title == title }) ||
					m.Sessions != sessions || m.BackupActions.ArchiveBusy != busy {
					t.Fatalf("archive is %+v, active=%d, busy=%t; want %v, active=%d, busy=%t", m.Archived, m.Sessions, m.BackupActions.ArchiveBusy, want, sessions, busy)
				}
			}

			confirm := prepare()
			if confirm == nil {
				t.Fatal("deletion could not be prepared")
			}
			w.window.Frame()
			assertArchive(before, 1, false)
			confirm()
			w.window.Frame()
			assertArchive(remaining, active, true)
			if test.failure == protocol.ErrorCodeRevisionConflict {
				updated := snapshot
				updated.Cursor++
				updated.Sessions = slices.Clone(snapshot.Sessions)
				before[0] = "Updated archive"
				updated.Sessions[0].Activity.Title = before[0]
				updated.Sessions[0].Revision++
				w.update(t, updated.Cursor, protocol.Update{Kind: "snapshot", Data: updated})
				assertArchive(remaining, active, true)
			}
			release()
			w.r.workers.Wait()
			w.window.Frame()
			if len(w.r.snapshot().Sessions) != len(snapshot.Sessions) {
				t.Fatal("deletion changed the Client snapshot ahead of the Node event")
			}
			if test.failure == "" {
				assertArchive(remaining, active, false)
				if prepare() != nil {
					t.Fatal("delete all prepared another request for sessions already confirmed deleted")
				}
				w.event(t, "sessions_removed", removed)
				assertArchive(remaining, active, false)
				return
			}
			assertArchive(before, 1, false)
			retry := prepare()
			if retry == nil {
				t.Fatal("restored rows did not allow the removal to be retried")
			}
			retry()
			w.r.workers.Wait()
			w.window.Frame()
			assertArchive(before, 1, false)
			removals := backupArchiveRequests(w.source, "remove_session", "remove_sessions")
			if test.failure == protocol.ErrorCodeRevisionConflict {
				if len(removals) != 2 || removals[0].ID == removals[1].ID {
					t.Fatalf("retry did not submit a fresh removal: %+v", removals)
				}
				if got := removals[1].Command.Data.(protocol.CommandRemoveSessionPayload).ExpectedRevision; got != snapshot.Sessions[0].Revision+1 {
					t.Fatalf("retried at stale revision %d", got)
				}
				return
			}
			inspections := backupArchiveRequests(w.source, "inspect_request")
			if len(removals) != 1 || len(inspections) != 2 {
				t.Fatalf("retry sent %d removals and %d inspections, want one original removal inspected twice", len(removals), len(inspections))
			}
			for _, request := range inspections {
				if id := request.Command.Data.(protocol.CommandInspectRequestPayload).ID; id != removals[0].ID {
					t.Fatalf("inspected request %s, want %s", id, removals[0].ID)
				}
			}
		})
	}
}

func backupArchiveRequests(source *selectionTestTransport, kinds ...string) []protocol.Request {
	source.mu.Lock()
	defer source.mu.Unlock()
	var requests []protocol.Request
	for _, request := range source.requests {
		if slices.Contains(kinds, request.Command.Kind) {
			requests = append(requests, request)
		}
	}
	return requests
}
