package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"math"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// PrepareSessionForward keeps exact replays of already admitted operations, but
// a new session must be created by the Node that owns its selected model.
func PrepareSessionForward(ctx context.Context, db *sql.Conn, node, caller protocol.NodeID, request protocol.Request) (protocol.Request, error) {
	input, ok := request.Command.Data.(*protocol.CommandCreateSessionAtPayload)
	if !ok || input.Config == nil {
		return protocol.Request{}, invalid("configuration forwarding expected")
	}
	if caller != node || input.Target == node {
		return protocol.Request{}, fault(protocol.ErrorCodePermissionDenied, "only the local controller can forward configuration to another Node")
	}
	var frozen []byte
	err := db.QueryRowContext(ctx, "SELECT body FROM session_forwards WHERE caller=?1 AND request=?2", caller[:], request.ID.String()).Scan(&frozen)
	if err == nil {
		var forwarded protocol.Request
		err = json.Unmarshal(frozen, &forwarded)
		return forwarded, err
	}
	if !errors.Is(err, sql.ErrNoRows) {
		return protocol.Request{}, err
	}
	return protocol.Request{}, fault(protocol.ErrorCodeNotConfigured, "select a model channel configured on the execution Node")
}

// importSession rejects first-time remote model captures. Exact accepted request
// replays are answered by the admission ledger before this handler runs.
func importSession(_ context.Context, _ *sql.Conn, node, caller protocol.NodeID, _ string, bundle protocol.SessionImport) (protocol.Output, []protocol.Event, error) {
	if bundle.Source != caller || caller == node {
		return protocol.Output{}, nil, fault(protocol.ErrorCodePermissionDenied, "configuration source does not match the paired caller")
	}
	return protocol.Output{}, nil, fault(protocol.ErrorCodeNotConfigured, "create the session on the Node that owns its model channel")
}

// removePrivateProviders deletes the providers forwarded for removed sessions
// once no remaining session uses them. Their secrets leave with them.
func removePrivateProviders(ctx context.Context, db *sql.Conn, candidates []protocol.ProviderID) error {
	for _, id := range candidates {
		stored, err := readProviderConfiguration(ctx, db, id)
		if err != nil {
			return err
		}
		if stored == nil || stored.Listed {
			continue
		}
		var used bool
		err = db.QueryRowContext(ctx, `SELECT EXISTS(SELECT 1 FROM sessions s JOIN session_revisions r ON r.session=s.id AND r.revision=s.revision
 WHERE s.state!='removed' AND (json_extract(r.config,'$.provider')=?1 OR json_extract(r.profile,'$.provider.id')=?1
 OR EXISTS(SELECT 1 FROM json_each(r.roles,'$.providers') p WHERE json_extract(p.value,'$.id')=?1)))`, id.String()).Scan(&used)
		if err != nil {
			return err
		}
		if used {
			continue
		}
		if _, err := db.ExecContext(ctx, "DELETE FROM providers WHERE id=?1", id.String()); err != nil {
			return err
		}
	}
	return nil
}

// ListAssociatedSessions pages the sessions a package associated with one
// business identity, newest first. A worktree scope confines the page.
func ListAssociatedSessions(ctx context.Context, db *sql.Conn, request protocol.Request) (protocol.ConversationAssociationAssociatedSessions, error) {
	page := protocol.DefaultConversationAssociationAssociatedSessions()
	input := request.Command.Data.(*protocol.CommandListAssociatedSessionsPayload)
	if err := validateSessionAssociation(input.Association); err != nil {
		return page, err
	}
	if input.Limit < 1 || input.Limit > 100 {
		return page, invalid("session page limit must be between 1 and 100")
	}
	var worktree *protocol.WorktreeID
	if request.Plugin != nil {
		worktree = request.Plugin.Worktree
	}
	var scope any
	if worktree != nil {
		scope = worktree.String()
	}
	sequence := uint64(math.MaxInt64)
	if input.Before != nil {
		if err := CheckSessionAccess(ctx, db, *input.Before); err != nil {
			return page, err
		}
		session, err := ReadSession(ctx, db, *input.Before)
		if err != nil {
			return page, err
		}
		if session.Association == nil || *session.Association != input.Association || worktree != nil && *worktree != session.Worktree {
			return page, fault(protocol.ErrorCodeWrongTarget, "session cursor belongs to another association")
		}
		sequence = session.CreationOrder
	}
	user, err := CurrentUser(ctx, db)
	if err != nil {
		return page, err
	}
	rows, err := db.QueryContext(ctx, `SELECT s.id FROM sessions s JOIN session_associations a ON a.session=s.id LEFT JOIN projects p ON p.id=s.project
 WHERE a.namespace=?1 AND a.kind=?2 AND a.identity=?3 AND s.state!='removed'
 AND s.rowid<?4 AND (?5 IS NULL OR s.worktree=?5) AND (CASE WHEN s.project IS NULL THEN coalesce(s.owner,(SELECT id FROM users WHERE local=1))=?7 ELSE p.visibility='shared' OR coalesce(p.owner,(SELECT id FROM users WHERE local=1))=?7 END) ORDER BY s.rowid DESC LIMIT ?6`,
		input.Association.Namespace, input.Association.Kind, input.Association.ID, int64(sequence), scope, int64(input.Limit)+1, user.ID.String())
	if err != nil {
		return page, err
	}
	ids := []protocol.SessionID{}
	for rows.Next() {
		var text string
		if err := rows.Scan(&text); err != nil {
			rows.Close()
			return page, err
		}
		id, err := protocol.ParseSessionID(text)
		if err != nil {
			rows.Close()
			return page, err
		}
		ids = append(ids, id)
	}
	if err := rows.Err(); err != nil {
		rows.Close()
		return page, err
	}
	if err := rows.Close(); err != nil {
		return page, err
	}
	if len(ids) > int(input.Limit) {
		ids = ids[:input.Limit]
		next := ids[len(ids)-1]
		page.NextBefore = &next
	}
	for _, id := range ids {
		session, err := ReadSession(ctx, db, id)
		if err != nil {
			return page, err
		}
		page.Sessions = append(page.Sessions, session)
	}
	return page, nil
}
