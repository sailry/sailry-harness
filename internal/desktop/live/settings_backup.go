package live

import (
	"context"
	"errors"
	"fmt"
	"strings"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// Settings › Backup: the selected host's storage, schedule and archives.
// Settings are read when the page shows and after every action; the
// schedule's draft is saved shortly after it changes.

// backupScheduleDelay is how long the schedule's draft rests before it is saved.
const backupScheduleDelay = 350 * time.Millisecond

// backupSettingsState is a Node's backup settings and the action running
// against it. shown, visible and actions are the local runtime's: the Node
// the page holds, whether the page showed last frame, and its actions.
type backupSettingsState struct {
	saved   protocol.BackupSettings
	known   bool
	reading bool
	failed  bool
	stale   bool
	busy    bool

	records []settings.BackupRecord
	partial bool
	listing bool

	schedule backupSchedule
	changed  time.Time
	refused  *backupSchedule

	shown   *Runtime
	visible bool
	actions *settings.BackupActions
}

// backupSchedule is the schedule as the page drafts it.
type backupSchedule struct {
	enabled      bool
	hour, minute int
	timezone     string
}

// backupSettingsFrame keeps the page on the selected host's Node. Turning
// to another host drops the dialogs and their drafts; the Node's settings
// are read when unknown or after an action, and only while the page shows.
func (r *Runtime) backupSettingsFrame(c *ui.Context) {
	root := r.localRuntime()
	state := &root.backupSettings
	m := root.App.Settings
	owner := root.runtimeFor(root.App.Host)
	if state.actions == nil {
		state.actions = root.backupActions()
	}
	m.BackupActions = state.actions
	if owner != state.shown {
		state.shown = owner
		m.CloseBackupDialogs()
		m.Backup = settings.Stored[settings.BackupTarget]{}
		if owner != nil && owner.backupSettings.known {
			owner.showBackupSettings()
		}
	}
	visible := root.App.Page == shell.SettingsPage && m.Section == settings.Backup
	entered := visible && !state.visible
	state.visible = visible
	if owner == nil {
		m.Backup.Pending, m.Archived, m.Sessions = true, nil, 0
		state.actions.Records, state.actions.Partial, state.actions.Listing = nil, false, false
		return
	}
	s := &owner.backupSettings
	m.Backup.Pending = s.busy || !s.known
	state.actions.Records, state.actions.Partial, state.actions.Listing = s.records, s.partial, s.listing
	if !visible {
		return
	}
	if entered {
		s.failed = false
		s.stale = s.known
	}
	if !s.reading && !s.busy && (!s.known || s.stale) && !s.failed {
		owner.readBackupSettings()
	}
	state.actions.Host = i18n.T(c, "local_host_name")
	if owner != root {
		state.actions.Host = root.App.Host
		if host := root.hostState(root.App.Host); host != nil {
			state.actions.Host = host.view.Name
		}
	}
	m.Archived, m.Sessions = backupSessions(owner.snapshot().Sessions, i18n.T(c, "conversation"))
	owner.saveBackupSchedule(time.Now())
}

// backupSessions are the archived sessions the page lists and how many
// sessions Archive all would archive. Delegated sessions follow their parents.
func backupSessions(sessions []protocol.Session, untitled string) (archived []viewmodel.Archived, active int) {
	for _, session := range sessions {
		switch {
		case session.Delegation != nil:
		case !session.Archived:
			active++
		default:
			row := viewmodel.Archived{ID: session.ID.String(), Title: session.Activity.Title}
			if row.Title == "" {
				row.Title = untitled
			}
			if last := lastRun(session); last != nil {
				row.At = time.UnixMilli(*last)
			}
			archived = append(archived, row)
		}
	}
	return archived, active
}

// backupRevisions are the sessions a bulk command names at the revisions
// the page saw; archived ones only when they are removed.
func backupRevisions(sessions []protocol.Session, archived bool) []protocol.SessionRevision {
	expected := []protocol.SessionRevision{}
	for _, session := range sessions {
		if session.Delegation == nil && (archived || !session.Archived) {
			expected = append(expected, protocol.SessionRevision{Session: session.ID, Revision: session.Revision})
		}
	}
	return expected
}

// backupStorageTab is the storage a Node's settings use: 2 for the local
// folder alone, otherwise the destination's kind.
func backupStorageTab(value protocol.BackupSettings) int {
	switch {
	case !value.OnlineEnabled && value.Local != nil && value.Local.Enabled:
		return 2
	case value.Destination != nil && value.Destination.Kind == protocol.BackupKindS3:
		return 1
	}
	return 0
}

// backupTarget is the page's view of a Node's settings. No secret is part
// of it: the Node reports only whether they are set.
func backupTarget(value protocol.BackupSettings) settings.BackupTarget {
	target := settings.BackupTarget{Tab: backupStorageTab(value), Prefix: "sailry-backups", Timezone: "UTC",
		Time: time.Date(2026, 1, 1, 3, 0, 0, 0, time.Local), PassphraseSet: value.PassphraseConfigured, CredentialsSet: value.CredentialsConfigured}
	if d := value.Destination; d != nil {
		target.Endpoint, target.Prefix, target.Bucket, target.Region = d.Endpoint, d.Root, d.Bucket, d.Region
	}
	if value.Local != nil {
		target.Directory = value.Local.Directory
	}
	if s := value.Schedule; s != nil {
		target.Schedule, target.Timezone = s.Enabled, s.Timezone
		target.Time = time.Date(2026, 1, 1, int(s.Hour), int(s.Minute), 0, 0, time.Local)
	}
	return target
}

func backupScheduleOf(target settings.BackupTarget) backupSchedule {
	return backupSchedule{enabled: target.Schedule, hour: target.Time.Hour(), minute: target.Time.Minute(), timezone: strings.TrimSpace(target.Timezone)}
}

func backupKeep() protocol.PluginSettingsSecretUpdate {
	return protocol.PluginSettingsSecretUpdate{Kind: "keep"}
}

// backupScheduleUpdate saves only the schedule; every secret is kept.
func backupScheduleUpdate(draft backupSchedule, saved protocol.BackupSettings) protocol.BackupUpdate {
	saved.Schedule = &protocol.BackupSchedule{Enabled: draft.enabled, Timezone: draft.timezone, Hour: uint8(draft.hour), Minute: uint8(draft.minute)}
	return protocol.BackupUpdate{Settings: saved, Identity: backupKeep(), Secret: backupKeep(), SessionToken: backupKeep(), Passphrase: backupKeep()}
}

// backupStorageUpdate is the storage dialog's draft as the Node saves it,
// at the revision the page read. A typed secret replaces the stored one, a
// blank one keeps it, and cleared credentials are removed; WebDAV has no
// session token. The schedule is not the dialog's to change.
func backupStorageUpdate(target settings.BackupTarget, saved protocol.BackupSettings) protocol.BackupUpdate {
	directory := strings.TrimSpace(target.Directory)
	next := saved
	if target.Tab < 2 {
		kind := protocol.BackupKindWebdav
		if target.Tab == 1 {
			kind = protocol.BackupKindS3
		}
		next.Destination = &protocol.BackupDestination{Kind: kind, Endpoint: strings.TrimSpace(target.Endpoint), Root: strings.TrimSpace(target.Prefix),
			Bucket: strings.TrimSpace(target.Bucket), Region: strings.TrimSpace(target.Region)}
	}
	next.OnlineEnabled, next.Local = target.Tab < 2, nil
	if directory != "" {
		next.Local = &protocol.BackupLocal{Directory: directory, Enabled: target.Tab == 2}
	}
	clear := saved.CredentialsConfigured && !target.CredentialsSet
	credential := func(value string) protocol.PluginSettingsSecretUpdate {
		switch {
		case value != "":
			return protocol.PluginSettingsSecretUpdate{Kind: "replace", Data: protocol.NewSecret(value)}
		case clear:
			return protocol.PluginSettingsSecretUpdate{Kind: "clear"}
		}
		return backupKeep()
	}
	update := protocol.BackupUpdate{Settings: next, Identity: credential(target.Identity), Secret: credential(target.Secret),
		SessionToken: protocol.PluginSettingsSecretUpdate{Kind: "clear"}, Passphrase: backupKeep()}
	if target.Tab != 0 {
		update.SessionToken = credential(target.Token)
	}
	if target.Passphrase != "" {
		update.Passphrase = protocol.PluginSettingsSecretUpdate{Kind: "replace", Data: protocol.NewSecret(target.Passphrase)}
	}
	return update
}

// backupStorageDirty reports whether the dialog's draft differs from what
// the Node holds; a clean draft closes without a request.
func backupStorageDirty(target settings.BackupTarget, saved protocol.BackupSettings) bool {
	current := backupTarget(saved)
	return saved.CredentialsConfigured && !target.CredentialsSet ||
		target.Identity != "" || target.Secret != "" || target.Token != "" || target.Passphrase != "" || target.Confirm != "" ||
		target.Tab != current.Tab || strings.TrimSpace(target.Endpoint) != current.Endpoint || strings.TrimSpace(target.Prefix) != current.Prefix ||
		strings.TrimSpace(target.Bucket) != current.Bucket || strings.TrimSpace(target.Region) != current.Region ||
		strings.TrimSpace(target.Directory) != current.Directory ||
		(target.Tab == 2) != (saved.Local != nil && saved.Local.Enabled) || (target.Tab < 2) != saved.OnlineEnabled
}

// backupRecords are the stored backups by copy. A backup the Node lists
// without locations is an online one.
func backupRecords(page protocol.BackupPage) []settings.BackupRecord {
	records := make([]settings.BackupRecord, 0, len(page.Records))
	for _, record := range page.Records {
		row := settings.BackupRecord{ID: record.ID, Created: time.UnixMilli(int64(min(record.CreatedMs, 1<<62))), Size: record.SizeBytes, Online: true}
		if copies, listed := page.Locations[record.ID]; listed {
			row.Local, row.Online = copies.Local, copies.Online
		}
		records = append(records, row)
	}
	return records
}

// backupFailure is a failed backup action's toast.
func backupFailure(err error) (kit.Outcome, string) {
	switch faultCode(err) {
	case protocol.ErrorCodeRevisionConflict, protocol.ErrorCodeConflict:
		return kit.Conflict, "backup_conflict"
	case protocol.ErrorCodeInvalidRequest:
		return kit.Failed, "backup_invalid"
	case protocol.ErrorCodePermissionDenied:
		return kit.Failed, "backup_denied"
	case protocol.ErrorCodeOutcomeUnknown:
		return kit.Uncertain, "backup_unknown"
	}
	return kit.Failed, "backup_failed"
}

// backupSessionFailure is a failed session action's toast.
func backupSessionFailure(err error) (kit.Outcome, string) {
	switch faultCode(err) {
	case protocol.ErrorCodeBusy:
		return kit.Failed, "backup_sessions_busy"
	case protocol.ErrorCodeRevisionConflict, protocol.ErrorCodeNotFound:
		return kit.Conflict, "session_changed"
	case protocol.ErrorCodeOutcomeUnknown:
		return kit.Uncertain, "backup_sessions_unknown"
	}
	return kit.Failed, "backup_sessions_failed"
}

// showBackupSettings puts the Node's settings on the page, dropping its draft.
func (r *Runtime) showBackupSettings() {
	s := &r.backupSettings
	target := backupTarget(s.saved)
	r.localRuntime().App.Settings.Backup = settings.Stored[settings.BackupTarget]{Saved: target, Draft: target, Revision: s.saved.Revision}
	s.schedule, s.refused = backupScheduleOf(target), nil
}

// acceptBackupSettings takes what the Node holds. While the page shows this
// Node, a schedule draft that differs from what was saved is kept.
func (r *Runtime) acceptBackupSettings(value protocol.BackupSettings) {
	s := &r.backupSettings
	root := r.localRuntime()
	model := &root.App.Settings.Backup
	draft, dirty := model.Draft, s.known && root.backupSettings.shown == r && backupScheduleOf(model.Draft) != backupScheduleOf(model.Saved)
	s.saved, s.known, s.failed, s.stale = value, true, false, false
	if root.backupSettings.shown != r {
		return
	}
	schedule, refused := s.schedule, s.refused
	r.showBackupSettings()
	if dirty {
		model.Draft.Schedule, model.Draft.Time, model.Draft.Timezone = draft.Schedule, draft.Time, draft.Timezone
		s.schedule, s.refused = schedule, refused
	}
}

// readBackupSettings reads the Node's settings. A failure is a toast and is
// not tried again until the page is entered anew.
func (r *Runtime) readBackupSettings() {
	s := &r.backupSettings
	if s.reading || r.ctx != nil && r.ctx.Err() != nil {
		return
	}
	s.reading, s.stale = true, false
	s.reading = r.async(func() {
		output, err := r.controller.Do(r.ctx, protocol.Command{Kind: "read_backup_settings"})
		value, ok := detail[protocol.BackupSettings](output.Data)
		if err == nil && (output.Kind != "backup_settings" || !ok) {
			err = fmt.Errorf("unexpected backup settings response")
		}
		r.enqueue(func(c *ui.Context) {
			s.reading = false
			if err != nil {
				s.failed = true
				outcome, key := backupFailure(err)
				kit.Toast(c, outcome, i18n.T(c, key))
				return
			}
			r.acceptBackupRead(value)
		})
	})
}

// acceptBackupRead takes a read's answer, unless it is older than the
// settings held: such a read was answered before an action whose result was
// accepted since, and would take the page back to what that action replaced.
func (r *Runtime) acceptBackupRead(value protocol.BackupSettings) {
	if s := &r.backupSettings; s.known && value.Revision < s.saved.Revision {
		return
	}
	r.acceptBackupSettings(value)
}

// saveBackupSchedule saves the schedule's draft once it differs from what
// the Node holds: the switch at once, the time and timezone after they
// rest. An enabled schedule without a timezone waits for one. A draft the
// Node refused is not sent again until it changes.
func (r *Runtime) saveBackupSchedule(now time.Time) {
	s := &r.backupSettings
	model := &r.localRuntime().App.Settings.Backup
	if !s.known || s.busy || s.reading {
		return
	}
	draft, saved := backupScheduleOf(model.Draft), backupScheduleOf(model.Saved)
	if draft != s.schedule {
		s.schedule, s.changed = draft, now
		if draft.enabled == saved.enabled {
			time.AfterFunc(backupScheduleDelay, func() { r.enqueue(func(*ui.Context) {}) })
		}
	}
	if draft == saved {
		s.refused = nil
		return
	}
	if s.refused != nil && *s.refused == draft || draft.enabled && draft.timezone == "" ||
		draft.enabled == saved.enabled && now.Sub(s.changed) < backupScheduleDelay {
		return
	}
	command := protocol.Command{Kind: "save_backup_settings", Data: backupScheduleUpdate(draft, s.saved)}
	r.submitBackup(command, "", backupFailure, func(err error) {
		if err != nil {
			s.refused = &draft
		}
	})
}

// submitBackup runs one action against the Node; the page's controls stay
// disabled until it ends. Its outcome is a toast, and the settings are read
// again unless the Node returned them. A request whose outcome is unknown
// is retained and sent again by the same action.
func (r *Runtime) submitBackup(command protocol.Command, done string, failure func(error) (kit.Outcome, string), finished func(error)) {
	s := &r.backupSettings
	s.busy = true
	r.execute(command, func(c *ui.Context, output protocol.Output, err error) {
		s.busy, s.stale = false, true
		if value, ok := detail[protocol.BackupSettings](output.Data); err == nil && ok && output.Kind == "backup_settings" {
			r.acceptBackupSettings(value)
		}
		outcome, key := kit.Done, done
		if err != nil {
			outcome, key = failure(err)
		}
		if key != "" {
			kit.Toast(c, outcome, i18n.T(c, key))
		}
		if finished != nil {
			finished(err)
		}
	})
}

// backupActions are the page's actions; each acts on the Node the page shows.
func (r *Runtime) backupActions() *settings.BackupActions {
	state := &r.backupSettings
	// ready is the shown Node when it can take an action.
	ready := func() *Runtime {
		owner := state.shown
		if owner == nil || owner.backupSettings.busy || !owner.backupSettings.known {
			return nil
		}
		return owner
	}
	session := func(id string) (protocol.Session, bool) {
		for _, session := range state.shown.controller.Snapshot().Sessions {
			if session.ID.String() == id {
				return session, true
			}
		}
		return protocol.Session{}, false
	}
	picker := func(key string) {
		r.enqueue(func(c *ui.Context) { kit.Toast(c, kit.Failed, i18n.T(c, key)) })
	}
	return &settings.BackupActions{
		Save: func(target settings.BackupTarget, done func(bool)) {
			owner := ready()
			if owner == nil {
				done(false)
				return
			}
			saved := owner.backupSettings.saved
			if !backupStorageDirty(target, saved) {
				done(true)
				return
			}
			command := protocol.Command{Kind: "save_backup_settings", Data: backupStorageUpdate(target, saved)}
			owner.submitBackup(command, "backup_saved", backupFailure, func(err error) { done(err == nil) })
		},
		ChooseFolder: func(selected func(string)) {
			owner := state.shown
			r.browse(r.App.Host, func(path string) {
				if state.shown == owner {
					selected(path)
				}
			})
		},
		Export: func() {
			owner := ready()
			if owner == nil {
				return
			}
			if r.attachments == nil || r.attachments.native.SaveFile == nil {
				picker("files_download_picker")
				return
			}
			r.attachments.native.SaveFile("sailry-backup.age", func(path string) {
				if path != "" {
					r.enqueue(func(*ui.Context) { owner.downloadBackup(path) })
				}
			})
		},
		Now: func() {
			if owner := ready(); owner != nil {
				command := protocol.Command{Kind: "export_backup", Data: protocol.CommandExportBackupPayload{ExpectedRevision: owner.backupSettings.saved.Revision}}
				owner.submitBackup(command, "backup_exported", backupFailure, nil)
			}
		},
		Import: func(selected func(string)) {
			owner := ready()
			if owner == nil {
				return
			}
			if r.attachments == nil || r.attachments.native.PickFiles == nil {
				picker("files_upload_picker")
				return
			}
			r.attachments.native.PickFiles(func(paths []string) {
				if len(paths) == 0 {
					return
				}
				path := paths[0]
				r.enqueue(func(*ui.Context) {
					if state.shown == owner {
						selected(path)
					}
				})
			})
		},
		List: func() {
			if owner := ready(); owner != nil {
				owner.listBackups()
			}
		},
		Restore: func(restore settings.BackupRestore, done func(bool)) {
			owner := ready()
			if owner == nil {
				done(false)
				return
			}
			finished := func(err error) { done(err == nil) }
			if restore.File != "" {
				owner.restoreBackupFile(restore, finished)
				return
			}
			location := protocol.BackupLocationLocal
			if restore.Online {
				location = protocol.BackupLocationOnline
			}
			command := protocol.Command{Kind: "restore_backup", Data: protocol.CommandRestoreBackupPayload{ID: restore.ID,
				ExpectedRevision: owner.backupSettings.saved.Revision, Target: restore.Target, Passphrase: backupPassphrase(restore.Passphrase), Location: location}}
			owner.submitBackup(command, "backup_restored", backupFailure, finished)
		},
		Unarchive: func(id string) {
			if owner := ready(); owner != nil {
				if session, found := session(id); found {
					command := protocol.Command{Kind: "set_session_archived", Data: protocol.CommandSetSessionArchivedPayload{Session: session.ID, ExpectedRevision: session.Revision, Archived: false}}
					owner.submitBackup(command, "backup_unarchived", backupSessionFailure, nil)
				}
			}
		},
		Delete: func(id string) {
			if owner := ready(); owner != nil {
				if session, found := session(id); found {
					command := protocol.Command{Kind: "remove_session", Data: protocol.CommandRemoveSessionPayload{Session: session.ID, ExpectedRevision: session.Revision}}
					owner.submitBackup(command, "backup_deleted", backupSessionFailure, nil)
				}
			}
		},
		ArchiveAll: func() {
			if owner := ready(); owner != nil {
				if expected := backupRevisions(owner.controller.Snapshot().Sessions, false); len(expected) != 0 {
					command := protocol.Command{Kind: "archive_sessions", Data: protocol.CommandArchiveSessionsPayload{Expected: expected}}
					owner.submitBackup(command, "backup_archived", backupSessionFailure, nil)
				}
			}
		},
		DeleteAll: func() {
			if owner := ready(); owner != nil {
				if expected := backupRevisions(owner.controller.Snapshot().Sessions, true); len(expected) != 0 {
					command := protocol.Command{Kind: "remove_sessions", Data: protocol.CommandRemoveSessionsPayload{Expected: expected}}
					owner.submitBackup(command, "backup_deleted", backupSessionFailure, nil)
				}
			}
		},
	}
}

// backupPassphrase is a typed passphrase; blank uses the Node's saved one.
func backupPassphrase(value string) *protocol.Secret {
	if value == "" {
		return nil
	}
	secret := protocol.NewSecret(value)
	return &secret
}

// listBackups reads the storage's backups for the restore dialog.
func (r *Runtime) listBackups() {
	s := &r.backupSettings
	if s.listing {
		return
	}
	s.records, s.partial = nil, false
	s.listing = r.async(func() {
		output, err := r.controller.Do(r.ctx, protocol.Command{Kind: "list_backups"})
		page, ok := detail[protocol.BackupPage](output.Data)
		if err == nil && (output.Kind != "backups" || !ok) {
			err = fmt.Errorf("unexpected backups response")
		}
		r.enqueue(func(c *ui.Context) {
			s.listing = false
			if err != nil {
				outcome, key := backupFailure(err)
				kit.Toast(c, outcome, i18n.T(c, key))
				return
			}
			s.records, s.partial = backupRecords(page), page.Partial
		})
	})
}

// backupTransferEnded reports a transfer's end as a toast; a cancelled one
// shows none.
func (r *Runtime) backupTransferEnded(c *ui.Context, err error, done string) {
	switch {
	case err == nil:
		if done != "" {
			kit.Toast(c, kit.Done, i18n.T(c, done))
		}
	case errors.Is(err, context.Canceled) || faultCode(err) == protocol.ErrorCodeCancelled:
	default:
		outcome, key := backupFailure(err)
		kit.Toast(c, outcome, i18n.T(c, key))
	}
}

// downloadBackup exports a sealed archive to the path the user chose. The
// file appears there only once it is complete.
func (r *Runtime) downloadBackup(path string) {
	s := &r.backupSettings
	if s.busy || !s.known {
		return
	}
	revision := s.saved.Revision
	s.busy = r.async(func() {
		err := r.exportBackup(r.ctx, path, revision)
		r.enqueue(func(c *ui.Context) {
			s.busy, s.stale = false, true
			r.backupTransferEnded(c, err, "backup_exported")
		})
	})
}

func (r *Runtime) exportBackup(ctx context.Context, path string, revision uint64) error {
	target, err := prepareDownloadDestination(path)
	if err != nil {
		return err
	}
	defer target.close()
	output, err := r.controller.Do(ctx, protocol.Command{Kind: "download_backup", Data: protocol.CommandDownloadBackupPayload{ExpectedRevision: revision}})
	if err != nil {
		return err
	}
	download, ok := detail[protocol.BackupDownload](output.Data)
	if output.Kind != "backup_download" || !ok {
		return fmt.Errorf("unexpected backup download response")
	}
	if err = r.controller.Download(ctx, download.Stream, download.Size, download.Revision, target.file, func(uint64) {}); err != nil {
		return err
	}
	return target.publish(ctx)
}

// restoreBackupFile stages the chosen archive on the Node, then restores it
// into the new profile. A restore the Node refused releases what was staged.
func (r *Runtime) restoreBackupFile(restore settings.BackupRestore, finished func(error)) {
	s := &r.backupSettings
	revision := s.saved.Revision
	s.busy = r.async(func() {
		upload, err := r.stageBackup(r.ctx, restore.File)
		r.enqueue(func(c *ui.Context) {
			s.busy = false
			if err != nil {
				r.backupTransferEnded(c, err, "")
				finished(err)
				return
			}
			command := protocol.Command{Kind: "restore_backup_file", Data: protocol.CommandRestoreBackupFilePayload{Stream: upload.Stream,
				ExpectedRevision: revision, Target: restore.Target, Passphrase: backupPassphrase(restore.Passphrase)}}
			r.submitBackup(command, "backup_restored", backupFailure, func(err error) {
				if err != nil && faultCode(err) != protocol.ErrorCodeOutcomeUnknown {
					r.async(func() {
						_, _ = r.controller.Do(r.ctx, protocol.Command{Kind: "cancel_file_transfer", Data: &protocol.CommandCancelFileTransferPayload{Stream: upload.Stream}})
					})
				}
				finished(err)
			})
		})
	})
	if !s.busy {
		finished(context.Canceled)
	}
}

// stageBackup uploads a local archive to the Node's private staging.
func (r *Runtime) stageBackup(ctx context.Context, path string) (protocol.BackupUpload, error) {
	source, err := localSource(ctx, path, nil, "", backupUploadLimit)
	if err != nil {
		return protocol.BackupUpload{}, err
	}
	defer source.reader.Close()
	if source.size == 0 {
		return protocol.BackupUpload{}, fileActionFault(protocol.ErrorCodeInvalidRequest, "select a nonempty backup archive file")
	}
	output, err := r.controller.Do(ctx, protocol.Command{Kind: "upload_backup", Data: protocol.CommandUploadBackupPayload{Size: source.size, Revision: source.revision}})
	if err != nil {
		return protocol.BackupUpload{}, err
	}
	upload, ok := detail[protocol.BackupUpload](output.Data)
	if output.Kind != "backup_upload" || !ok {
		return protocol.BackupUpload{}, fmt.Errorf("unexpected backup upload response")
	}
	return upload, r.controller.Upload(ctx, upload.Stream, upload.Size, upload.Revision, source.reader, func(uint64) {})
}

// backupUploadLimit is the largest archive the Node accepts.
const backupUploadLimit = 1 << 40
