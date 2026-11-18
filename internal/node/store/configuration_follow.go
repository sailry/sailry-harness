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
