package store

import (
	"context"
	"database/sql"
	"encoding/hex"
	"encoding/json"
	"errors"
	"math"

	"github.com/sailry/sailry-harness/internal/protocol"
)

var ConfigurationCategories = []string{"plugins", "providers", "roles", "teams", "ssh_profiles", "database_profiles", "defaults", "team_settings", "plugin_settings", "connections"}

type configurationApplyKey struct{}

func configurationApplyContext(ctx context.Context) context.Context {
	return context.WithValue(ctx, configurationApplyKey{}, true)
}
func configurationApplying(ctx context.Context) bool {
	value, _ := ctx.Value(configurationApplyKey{}).(bool)
	return value
}

func configurationOrigin(ctx context.Context, db *sql.Conn, category, id string) (*protocol.ConfigurationOrigin, error) {
	user, err := CurrentUser(ctx, db)
	if err != nil {
		return nil, err
	}
	var source []byte
	var revision uint64
	err = db.QueryRowContext(ctx, "SELECT source,revision FROM configuration_follow_records WHERE user=?1 AND category=?2 AND id=?3 AND materialized=1", user.ID.String(), category, id).Scan(&source, &revision)
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
	if configurationApplying(ctx) {
		return nil
	}
	source, err := configurationOrigin(ctx, db, category, id)
	if err != nil {
		return err
	}
	if source != nil {
		return fault(protocol.ErrorCodePermissionDenied, "configuration is read-only on a following host")
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
	rows, err = db.QueryContext(ctx, "SELECT category,id,state,message FROM configuration_follow_records WHERE user=?1 AND state!='applied' ORDER BY category,id", user.ID.String())
	if err != nil {
		return status, err
	}
	defer rows.Close()
	for rows.Next() {
		var issue protocol.ConfigurationFollowIssue
		if err := rows.Scan(&issue.Category, &issue.ID, &issue.State, &issue.Message); err != nil {
			return status, err
		}
		status.Issues = append(status.Issues, issue)
	}
	return status, rows.Err()
}

func SetConfigurationSource(ctx context.Context, db *sql.Conn, node, caller protocol.NodeID, source *protocol.NodeID, expected uint64) error {
	user, err := CurrentUser(ctx, db)
	if err != nil {
		return err
	}
	current, err := ReadConfigurationFollow(ctx, db)
	if err != nil {
		return err
	}
	if err := checkRevision(current.Revision, expected); err != nil {
		return err
	}
	if source == nil {
		revision, err := nextRevision(current.Revision)
		if err != nil {
			return err
		}
		_, err = db.ExecContext(ctx, "INSERT INTO configuration_follow_sources(user,source,revision) VALUES(?1,NULL,?2) ON CONFLICT(user) DO UPDATE SET source=NULL,revision=excluded.revision", user.ID.String(), revision)
		return err
	}
	if *source == node && !user.Local {
		return fault(protocol.ErrorCodePermissionDenied, "only the local user can publish from this Node")
	}
	if *source != node {
		member, err := CallerUser(ctx, db, node, *source)
		if err != nil || member.ID != user.ID {
			return fault(protocol.ErrorCodePermissionDenied, "source host must belong to this user")
		}
	}
	if current.Source == nil || *current.Source != *source {
		var copied bool
		if err := db.QueryRowContext(ctx, "SELECT EXISTS(SELECT 1 FROM configuration_follow_records WHERE user=?1 AND materialized=1 AND source!=?2)", user.ID.String(), source[:]).Scan(&copied); err != nil {
			return err
		}
		if copied {
			return fault(protocol.ErrorCodeConflict, "detach existing configuration copies before changing source")
		}
	}
	revision, err := nextRevision(current.Revision)
	if err != nil {
		return err
	}
	_, err = db.ExecContext(ctx, "INSERT INTO configuration_follow_sources(user,source,revision) VALUES(?1,?2,?3) ON CONFLICT(user) DO UPDATE SET source=excluded.source,revision=excluded.revision", user.ID.String(), source[:], revision)
	return err
}

func SaveConfigurationTarget(ctx context.Context, db *sql.Conn, target protocol.ConfigurationFollowTarget, expected uint64) error {
	user, err := CurrentUser(ctx, db)
	if err != nil {
		return err
	}
	if !user.Local {
		return fault(protocol.ErrorCodePermissionDenied, "only this Node's local user can publish configuration")
	}
	var outbound bool
	if err := db.QueryRowContext(ctx, "SELECT EXISTS(SELECT 1 FROM link_peers p LEFT JOIN identity_peer_origins o ON o.peer=p.identity WHERE p.identity=?1 AND p.direction IN ('host','both') AND coalesce(o.direct,1)=1 AND coalesce(o.removed,0)=0)", target.Host[:]).Scan(&outbound); err != nil {
		return err
	}
	if !outbound {
		return fault(protocol.ErrorCodePermissionDenied, "configuration target is not a paired outbound host")
	}
	var revision uint64
	err = db.QueryRowContext(ctx, "SELECT revision FROM configuration_follow_targets WHERE user=?1 AND host=?2", user.ID.String(), target.Host[:]).Scan(&revision)
	if err != nil && !errors.Is(err, sql.ErrNoRows) {
		return err
	}
	if err := checkRevision(revision, expected); err != nil {
		return err
	}
	target.Revision, err = nextRevision(revision)
	if err != nil {
		return err
	}
	_, err = db.ExecContext(ctx, "INSERT INTO configuration_follow_targets(user,host,revision,configuration,secrets) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(user,host) DO UPDATE SET revision=excluded.revision,configuration=excluded.configuration,secrets=excluded.secrets,failure=NULL", user.ID.String(), target.Host[:], target.Revision, target.Configuration, target.Secrets)
	return err
}

// EnsureConfigurationTarget is called only after a direct outbound host reports
// the authenticated receiving user's role. Public hosts default to no keys.
func EnsureConfigurationTarget(ctx context.Context, db *sql.Conn, host protocol.NodeID, administrator bool) error {
	user, err := CurrentUser(ctx, db)
	if err != nil {
		return err
	}
	if !user.Local {
		return fault(protocol.ErrorCodePermissionDenied, "configuration source is not local")
	}
	_, err = db.ExecContext(ctx, "INSERT INTO configuration_follow_targets(user,host,revision,configuration,secrets) VALUES(?1,?2,1,1,?3) ON CONFLICT(user,host) DO NOTHING", user.ID.String(), host[:], administrator)
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
	user, err := CurrentUser(ctx, db)
	if err != nil {
		return protocol.Request{}, err
	}
	var raw []byte
	err = db.QueryRowContext(ctx, "SELECT request FROM configuration_follow_deliveries WHERE user=?1 AND host=?2 AND category=?3", user.ID.String(), host[:], manifest.Category).Scan(&raw)
	if err == nil {
		var request protocol.Request
		err = json.Unmarshal(raw, &request)
		return request, err
	}
	if !errors.Is(err, sql.ErrNoRows) {
		return protocol.Request{}, err
	}
	request := protocol.NewRequest(host, protocol.Command{Kind: "configuration_follow", Data: &protocol.ConfigurationFollowCommand{Action: "apply", Manifest: &manifest}})
	raw, err = encode(request)
	if err != nil {
		return request, err
	}
	_, err = db.ExecContext(ctx, "INSERT INTO configuration_follow_deliveries(user,host,category,request) VALUES(?1,?2,?3,?4)", user.ID.String(), host[:], manifest.Category, raw)
	return request, err
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
	user, err := CurrentUser(ctx, db)
	if err != nil {
		return protocol.ConfigurationManifest{}, err
	}
	value := protocol.ConfigurationManifest{Source: source, Category: category, Records: []protocol.ConfigurationFollowRecord{}}
	err = db.QueryRowContext(ctx, "SELECT revision FROM configuration_follow_versions WHERE user=?1 AND category=?2 AND source=?3", user.ID.String(), category, source[:]).Scan(&value.Revision)
	if err != nil && !errors.Is(err, sql.ErrNoRows) {
		return value, err
	}
	rows, err := db.QueryContext(ctx, "SELECT id,revision,digest FROM configuration_follow_records WHERE user=?1 AND source=?2 AND category=?3 AND state IN ('applied','authorization') ORDER BY id", user.ID.String(), source[:], category)
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
	return value, rows.Err()
}

func validateConfigurationManifest(value protocol.ConfigurationManifest) error {
	valid := false
	for _, category := range ConfigurationCategories {
		valid = valid || value.Category == category
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
	user, err := CurrentUser(ctx, db)
	if err != nil {
		return false, err
	}
	var local bool
	err = db.QueryRowContext(ctx, "SELECT EXISTS(SELECT 1 FROM configuration_follow_mcp_overrides WHERE user=?1 AND plugin=?2 AND server=?3)", user.ID.String(), plugin, server).Scan(&local)
	return local, err
}
func SetConfigurationMCPServerLocal(ctx context.Context, db *sql.Conn, plugin, server string, local bool) error {
	user, err := CurrentUser(ctx, db)
	if err != nil {
		return err
	}
	info, err := ReadPlugin(ctx, db, plugin)
	if err != nil {
		return err
	}
	if info == nil {
		return fault(protocol.ErrorCodeNotFound, "plugin is not installed")
	}
	declared := false
	for _, entry := range info.Mcp {
		declared = declared || entry.Name == server
	}
	if !declared {
		return invalid("MCP server is not declared by this package")
	}
	if local {
		_, err = db.ExecContext(ctx, "INSERT INTO configuration_follow_mcp_overrides(user,plugin,server) VALUES(?1,?2,?3) ON CONFLICT(user,plugin,server) DO NOTHING", user.ID.String(), plugin, server)
	} else {
		_, err = db.ExecContext(ctx, "DELETE FROM configuration_follow_mcp_overrides WHERE user=?1 AND plugin=?2 AND server=?3", user.ID.String(), plugin, server)
	}
	return err
}
