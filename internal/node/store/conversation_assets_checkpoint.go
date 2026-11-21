package store

import (
	"context"
	"crypto/sha256"
	"database/sql"
	"errors"
	"maps"
	"sync"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// ConversationAssetReads belongs to one Node. A checkpoint certifies the
// dependencies of a range already delivered to the caller. It holds no bodies,
// target collections or database leases. Eviction falls back to ordinary paging.
type ConversationAssetReads struct {
	mu     sync.Mutex
	recent [32]*assetCheckpoint
	next   int
}

type assetCheckpoint struct {
	token                string
	session              protocol.SessionID
	kind                 protocol.ConversationAssetsKind
	revision, head, next uint64
	headID               string
	deps                 assetDependencies
}

func (reads *ConversationAssetReads) lookup(token string) *assetCheckpoint {
	if reads != nil && token != "" {
		reads.mu.Lock()
		defer reads.mu.Unlock()
		for _, entry := range reads.recent {
			if entry != nil && entry.token == token {
				return entry
			}
		}
	}
	return nil
}

func (reads *ConversationAssetReads) remember(entry assetCheckpoint) string {
	if !entry.deps.bounded() || len(entry.headID) > 128 {
		return ""
	}
	entry.token = protocol.NewRequestID().String()
	reads.mu.Lock()
	reads.recent[reads.next] = &entry
	reads.next = (reads.next + 1) % len(reads.recent)
	reads.mu.Unlock()
	return entry.token
}

type assetLocation struct {
	worktree protocol.WorktreeID
	root     *string
}

func readAssetLocation(ctx context.Context, db *sql.Conn, session protocol.SessionID, turn protocol.TurnID) (assetLocation, error) {
	run, err := visibleAgentRun(ctx, db, session, turn)
	if err != nil {
		return assetLocation{}, err
	}
	owner := assetLocation{worktree: run.Worktree}
	var path string
	err = db.QueryRowContext(ctx, "SELECT path FROM worktrees WHERE id=?1", run.Worktree.String()).Scan(&path)
	if err == nil {
		owner.root = &path
	} else if !errors.Is(err, sql.ErrNoRows) {
		return owner, err
	}
	return owner, nil
}

func (loc assetLocation) fingerprint() [32]byte {
	input := loc.worktree.String()
	if loc.root != nil {
		input += "\x00" + *loc.root
	}
	return sha256.Sum256([]byte(input))
}

type assetAttachmentKey struct {
	turn protocol.TurnID
	id   protocol.AttachmentID
}

type assetDependencies struct {
	turns       map[protocol.TurnID][32]byte
	attachments map[assetAttachmentKey][32]byte
	blobs       map[string]bool
}

const assetDependencyLimit = 128

func (d *assetDependencies) bounded() bool {
	return d != nil && len(d.turns)+len(d.attachments)+len(d.blobs) <= assetDependencyLimit
}

func newAssetDependencies() assetDependencies {
	return assetDependencies{turns: make(map[protocol.TurnID][32]byte), attachments: make(map[assetAttachmentKey][32]byte), blobs: make(map[string]bool)}
}

func (d *assetDependencies) merge(prior assetDependencies) {
	if !d.bounded() {
		return
	}
	maps.Copy(d.turns, prior.turns)
	maps.Copy(d.attachments, prior.attachments)
	maps.Copy(d.blobs, prior.blobs)
}

// Canonical sequences and record IDs are immutable. Rewind advances the
// membership revision. A restored database may reuse a sequence with a new ID.
func (c *assetCheckpoint) valid(ctx context.Context, db *sql.Conn) (bool, error) {
	if c.head != 0 {
		var member bool
		err := db.QueryRowContext(ctx, `SELECT EXISTS(SELECT 1 FROM conversation_events h JOIN agent_events e ON e.sequence=h.sequence WHERE h.session=?1 AND h.sequence=?2 AND e.id=?3)`, c.session.String(), c.head, c.headID).Scan(&member)
		if err != nil || !member {
			return false, err
		}
	}
	for turn, expected := range c.deps.turns {
		owner, err := readAssetLocation(ctx, db, c.session, turn)
		if err != nil || owner.fingerprint() != expected {
			return false, assetDependencyError(err)
		}
	}
	for key, expected := range c.deps.attachments {
		attachment, err := GetTurnAttachment(ctx, db, key.turn, key.id)
		if err != nil {
			return false, assetDependencyError(err)
		}
		body, err := encode(attachment)
		if err != nil || sha256.Sum256(body) != expected {
			return false, err
		}
	}
	for digest := range c.deps.blobs {
		var present bool
		err := db.QueryRowContext(ctx, "SELECT EXISTS(SELECT 1 FROM agent_blobs WHERE digest=?1)", digest).Scan(&present)
		if err != nil || !present {
			return false, err
		}
	}
	return true, nil
}

func assetDependencyError(err error) error {
	var fault *protocol.Fault
	if errors.As(err, &fault) && (fault.Code == protocol.ErrorCodeNotFound || fault.Code == protocol.ErrorCodeWrongTarget) {
		return nil
	}
	return err
}
