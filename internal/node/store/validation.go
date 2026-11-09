package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"math"
	"net/url"
	"strings"

	"github.com/sailry/sailry-harness/internal/protocol"
)

type networkStored struct {
	Revision  uint64                    `json:"revision"`
	Selection protocol.NetworkSelection `json:"selection"`
}

func (value *networkStored) UnmarshalJSON(data []byte) error {
	type plain networkStored
	var decoded plain
	if err := protocol.DecodeObject(data, &decoded, []string{"revision", "selection"}, true); err != nil {
		return err
	}
	*value = networkStored(decoded)
	return nil
}

type backgroundStored struct {
	Revision         uint64  `json:"revision"`
	Resume           bool    `json:"resume"`
	ChildConcurrency *uint64 `json:"child_concurrency,omitempty"`
}

func (value *backgroundStored) UnmarshalJSON(data []byte) error {
	type plain backgroundStored
	var decoded plain
	if err := protocol.DecodeObject(data, &decoded, []string{"revision", "resume"}, true); err != nil {
		return err
	}
	*value = backgroundStored(decoded)
	return nil
}

type backupAuth struct {
	Identity     *protocol.Secret `json:"identity"`
	Secret       *protocol.Secret `json:"secret"`
	SessionToken *protocol.Secret `json:"session_token"`
	Passphrase   *protocol.Secret `json:"passphrase"`
}

func (value *backupAuth) UnmarshalJSON(data []byte) error {
	type plain backupAuth
	var decoded plain
	if err := protocol.DecodeObject(data, &decoded, nil, true); err != nil {
		return err
	}
	*value = backupAuth(decoded)
	return nil
}

type backupStored struct {
	Settings protocol.BackupSettings `json:"settings"`
	Auth     backupAuth              `json:"auth"`
}

func (value *backupStored) UnmarshalJSON(data []byte) error {
	type plain backupStored
	var decoded plain
	if err := protocol.DecodeObject(data, &decoded, []string{"settings", "auth"}, true); err != nil {
		return err
	}
	*value = backupStored(decoded)
	return nil
}

const liveTurns = `SELECT turn FROM agent_pending
 UNION SELECT turn FROM agent_runs WHERE json_extract(body,'$.status') IN ('queued','running','stopping')
 UNION SELECT turn FROM agent_approvals WHERE json_extract(body,'$.state')='pending'
 UNION SELECT turn FROM agent_questions WHERE json_extract(body,'$.state.kind')='pending'
 UNION SELECT turn FROM agent_background WHERE state IN ('running','pending')
 UNION SELECT delivery FROM agent_background WHERE state IN ('running','pending') AND delivery IS NOT NULL`

func validateStored(ctx context.Context, connection *sql.Conn) error {
	return validateRecords(ctx, connection, true)
}

func validateStartup(ctx context.Context, connection *sql.Conn) error {
	return validateRecords(ctx, connection, false)
}

func validateRecords(ctx context.Context, connection *sql.Conn, archives bool) error {
	for _, record := range []struct {
		table, column string
		factory       func() any
	}{
		{"telemetry_settings", "body", func() any { return new(protocol.TelemetrySettings) }},
		{"power_settings", "body", func() any { return new(protocol.PowerSettings) }},
		{"compaction_settings", "body", func() any { return new(protocol.ConversationCompactionSettings) }},
		{"network_settings", "body", func() any { return new(networkStored) }},
		{"search_settings", "body", func() any { return new(searchStored) }},
		{"background_settings", "body", func() any { return new(backgroundStored) }},
		{"backup_settings", "body", func() any { return new(backupStored) }},
		{"sandbox_settings", "body", func() any { return new(protocol.SandboxSettings) }},
		{"local_model_settings", "body", func() any { return new(protocol.ModelsLocalSettings) }},
		{"terminal_settings", "body", func() any { return new(protocol.TerminalSettings) }},
		{"database_profiles", "password_target", func() any { return new(databaseCredentialBinding) }},
		{"team_settings", "body", func() any { return new(protocol.TeamSettings) }},
		{"hook_settings", "body", func() any { return new(protocol.HooksSettings) }},
		{"roles", "body", func() any { return new(protocol.RoleProfile) }},
		{"teams", "body", func() any { return new(protocol.TeamProfile) }},
		{"artifacts", "body", func() any { return new(protocol.ArtifactSummary) }},
		{"artifact_revisions", "body", func() any { return new(protocol.ArtifactSummary) }},
		{"defaults", "config", func() any { return new(protocol.SessionConfig) }},
		{"session_revisions", "config", func() any { return new(protocol.SessionConfig) }},
		{"session_revisions", "roles", func() any { return new(protocol.RoleSnapshot) }},
		{"session_revisions", "profile", func() any { return new(protocol.SessionProfile) }},
		{"notifications", "body", func() any { return new(protocol.NotificationNotice) }},
		{"dispatch_handlers", "body", func() any { return new(protocol.DispatchHandler) }},
		{"dispatch_schedules", "body", func() any { return new(protocol.DispatchSchedule) }},
		{"dispatch_jobs", "body", func() any { return new(protocol.DispatchJob) }},
		{"dispatch_jobs", "callback", func() any { return new(protocol.DispatchCallback) }},
		{"plugins", "body", func() any { return new(protocol.PluginInfo) }},
		{"plugin_packages", "body", func() any { return new(protocol.PluginInfo) }},
	} {
		query := "SELECT " + record.column + " FROM " + record.table + " WHERE " + record.column + " IS NOT NULL"
		if !archives {
			switch record.table {
			case "artifact_revisions":
				continue
			case "session_revisions":
				query += " AND (session,revision) IN (SELECT id,revision FROM sessions WHERE state!='removed' UNION SELECT session,revision FROM turns WHERE id IN (" + liveTurns + "))"
			case "dispatch_jobs":
				query += " AND status IN ('queued','running')"
			case "plugin_packages":
				query = "SELECT p.body FROM plugin_packages p JOIN plugins c ON json_extract(c.body,'$.summary.digest')=p.digest WHERE c.installed=1"
			}
		}
		rows, err := connection.QueryContext(ctx, query)
		if err != nil {
			return err
		}
		for rows.Next() {
			var body []byte
			if err := rows.Scan(&body); err != nil {
				rows.Close()
				return err
			}
			value := record.factory()
			if err := json.Unmarshal(body, value); err != nil {
				rows.Close()
				return storedError(record.table+"."+record.column, err)
			}
			if err := validateConfiguration(value); err != nil {
				rows.Close()
				return storedError(record.table+"."+record.column, err)
			}
		}
		err = rows.Err()
		rows.Close()
		if err != nil {
			return err
		}
	}
	var count int
	if err := connection.QueryRowContext(ctx, "SELECT count(*) FROM local_model_settings WHERE singleton=1").Scan(&count); err != nil {
		return err
	}
	if count != 1 {
		return errors.New("local model settings are missing")
	}
	if err := connection.QueryRowContext(ctx, "SELECT count(*) FROM telemetry_settings WHERE singleton=1").Scan(&count); err != nil {
		return err
	}
	if count != 1 {
		return errors.New("telemetry settings are missing")
	}
	return validateCounters(ctx, connection)
}

func validateConfiguration(value any) error {
	switch saved := value.(type) {
	case *protocol.TelemetrySettings:
		return validateTelemetry(*saved)
	case *protocol.ConversationCompactionSettings:
		if saved.Percent < 1 || saved.Percent > 99 {
			return errors.New("compaction percentage must be between 1 and 99")
		}
	case *protocol.TeamSettings:
		if saved.ChildConcurrency == 0 {
			return errors.New("child concurrency must be a positive integer")
		}
	case *backgroundStored:
		if saved.ChildConcurrency != nil && *saved.ChildConcurrency == 0 {
			return errors.New("invalid retained child concurrency")
		}
	case *searchStored:
		describeSearch(&saved.Settings)
		return validateSearch(*saved)
	case *protocol.ModelsLocalSettings:
		if saved.Revision > math.MaxInt64 {
			return errors.New("invalid local model settings")
		}
		for kind, choice := range saved.Choices {
			if kind == protocol.ModelsKindSpeech || choice.Model != nil && !validModelID(*choice.Model) {
				return errors.New("invalid local model settings")
			}
		}
		if saved.Mirror != nil {
			mirror, err := url.Parse(*saved.Mirror)
			if err != nil || len(*saved.Mirror) > 4096 || mirror.Scheme != "https" || mirror.Hostname() == "" || mirror.RawQuery != "" || mirror.ForceQuery || strings.Contains(*saved.Mirror, "#") {
				return errors.New("model mirror requires an HTTPS URL without credentials")
			}
			if mirror.User != nil {
				_, password := mirror.User.Password()
				if mirror.User.Username() != "" || password {
					return errors.New("model mirror requires an HTTPS URL without credentials")
				}
			}
		}
	}
	return nil
}

func validModelID(value string) bool {
	if len(value) == 0 || len(value) > 128 {
		return false
	}
	for _, character := range []byte(value) {
		if (character < 'a' || character > 'z') && (character < '0' || character > '9') && character != '-' {
			return false
		}
	}
	return true
}
