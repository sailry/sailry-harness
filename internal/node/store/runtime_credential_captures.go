package store

import (
	"context"
	"database/sql"
	"errors"
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

type runtimeCapture struct {
	waiting bool
	// A peer predating capture reports keeps its existing access material
	// until expiry, removal or revocation. Never send it a fresh access token.
	legacy *protocol.ConfigurationFollowRecord
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
