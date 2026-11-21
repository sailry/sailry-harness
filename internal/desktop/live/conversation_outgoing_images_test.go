package live

import (
	"image"
	"testing"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/composer"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// A sent picture's preview hands its layout and original to the user entry of
// the same session and revision. Pixels the session's thumbnail cache owns are
// resolved through it rather than held by the preview, and no preview pixels
// enter the cache.
func TestOutgoingPictureHandoff(t *testing.T) {
	node, session, other, turn, request := protocol.NodeID{1}, protocol.SessionID{2}, protocol.SessionID{7}, protocol.TurnID{3}, protocol.RequestID{4}
	worktree := protocol.WorktreeID{8}
	snapshot := protocol.DefaultSnapshot()
	snapshot.Node = node
	controller := client.New(&conversationTestTransport{snapshot: snapshot})
	defer controller.Close()
	cv := &viewmodel.Conversation{Node: node.String(), ID: session.String()}
	r := &Runtime{ctx: t.Context(), controller: controller, snapshotRead: true, snapshotCopy: snapshot,
		App: shell.NewLive(&viewmodel.Data{Workspace: &viewmodel.Workspace{}, Unassigned: []*viewmodel.Conversation{cv}})}
	r.initAttachments()
	defer r.closeAttachments()
	r.conversationSeen = map[protocol.SessionID]bool{session: true, other: true}

	uploaded := func(id byte, name, revision string) protocol.Attachment {
		return protocol.Attachment{ID: protocol.AttachmentID{id}, Spec: protocol.AttachmentSpec{Worktree: worktree, Name: name, MediaType: "image/png", Revision: revision}}
	}
	pastedUpload, cachedUpload := uploaded(5, "pasted.png", "pasted-original"), uploaded(6, "cached.png", "cached-original")
	pastedPixels := ui.NewBitmap(image.NewRGBA(image.Rect(0, 0, 400, 300)))
	pasted := &composer.Attachment{ID: pastedUpload.ID.String(), Name: "pasted.png", Mime: "image/png", Status: composer.Ready, Image: pastedPixels}
	cached := &composer.Attachment{ID: cachedUpload.ID.String(), Name: "cached.png", Mime: "image/png", Status: composer.Ready,
		Image: ui.NewBitmap(image.NewRGBA(image.Rect(0, 0, 120, 90))), ImageRevision: "cached-original"}
	// An existing session's uploads carry its worktree, as attachments_upload does.
	r.attachments.items[pasted] = &attachmentRecord{conversation: cv.ID, worktree: worktree, ready: &pastedUpload}
	r.attachments.items[cached] = &attachmentRecord{conversation: cv.ID, worktree: worktree, ready: &cachedUpload}

	actions := &conversationActions{r: r, cv: cv, comp: r.App.Composer(cv.ID)}
	pending := actions.stageOutgoing(composer.Message{Text: "Look", Attachments: []*composer.Attachment{pasted, cached}})
	if pending == nil || len(pending.input.Images) != 2 {
		t.Fatal("send did not stage its pictures")
	}
	local, shared := pending.input.Images[0], pending.input.Images[1]
	if local.Source != pastedPixels || local.Width != 400 || local.Height != 300 || local.OriginalRevision != "pasted-original" {
		t.Fatalf("pasted preview = %+v; want its own pixels, size and uploaded original", local)
	}
	if shared.Source != nil || shared.Width != 120 || shared.Height != 90 || shared.OriginalRevision != "cached-original" {
		t.Fatalf("cached preview = %+v; want the cache's key, size and original without its bitmap", shared)
	}

	t.Run("another session or revision gets nothing", func(t *testing.T) {
		r.registerConversationAttachment(other.String(), pastedUpload)
		r.adoptOutgoingImageFacts(session.String(), []conversation.Image{local})
		r.adoptOutgoingImageFacts(other.String(), []conversation.Image{{ID: local.ID, Width: 400, Height: 300, OriginalRevision: "changed"}})
		for _, id := range []string{session.String(), other.String()} {
			if _, known := r.attachmentImages(id).facts[attachmentKey(id, local.ID)]; known {
				t.Fatalf("session %s adopted a preview that is not its attachment of that revision", id)
			}
		}
		delete(r.attachments.files, attachmentKey(other.String(), local.ID))
	})

	// The Node's user entry lands with its run, which the transcript projects
	// rows from; its attachments are registered as the page binds its sources
	// before the outgoing projection.
	pending.request = request
	r.snapshotCopy.Turns = []protocol.TurnSummary{{ID: turn, Session: session, Request: request}}
	r.registerConversationAttachment(cv.ID, pastedUpload)
	r.registerConversationAttachment(cv.ID, cachedUpload)
	page := protocol.DefaultConversationPage()
	page.Session = session
	page.Runs = []protocol.ConversationRun{{Session: session, Turn: turn, Kind: protocol.ConversationRunKindTask, Status: protocol.ConversationStatusRunning}}
	page.Entries = []protocol.ConversationEntry{{Turn: turn, ID: "user", Author: "user", Parts: []protocol.ConversationPart{{Kind: "text", Data: "Look"}}}}
	cv.Turns, cv.Session, cv.Waiting, cv.Queue = Conversation(client.ConversationState{Connected: true, Snapshot: protocol.ConversationSnapshot{Page: page}})
	r.projectOutgoing(cv, page)
	if r.composerOutgoing[cv.ID] != nil {
		t.Fatal("the user entry did not take over from the preview")
	}
	cache := r.attachmentImages(cv.ID)
	got := r.resolveConversationImage(cv.ID, conversation.Image{ID: local.ID, Name: local.Name})
	if got.Width != 400 || got.Height != 300 || got.OriginalRevision != "pasted-original" || got.Missing {
		t.Fatalf("user entry = %+v; want the preview's size and original", got)
	}
	if len(cache.images) != 0 || r.attachments.thumbnailPixels != 0 {
		t.Fatal("the preview's pixels entered the thumbnail cache")
	}
	if facts := cache.facts[attachmentKey(cv.ID, shared.ID)]; facts != (attachmentImageFacts{120, 90, "cached-original"}) {
		t.Fatalf("cached picture facts = %+v; want the preview's", facts)
	}
	// Committing the sent files keeps the session's registered attachments:
	// an entry that landed before the response still resolves its picture.
	r.attachmentsCommitted([]*composer.Attachment{pasted, cached})
	if _, ok := r.attachmentDescriptor(cv.ID, local.ID); !ok {
		t.Fatal("committing the send dropped the user entry's attachment")
	}
	if got := r.resolveConversationImage(cv.ID, conversation.Image{ID: local.ID}); got.Width != 400 || got.OriginalRevision != "pasted-original" {
		t.Fatalf("user entry after commit = %+v", got)
	}
	r.pruneAttachmentImages()
	if w, h := ui.ImageSize(pastedPixels); w != 400 || h != 300 {
		t.Fatal("pruning the cache released the preview's own pixels")
	}
}
