package store

import (
	"context"
	"database/sql"
	"errors"
	"slices"
	"strings"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// The execution Node reports the immutable model, role and media captures for
// its unfinished work, including the effective credential after recovery.
func capturedRuntimeCredentials(ctx context.Context, db *sql.Conn, user protocol.UserID, source protocol.NodeID) ([]protocol.CapturedModelCredential, error) {
	encodedSource, err := encode(source)
	if err != nil {
		return nil, err
	}
	rows, err := db.QueryContext(ctx, `WITH active AS (
 SELECT a.turn,a.body,a.provider,a.media,r.config,t.roles FROM agent_runs a
 JOIN turns t ON t.id=a.turn JOIN session_revisions r ON r.session=t.session AND r.revision=t.revision
 WHERE t.user=?1 AND (json_extract(a.body,'$.status') IN ('running','queued','stopping') OR
  (json_extract(a.body,'$.status') IN ('failed','cancelled','interrupted') AND EXISTS (SELECT 1 FROM agent_pending p WHERE p.turn=a.turn)
   AND NOT EXISTS (SELECT 1 FROM agent_events e WHERE e.turn=a.turn AND json_extract(e.body,'$.provenance.author')='user')))
), refs(ref,waiting) AS (
 SELECT json_extract(config,'$.credential'),0 FROM active
 UNION ALL SELECT json_extract(provider,'$.credential'),0 FROM active
 UNION ALL SELECT json_extract(p.value,'$.credential'),0 FROM active,json_each(active.roles,'$.providers') p
 UNION ALL SELECT json_extract(m.value,'$.provider.credential'),0 FROM active,json_each(active.media) m
 UNION ALL SELECT json_extract((SELECT c.body FROM agent_credential_recoveries c WHERE c.turn=active.turn ORDER BY c.sequence DESC LIMIT 1),'$.replacement'),0 FROM active
 UNION ALL SELECT json_extract(body,'$.waiting.credential'),1 FROM active WHERE json_extract(body,'$.waiting.kind')='controller_login'
)
SELECT json_extract(ref,'$.id'),max(waiting) FROM refs WHERE json_extract(ref,'$.node')=?2 GROUP BY json_extract(ref,'$.id') ORDER BY 1`, user.String(), string(encodedSource))
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	captures := []protocol.CapturedModelCredential{}
	for rows.Next() {
		var capture protocol.CapturedModelCredential
		var id string
		if err := rows.Scan(&id, &capture.Waiting); err != nil {
			return nil, err
		}
		capture.ID, err = protocol.ParseCredentialID(id)
		if err != nil {
			return nil, err
		}
		captures = append(captures, capture)
	}
	return captures, rows.Err()
}

// A peer's capture declaration can only retain credentials in the last
// inventory this controller actually exported to that particular peer.
func authorizedRuntimeCaptures(ctx context.Context, db *sql.Conn, user protocol.User, host, source protocol.NodeID, known protocol.ConfigurationManifest) (map[protocol.CredentialID]runtimeCapture, error) {
	if known.Source != source || known.Category != "runtime_credentials" || known.Revision == 0 {
		return nil, nil
	}
	var digest string
	err := db.QueryRowContext(ctx, "SELECT digest FROM configuration_follow_versions WHERE user=?1 AND source=?2 AND category='runtime_credentials' AND revision=?3", user.ID.String(), host[:], known.Revision).Scan(&digest)
	if errors.Is(err, sql.ErrNoRows) {
		return nil, nil
	}
	if err != nil {
		return nil, err
	}
	actual, err := configurationManifestDigest(known.Records)
	if err != nil || actual != digest {
		return nil, err
	}
	owned := map[protocol.CredentialID]protocol.ConfigurationFollowRecord{}
	for _, item := range known.Records {
		if raw, ok := strings.CutPrefix(item.ID, "provider:"); ok {
			id, err := protocol.ParseCredentialID(raw)
			if err != nil {
				return nil, err
			}
			owned[id] = protocol.ConfigurationFollowRecord{ID: item.ID, Revision: item.Revision, Digest: item.Digest}
		}
	}
	captures := map[protocol.CredentialID]runtimeCapture{}
	if known.Captured == nil {
		for id, item := range owned {
			captures[id] = runtimeCapture{legacy: &item}
		}
		return captures, nil
	}
	for _, ref := range known.Captured {
		if _, ok := owned[ref.ID]; ok {
			captures[ref.ID] = runtimeCapture{waiting: ref.Waiting}
		}
	}
	return captures, nil
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

// ExportConfigurationManifest exports only the source Node's local personal
// rows. A receiver's replicas never become another user's onward source.
func ExportConfigurationManifest(ctx context.Context, db *sql.Conn, node, host protocol.NodeID, category string, secrets bool, declarations map[string]protocol.PluginMcpConfiguration, known *protocol.ConfigurationManifest) (protocol.ConfigurationManifest, error) {
	value := protocol.ConfigurationManifest{Source: node, Category: category, Records: []protocol.ConfigurationFollowRecord{}}
	user, err := CurrentUser(ctx, db)
	if err != nil {
		return value, err
	}
	if !user.Local {
		return value, fault(protocol.ErrorCodePermissionDenied, "only local configuration can be published")
	}
	kind, ok := configurationKind(category)
	if !ok || kind.Export == nil || kind.LocalOnly {
		return value, invalid("unsupported configuration category")
	}
	if category != "runtime_credentials" {
		return value, fault(protocol.ErrorCodeNotConfigured, "configuration is managed on its owning Node")
	}
	options := ConfigurationExport{Node: node, Secrets: secrets, Declarations: declarations}
	if category == "runtime_credentials" && known != nil && len(known.Records) != 0 {
		options.Captured, err = authorizedRuntimeCaptures(ctx, db, user, host, node, *known)
		if err != nil {
			return value, err
		}
	}
	value.Records, err = kind.Export(ctx, db, options)
	if err != nil {
		return value, err
	}
	digest, err := configurationManifestDigest(value.Records)
	if err != nil {
		return value, err
	}
	slices.SortFunc(value.Records, func(a, b protocol.ConfigurationFollowRecord) int { return strings.Compare(a.ID, b.ID) })
	var previous string
	err = db.QueryRowContext(ctx, "SELECT revision,digest FROM configuration_follow_versions WHERE user=?1 AND category=?2 AND source=?3", user.ID.String(), category, host[:]).Scan(&value.Revision, &previous)
	if err != nil && !errors.Is(err, sql.ErrNoRows) {
		return value, err
	}
	if previous != digest {
		value.Revision, err = nextRevision(value.Revision)
		if err != nil {
			return value, err
		}
		_, err = db.ExecContext(ctx, "INSERT INTO configuration_follow_versions(user,category,source,revision,digest) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(user,category,source) DO UPDATE SET revision=excluded.revision,digest=excluded.digest", user.ID.String(), category, host[:], value.Revision, digest)
	}
	return value, err
}
