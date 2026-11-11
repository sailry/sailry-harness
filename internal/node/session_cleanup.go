package node

import (
	"context"
	"database/sql"
	"log"
	"sync"

	"github.com/sailry/sailry-harness/internal/node/files"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// storedFiles orders the files the profile stores under a row: attachments
// and artifacts. A publication writes its file before its row commits and a
// collection deletes the files without a row, so a collection excludes every
// publication from its file to its row. Publications run together. The
// worktree locks do not take part: stored files belong to no worktree.
type storedFiles struct {
	mu sync.RWMutex
}

// publish joins the publications until the effect closes, which is after its
// row commits or fails. It is called before the file is written.
func (stored *storedFiles) publish(effect *workspaceCompletion) {
	stored.mu.RLock()
	release := effect.release
	effect.release = func() {
		stored.mu.RUnlock()
		if release != nil {
			release()
		}
	}
}

// collectAttachments removes the stored attachment files that no row names:
// uploads whose row never committed and attachments whose row was deleted. It
// waits for the finishing uploads, so no file is taken between its
// publication and its row. A failure leaves the files for the next collection.
func (node *Node) collectAttachments(ctx context.Context) {
	node.stored.mu.Lock()
	defer node.stored.mu.Unlock()
	node.collectAttachmentsLocked(ctx)
}

// removedSessionFiles deletes the files that a committed session removal left
// without rows: the artifact directories of removed sessions that keep no
// artifact, and the attachments that were bound only to their turns.
func (node *Node) removedSessionFiles(changes []protocol.Event) {
	removed := false
	for _, change := range changes {
		removed = removed || change.Kind == "sessions_removed"
	}
	if !removed {
		return
	}
	// The caller may be a publication that has not closed, which a collection
	// waits for, so the work runs on its own. The removing command is still
	// counted, and Stop waits for this too.
	node.work.Add(1)
	go func() {
		defer node.work.Done()
		ctx := context.WithoutCancel(node.ctx)
		node.stored.mu.Lock()
		defer node.stored.mu.Unlock()
		var owners []protocol.SessionID
		err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
			var err error
			owners, err = store.RemovedArtifactOwners(ctx, db)
			return err
		})
		if err == nil {
			err = files.RemoveArtifacts(node.profile.Ownership.Path, owners)
		}
		if err != nil {
			log.Printf("removed session artifact cleanup failed: %v", err)
		}
		node.collectAttachmentsLocked(ctx)
	}()
}

// collectAttachmentsLocked is collectAttachments for a caller that excludes
// the publications already.
func (node *Node) collectAttachmentsLocked(ctx context.Context) {
	var retained map[protocol.AttachmentID]bool
	err := node.profile.Store.Node.Read(ctx, func(db *sql.Conn) error {
		var err error
		retained, err = store.RetainedAttachments(ctx, db)
		return err
	})
	if err == nil {
		err = node.transfers.CollectAttachments(retained)
	}
	if err != nil {
		log.Printf("attachment cleanup failed: %v", err)
	}
}
