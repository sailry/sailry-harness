package store

import (
	"bytes"
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"math"
	"path/filepath"
	"slices"
	"strings"
	"unicode"

	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/node/backup"
	dispatcher "github.com/sailry/sailry-harness/internal/node/dispatch"
	"github.com/sailry/sailry-harness/internal/protocol"
)

const (
	backupHandler  = "profile-backup"
	backupSchedule = "40ca706e-25c1-45ca-bacf-61ee028fb608"
)

var backupWeekdays = []uint8{1, 2, 3, 4, 5, 6, 7}

func readBackup(ctx context.Context, db *sql.Conn) (backupStored, error) {
	value, err := readBody[backupStored](ctx, db, "SELECT body FROM backup_settings WHERE singleton=1")
	if err != nil {
		return backupStored{}, err
	}
	if value == nil {
		return backupStored{}, nil
	}
	return *value, validateBackup(*value)
}

func validateBackup(stored backupStored) error {
	settings, auth := stored.Settings, stored.Auth
	if settings.Revision > math.MaxInt64 {
		return invalid("invalid backup settings revision")
	}
	if settings.Destination != nil {
		if err := backup.ValidateDestination(*settings.Destination); err != nil {
			return err
		}
		if settings.Destination.Kind != protocol.BackupKindS3 && auth.SessionToken != nil {
			return invalid("backup session token requires S3")
		}
	}
	if settings.Local != nil {
		directory := settings.Local.Directory
		traverses := slices.ContainsFunc(strings.Split(filepath.ToSlash(directory), "/"), func(part string) bool { return part == "." || part == ".." })
		if !filepath.IsAbs(directory) || len(directory) > 4096 || strings.ContainsFunc(directory, unicode.IsControl) || traverses {
			return invalid("local backup directory must be an absolute path")
		}
	}
	if schedule := settings.Schedule; schedule != nil {
		if _, err := dispatcher.NewCalendar(schedule.Timezone, backupWeekdays, schedule.Hour, schedule.Minute); err != nil {
			return err
		}
		if schedule.Enabled {
			if !(settings.Local != nil && settings.Local.Enabled || settings.OnlineEnabled && settings.Destination != nil) {
				return invalid("automatic backup requires an enabled destination")
			}
			if _, err := backup.Passphrase(auth.Passphrase); err != nil {
				return err
			}
		}
	}
	for _, value := range []*protocol.Secret{auth.Identity, auth.Secret, auth.SessionToken, auth.Passphrase} {
		if value != nil && (value.Expose() == "" || len(value.Expose()) > 8192 || strings.ContainsFunc(value.Expose(), unicode.IsControl)) {
			return invalid("invalid backup credential")
		}
	}
	if settings.CredentialsConfigured != backupConfigured(auth) {
		return invalid("backup credential state differs from stored values")
	}
	if settings.PassphraseConfigured != (auth.Passphrase != nil) {
		return invalid("backup passphrase state differs from stored value")
	}
	return nil
}

func backupConfigured(auth backupAuth) bool {
	return auth.Identity != nil || auth.Secret != nil || auth.SessionToken != nil
}

// ReadBackupSettings returns the settings without any credential value.
func ReadBackupSettings(ctx context.Context, db *sql.Conn) (protocol.BackupSettings, error) {
	stored, err := readBackup(ctx, db)
	return stored.Settings, err
}

// SaveBackupSettings stores the destinations, credentials and schedule, and
// registers the Node's own scheduled export.
func SaveBackupSettings(ctx context.Context, db *sql.Conn, input protocol.BackupUpdate, timestamp int64) (protocol.BackupSettings, []protocol.Event, error) {
	stored, err := readBackup(ctx, db)
	if err != nil {
		return protocol.BackupSettings{}, nil, err
	}
	if err := checkRevision(stored.Settings.Revision, input.Settings.Revision); err != nil {
		return protocol.BackupSettings{}, nil, err
	}
	if input.Passphrase.Kind == "replace" {
		if value, ok := dispatchValue[protocol.Secret](input.Passphrase.Data); ok {
			if err := backup.ValidatePassphrase(value.Expose()); err != nil {
				return protocol.BackupSettings{}, nil, err
			}
		}
	}
	kind := func(destination *protocol.BackupDestination) protocol.BackupKind {
		if destination == nil {
			return ""
		}
		return destination.Kind
	}
	if kind(stored.Settings.Destination) != kind(input.Settings.Destination) {
		stored.Auth = backupAuth{Passphrase: stored.Auth.Passphrase}
	}
	for _, change := range []struct {
		current *(*protocol.Secret)
		update  protocol.PluginSettingsSecretUpdate
	}{
		{&stored.Auth.Identity, input.Identity}, {&stored.Auth.Secret, input.Secret},
		{&stored.Auth.SessionToken, input.SessionToken}, {&stored.Auth.Passphrase, input.Passphrase},
	} {
		switch change.update.Kind {
		case "replace":
			value, ok := dispatchValue[protocol.Secret](change.update.Data)
			if !ok {
				return protocol.BackupSettings{}, nil, invalid("secret replacement requires a value")
			}
			*change.current = &value
		case "clear":
			*change.current = nil
		}
	}
	// The last outcome is the Node's record, not a setting.
	last := stored.Settings.Last
	stored.Settings = input.Settings
	stored.Settings.Last = last
	if stored.Settings.Revision >= math.MaxInt64 {
		return protocol.BackupSettings{}, nil, invalid("backup settings revision is exhausted")
	}
	stored.Settings.Revision++
	stored.Settings.CredentialsConfigured = backupConfigured(stored.Auth)
	stored.Settings.PassphraseConfigured = stored.Auth.Passphrase != nil
	if err := validateBackup(stored); err != nil {
		return protocol.BackupSettings{}, nil, err
	}
	if err := configureBackupDispatch(ctx, db, stored.Settings, timestamp); err != nil {
		return protocol.BackupSettings{}, nil, err
	}
	body, err := encode(stored)
	if err != nil {
		return protocol.BackupSettings{}, nil, err
	}
	if _, err := db.ExecContext(ctx, "INSERT INTO backup_settings(singleton,body) VALUES(1,?1) ON CONFLICT(singleton) DO UPDATE SET body=excluded.body", body); err != nil {
		return protocol.BackupSettings{}, nil, err
	}
	return stored.Settings, []protocol.Event{{Kind: "backup_settings_changed", Data: protocol.EventBackupSettingsChangedPayload{Revision: stored.Settings.Revision}}}, nil
}

// RecordBackupOutcome keeps how the most recent backup ended, for the Backup
// page. It is not a settings change: the revision stays.
func RecordBackupOutcome(ctx context.Context, db *sql.Conn, outcome protocol.BackupOutcome) error {
	stored, err := readBackup(ctx, db)
	if err != nil {
		return err
	}
	stored.Settings.Last = &outcome
	body, err := encode(stored)
	if err != nil {
		return err
	}
	_, err = db.ExecContext(ctx, "INSERT INTO backup_settings(singleton,body) VALUES(1,?1) ON CONFLICT(singleton) DO UPDATE SET body=excluded.body", body)
	return err
}

// configureBackupDispatch registers the one controller-only scheduled command.
func configureBackupDispatch(ctx context.Context, db *sql.Conn, settings protocol.BackupSettings, timestamp int64) error {
	id, err := protocol.ParseScheduleID(backupSchedule)
	if err != nil {
		return err
	}
	previous, err := readBody[protocol.DispatchSchedule](ctx, db, "SELECT body FROM dispatch_schedules WHERE package=?1 AND id=?2", dispatchNativeOwner, backupSchedule)
	if err != nil {
		return err
	}
	if settings.Schedule == nil && previous == nil {
		return nil
	}
	schedule := protocol.DispatchSchedule{ID: id, Topic: backupHandler, Payload: json.RawMessage("null")}
	if previous != nil {
		schedule.Revision, schedule.Timing = previous.Revision, previous.Timing
	}
	if settings.Schedule != nil {
		schedule.Enabled = settings.Schedule.Enabled
		schedule.Timing = protocol.DispatchTiming{Kind: "weekly", Data: protocol.DispatchTimingWeeklyPayload{
			Timezone: settings.Schedule.Timezone, Weekdays: protocol.Bytes(backupWeekdays),
			Hour: settings.Schedule.Hour, Minute: settings.Schedule.Minute,
		}}
	}
	handler := protocol.DispatchHandler{
		Name: backupHandler, Enabled: schedule.Enabled, Queue: backupHandler,
		Source: protocol.DispatchSource{Package: dispatchNativeOwner, Topic: backupHandler},
		Callback: protocol.DispatchCallback{
			Completion: protocol.DispatchCompletionCommand, Bindings: map[string]string{},
			Command: &protocol.Command{Kind: "export_backup", Data: protocol.CommandExportBackupPayload{ExpectedRevision: settings.Revision}},
		},
	}
	registered, err := readBody[protocol.DispatchHandler](ctx, db, "SELECT body FROM dispatch_handlers WHERE package=?1 AND name=?2", dispatchNativeOwner, backupHandler)
	if err != nil {
		return err
	}
	if registered != nil {
		handler.Revision = registered.Revision
	}
	if handler.Revision >= math.MaxInt64 {
		return invalid("backup handler revision is exhausted")
	}
	handler.Revision++
	body, err := encode(handler)
	if err != nil {
		return err
	}
	if _, err := db.ExecContext(ctx, "INSERT INTO dispatch_handlers(package,name,source,topic,body,reference) VALUES(?1,?2,?1,?2,?3,?4) ON CONFLICT(package,name) DO UPDATE SET body=excluded.body,reference=excluded.reference",
		dispatchNativeOwner, backupHandler, body, []byte("null")); err != nil {
		return err
	}
	if err := cancelDispatchPending(ctx, db, dispatchNativeOwner, backupHandler, timestamp); err != nil {
		return err
	}
	_, err = saveDispatchSchedule(ctx, db, dispatchNativeOwner, schedule, timestamp)
	return err
}

// BackupPlan captures the configuration of one operation. A non-nil expected
// revision must match the stored one.
func BackupPlan(ctx context.Context, db *sql.Conn, expected *uint64) (backup.Plan, error) {
	stored, err := readBackup(ctx, db)
	if err != nil {
		return backup.Plan{}, err
	}
	if expected != nil {
		if err := checkRevision(stored.Settings.Revision, *expected); err != nil {
			return backup.Plan{}, err
		}
	}
	return backup.Plan{
		Destination: stored.Settings.Destination, Local: stored.Settings.Local, OnlineEnabled: stored.Settings.OnlineEnabled,
		Keep: stored.Settings.Keep,
		Auth: backup.Auth{
			Identity: stored.Auth.Identity, Secret: stored.Auth.Secret,
			SessionToken: stored.Auth.SessionToken, Passphrase: stored.Auth.Passphrase,
		},
	}, nil
}

// PrepareBackup refuses a durable backup command before it is admitted.
func PrepareBackup(ctx context.Context, db *sql.Conn, profile string, command protocol.Command) error {
	var revision uint64
	var passphrase *protocol.Secret
	var target *string
	switch input := command.Data.(type) {
	case *protocol.CommandExportBackupPayload:
		revision = input.ExpectedRevision
	case *protocol.CommandRestoreBackupPayload:
		revision, passphrase, target = input.ExpectedRevision, input.Passphrase, &input.Target
		if !backup.ValidIdentifier(input.ID) {
			return invalid("invalid backup identifier")
		}
	case *protocol.CommandRestoreBackupFilePayload:
		revision, passphrase, target = input.ExpectedRevision, input.Passphrase, &input.Target
	default:
		return invalid("backup operation expected")
	}
	plan, err := BackupPlan(ctx, db, &revision)
	if err != nil {
		return err
	}
	if target == nil {
		if !(plan.OnlineEnabled && plan.Destination != nil || plan.Local != nil && plan.Local.Enabled) {
			return fault(protocol.ErrorCodeNotConfigured, "backup destination is not configured")
		}
		_, err = backup.Passphrase(plan.Auth.Passphrase)
		return err
	}
	if passphrase == nil {
		passphrase = plan.Auth.Passphrase
	}
	if _, err := backup.Passphrase(passphrase); err != nil {
		return err
	}
	_, err = backup.TargetPath(*target, profile)
	return err
}

// BackupSource reads the plan and the profile-relative content roots of one
// snapshot, on the connection that pins the state the database copy is made
// from.
func BackupSource(ctx context.Context, db *sql.Conn, node protocol.NodeID, profile string, revision uint64) (backup.Plan, []string, error) {
	var identity []byte
	if err := db.QueryRowContext(ctx, "SELECT identity FROM node WHERE singleton=1").Scan(&identity); err != nil {
		return backup.Plan{}, nil, err
	}
	if !bytes.Equal(identity, node[:]) {
		return backup.Plan{}, nil, invalid("backup source identity changed")
	}
	plan, err := BackupPlan(ctx, db, &revision)
	if err != nil {
		return backup.Plan{}, nil, err
	}
	if _, err := backup.Passphrase(plan.Auth.Passphrase); err != nil {
		return backup.Plan{}, nil, err
	}
	roots := []string{"link.key"}
	for _, source := range []struct{ query, prefix string }{
		{"SELECT session || '/' || id FROM artifacts ORDER BY session,id", "artifacts/"},
		{"SELECT digest FROM plugin_packages ORDER BY digest", "plugins/packages/"},
		{"SELECT id FROM attachments ORDER BY id", "attachments/"},
		{"SELECT path FROM worktrees WHERE id NOT IN (SELECT worktree FROM removed_worktrees) ORDER BY path", ""},
	} {
		values, err := backupStrings(ctx, db, source.query)
		if err != nil {
			return backup.Plan{}, nil, err
		}
		for _, value := range values {
			if source.prefix != "" {
				roots = append(roots, source.prefix+value)
				continue
			}
			relative, err := filepath.Rel(profile, value)
			if err != nil || !filepath.IsLocal(relative) {
				continue
			}
			if !core.ManagedWorkspace(value, profile) {
				return backup.Plan{}, nil, invalid("worktree is outside a managed profile workspace")
			}
			roots = append(roots, filepath.ToSlash(relative))
		}
	}
	slices.Sort(roots)
	return plan, slices.Compact(roots), nil
}

func backupStrings(ctx context.Context, db *sql.Conn, query string) ([]string, error) {
	rows, err := db.QueryContext(ctx, query)
	if err != nil {
		return nil, err
	}
	var values []string
	for rows.Next() {
		var value string
		if err = rows.Scan(&value); err != nil {
			break
		}
		values = append(values, value)
	}
	return values, errors.Join(err, rows.Err(), rows.Close())
}

// Pause checkpoints and closes the database while keeping the exclusive
// lease, so nothing is admitted between the source read and the file copy.
func (exclusive *Exclusive) Pause(ctx context.Context) (*Closed, error) {
	handle := exclusive.handle
	var busy, log, checkpointed int
	if err := exclusive.Connection().QueryRowContext(ctx, "PRAGMA wal_checkpoint(TRUNCATE)").Scan(&busy, &log, &checkpointed); err != nil {
		return nil, err
	}
	if busy != 0 {
		return nil, errors.New("database checkpoint is busy")
	}
	handle.mu.Lock()
	state := handle.state
	handle.state = nil
	handle.mu.Unlock()
	closed := &Closed{handle: handle}
	// The Closed owner takes the existing permit without releasing it in between.
	exclusive.close.Do(func() {})
	if err := state.close(); err != nil {
		_ = closed.Close()
		return nil, err
	}
	return closed, nil
}
