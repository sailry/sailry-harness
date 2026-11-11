package node

import (
	"context"
	"database/sql"
	"log"

	"github.com/sailry/sailry-harness/internal/node/files"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// collectAttachments removes the stored attachment files that no row names:
// uploads whose row never committed and attachments whose row was deleted. A
// finishing upload holds the effects lock from its publication to its row, so
// no file is taken between the two. A failure leaves the files for the next
// collection.
func (node *Node) collectAttachments(ctx context.Context) {
	node.effects.Lock()
	defer node.effects.Unlock()
	node.collectAttachmentsHeld(ctx)
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
	// The command lock may be held here, so the work runs on its own. The
	// removing command is still counted, and Stop waits for this too.
	node.work.Add(1)
	go func() {
		defer node.work.Done()
		ctx := context.WithoutCancel(node.ctx)
		node.effects.Lock()
		defer node.effects.Unlock()
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
		node.collectAttachmentsHeld(ctx)
	}()
}

// collectAttachmentsHeld is collectAttachments for a caller that holds the
// effects lock.
func (node *Node) collectAttachmentsHeld(ctx context.Context) {
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
