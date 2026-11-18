package store

import (
	"context"
	"database/sql"
	"encoding/hex"
	"encoding/json"
	"errors"
	"math"
	"slices"
	"strings"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func configurationOrigin(ctx context.Context, db *sql.Conn, category, id string) (*protocol.ConfigurationOrigin, error) {
	user, err := CurrentUser(ctx, db)
	if err != nil {
		return nil, err
	}
	if user.Controller != nil && !user.Local {
		var revision uint64
		err := db.QueryRowContext(ctx, "SELECT revision FROM configuration_cache WHERE user=?1 AND category=?2 AND id=?3", user.ID.String(), category, id).Scan(&revision)
		if errors.Is(err, sql.ErrNoRows) {
			return nil, nil
		}
		if err != nil {
			return nil, err
		}
		return &protocol.ConfigurationOrigin{Node: *user.Controller, Revision: revision}, nil
	}
	var source []byte
	var revision uint64
	err = db.QueryRowContext(ctx, "SELECT source,revision FROM configuration_follow_records WHERE user=?1 AND category=?2 AND CASE WHEN local_id='' THEN id ELSE local_id END=?3 AND materialized=1", user.ID.String(), category, id).Scan(&source, &revision)
	if errors.Is(err, sql.ErrNoRows) {
		return nil, nil
	}
	if err != nil {
		return nil, err
	}
	if len(source) != 32 {
		return nil, errors.New("invalid configuration source")
	}
	return &protocol.ConfigurationOrigin{Node: protocol.NodeID(source), Revision: revision}, nil
}

func CheckConfigurationEditable(ctx context.Context, db *sql.Conn, category, id string) error {
	kind, nodeOwned := configurationKind(category)
	if !nodeOwned || !kind.LocalOnly {
		if _, cached, err := configurationCacheUser(ctx, db); err != nil {
			return err
		} else if cached {
			return fault(protocol.ErrorCodePermissionDenied, "configuration is managed by its controller")
		}
		if source, err := configurationOrigin(ctx, db, category, id); err != nil {
			return err
		} else if source != nil {
			return fault(protocol.ErrorCodePermissionDenied, "configuration is read-only on a following host")
		}
	}
	return nil
}

func ReadConfigurationFollow(ctx context.Context, db *sql.Conn) (protocol.ConfigurationFollowStatus, error) {
	status := protocol.ConfigurationFollowStatus{Targets: []protocol.ConfigurationFollowTarget{}, Issues: []protocol.ConfigurationFollowIssue{}}
	user, err := CurrentUser(ctx, db)
	if err != nil {
		return status, err
	}
	status.User = &user
	var source []byte
	err = db.QueryRowContext(ctx, "SELECT source,revision FROM configuration_follow_sources WHERE user=?1", user.ID.String()).Scan(&source, &status.Revision)
	if err != nil && !errors.Is(err, sql.ErrNoRows) {
		return status, err
	}
	if len(source) != 0 {
		if len(source) != 32 {
			return status, errors.New("invalid configuration source")
		}
		id := protocol.NodeID(source)
		status.Source = &id
	}
	if !user.Local && user.Controller != nil {
		status.Source = user.Controller
	}
	rows, err := db.QueryContext(ctx, "SELECT host,revision,configuration,secrets,failure FROM configuration_follow_targets WHERE user=?1 ORDER BY host", user.ID.String())
	if err != nil {
		return status, err
	}
	for rows.Next() {
		var target protocol.ConfigurationFollowTarget
		var host, body []byte
		if err := rows.Scan(&host, &target.Revision, &target.Configuration, &target.Secrets, &body); err != nil {
			rows.Close()
			return status, err
		}
		if len(host) != 32 {
			rows.Close()
			return status, errors.New("invalid follow target")
		}
		target.Host = protocol.NodeID(host)
		if len(body) > 0 {
			if err := json.Unmarshal(body, &target.Error); err != nil {
				rows.Close()
				return status, err
			}
		}
		status.Targets = append(status.Targets, target)
	}
	if err := errors.Join(rows.Err(), rows.Close()); err != nil {
		return status, err
	}
	if !user.Local && user.Controller != nil {
		// Legacy mirror conflicts and local overrides describe superseded
		// copies. The controller cache uses its own revision and failure state.
		return status, nil
	}
	rows, err = db.QueryContext(ctx, "SELECT category,CASE WHEN local_id='' THEN id ELSE local_id END,state,message FROM configuration_follow_records WHERE user=?1 AND state!='applied' ORDER BY category,id", user.ID.String())
	if err != nil {
		return status, err
	}
	for rows.Next() {
		var issue protocol.ConfigurationFollowIssue
		if err := rows.Scan(&issue.Category, &issue.ID, &issue.State, &issue.Message); err != nil {
			rows.Close()
			return status, err
		}
		status.Issues = append(status.Issues, issue)
	}
	if err := errors.Join(rows.Err(), rows.Close()); err != nil {
		return status, err
	}
	rows, err = db.QueryContext(ctx, "SELECT plugin,server FROM configuration_follow_mcp_overrides WHERE user=?1 ORDER BY plugin,server", user.ID.String())
	if err != nil {
		return status, err
	}
	defer rows.Close()
	for rows.Next() {
		var value protocol.ConfigurationMCPLocal
		if err := rows.Scan(&value.Plugin, &value.Server); err != nil {
			return status, err
		}
		status.MCPLocal = append(status.MCPLocal, value)
	}
	return status, rows.Err()
}

func EnsureConfigurationTarget(ctx context.Context, db *sql.Conn, host protocol.NodeID) error {
	user, err := CurrentUser(ctx, db)
	if err != nil {
		return err
	}
	if !user.Local {
		return fault(protocol.ErrorCodePermissionDenied, "configuration source is not local")
	}
	_, err = db.ExecContext(ctx, "INSERT INTO configuration_follow_targets(user,host,revision,configuration,secrets) VALUES(?1,?2,1,1,1) ON CONFLICT(user,host) DO UPDATE SET revision=revision+1,configuration=1,secrets=1 WHERE configuration=0 OR secrets=0", user.ID.String(), host[:])
	return err
}

func RecordConfigurationTargetFailure(ctx context.Context, db *sql.Conn, host protocol.NodeID, failure *protocol.Fault) error {
	user, err := CurrentUser(ctx, db)
	if err != nil {
		return err
	}
	var body any
	if failure != nil {
		raw, err := encode(failure)
		if err != nil {
			return err
		}
		body = raw
	}
	_, err = db.ExecContext(ctx, "UPDATE configuration_follow_targets SET failure=?3 WHERE user=?1 AND host=?2", user.ID.String(), host[:], body)
	return err
}

func ConfigurationFollowDelivery(ctx context.Context, db *sql.Conn, host protocol.NodeID, manifest protocol.ConfigurationManifest) (protocol.Request, error) {
	request, err := ConfigurationCommandDelivery(ctx, db, host, manifest.Category, &protocol.ConfigurationFollowCommand{Action: "apply", Manifest: &manifest})
	if err != nil {
		return protocol.Request{}, err
	}
	return *request, nil
}

func CompleteConfigurationFollowDelivery(ctx context.Context, db *sql.Conn, host protocol.NodeID, category string, request protocol.RequestID) error {
	user, err := CurrentUser(ctx, db)
	if err != nil {
		return err
	}
	var raw []byte
	err = db.QueryRowContext(ctx, "SELECT request FROM configuration_follow_deliveries WHERE user=?1 AND host=?2 AND category=?3", user.ID.String(), host[:], category).Scan(&raw)
	if errors.Is(err, sql.ErrNoRows) {
		return nil
	}
	if err != nil {
		return err
	}
	var saved protocol.Request
	if err := json.Unmarshal(raw, &saved); err != nil {
		return err
	}
	if saved.ID != request {
		return fault(protocol.ErrorCodeRevisionConflict, "follow delivery changed")
	}
	_, err = db.ExecContext(ctx, "DELETE FROM configuration_follow_deliveries WHERE user=?1 AND host=?2 AND category=?3", user.ID.String(), host[:], category)
	return err
}

func ReadConfigurationManifest(ctx context.Context, db *sql.Conn, source protocol.NodeID, category string) (protocol.ConfigurationManifest, error) {
	if kind, ok := configurationKind(category); !ok || kind.LocalOnly {
		return protocol.ConfigurationManifest{}, invalid("unsupported configuration category")
	}
	user, err := CurrentUser(ctx, db)
	if err != nil {
		return protocol.ConfigurationManifest{}, err
	}
	value := protocol.ConfigurationManifest{Source: source, Category: category, Records: []protocol.ConfigurationFollowRecord{}}
	err = db.QueryRowContext(ctx, "SELECT revision FROM configuration_follow_versions WHERE user=?1 AND category=?2 AND source=?3", user.ID.String(), category, source[:]).Scan(&value.Revision)
	if err != nil && !errors.Is(err, sql.ErrNoRows) {
		return value, err
	}
	query := "SELECT id,revision,digest FROM configuration_follow_records WHERE user=?1 AND source=?2 AND category=?3 AND state IN ('applied','authorization') ORDER BY id"
	if user.Controller != nil && !user.Local {
		if *user.Controller != source {
			return value, fault(protocol.ErrorCodePermissionDenied, "configuration belongs to another controller")
		}
		query = "SELECT id,revision,digest FROM configuration_cache WHERE user=?1 AND ?2 IS NOT NULL AND category=?3 ORDER BY id"
	}
	rows, err := db.QueryContext(ctx, query, user.ID.String(), source[:], category)
	if err != nil {
		return value, err
	}
	defer rows.Close()
	for rows.Next() {
		var record protocol.ConfigurationFollowRecord
		if err := rows.Scan(&record.ID, &record.Revision, &record.Digest); err != nil {
			return value, err
		}
		value.Records = append(value.Records, record)
	}
	if err := rows.Err(); err != nil {
		return value, err
	}
	if category == "runtime_credentials" && user.Controller != nil && *user.Controller == source {
		value.Captured = []protocol.CapturedModelCredential{}
		if slices.ContainsFunc(value.Records, func(record protocol.ConfigurationFollowRecord) bool {
			return strings.HasPrefix(record.ID, "provider:")
		}) {
			value.Captured, err = capturedRuntimeCredentials(ctx, db, user.ID, source)
		}
	}
	return value, err
}

func validateConfigurationManifest(value protocol.ConfigurationManifest) error {
	kind, valid := configurationKind(value.Category)
	if valid && kind.LocalOnly {
		valid = false
	}
	if !valid || value.Revision == 0 || value.Revision > math.MaxInt64 || len(value.Records) > 4096 {
		return invalid("configuration manifest is invalid")
	}
	seen := map[string]bool{}
	for _, record := range value.Records {
		digest, err := hex.DecodeString(record.Digest)
		if record.ID == "" || len(record.ID) > 256 || record.Revision == 0 || record.Revision > math.MaxInt64 || err != nil || len(digest) != 32 || seen[record.ID] {
			return invalid("configuration manifest record is invalid")
		}
		seen[record.ID] = true
		if len(record.Body) > 1024*1024 {
			return invalid("configuration record exceeds its limit")
		}
	}
	return nil
}

func ConfigurationMCPServerLocal(ctx context.Context, db *sql.Conn, plugin, server string) (bool, error) {
	if origin, err := configurationOrigin(ctx, db, "plugin_settings", plugin); err != nil {
		return false, err
	} else if origin != nil {
		return false, nil
	}
	user, err := CurrentUser(ctx, db)
	if err != nil {
		return false, err
	}
	var local bool
	err = db.QueryRowContext(ctx, "SELECT EXISTS(SELECT 1 FROM configuration_follow_mcp_overrides WHERE user=?1 AND plugin=?2 AND server=?3)", user.ID.String(), plugin, server).Scan(&local)
	return local, err
}
