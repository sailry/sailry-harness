package files

import (
	"context"
	"errors"
	"io"
	"os"

	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func (t *Transfers) DownloadFileThumbnail(ctx context.Context, caller protocol.NodeID, root string, worktree protocol.WorktreeID, path string, access core.FileAccess, edge uint32) (protocol.FileDownload, error) {
	out := protocol.FileDownload{Worktree: worktree, Path: path}
	fs, err := core.OpenFiles(root, access)
	if err != nil {
		return out, err
	}
	defer fs.Close()
	out.Stamp, err = fs.SourceRevision(path)
	if err != nil {
		return out, err
	}
	file, err := fs.Open(path)
	if err != nil {
		return out, err
	}
	defer file.Close()
	info, err := file.Stat()
	if err != nil {
		return out, err
	}
	if info.Size() < 0 || info.Size() > attachmentLimit {
		return out, invalid("image exceeds the thumbnail source limit")
	}
	out.Size = uint64(info.Size())
	out.Revision, err = hashOpenFile(ctx, file, out.Size)
	if err != nil {
		return out, err
	}
	thumbnail, err := t.thumbnailForOpenFile(ctx, file, out.Size, out.Revision, edge)
	if err == nil {
		err = fs.VerifySource(ctx, path, out.Revision, out.Stamp)
	}
	if err != nil {
		return out, err
	}
	out.Stream, out.Thumbnail, err = t.stageThumbnail(ctx, caller, protocol.UserID{}, root, worktree, thumbnail)
	return out, err
}

func (t *Transfers) DownloadAttachmentThumbnail(ctx context.Context, caller protocol.NodeID, user protocol.UserID, root string, attachment protocol.Attachment, edge uint32) (protocol.AttachmentDownload, error) {
	out := protocol.AttachmentDownload{Attachment: attachment}
	var draftUser protocol.UserID
	if attachment.Spec.Worktree == (protocol.WorktreeID{}) {
		if user == (protocol.UserID{}) {
			return out, invalid("draft attachment requires an owner")
		}
		draftUser = user
	}
	if attachment.Spec.Size > attachmentLimit {
		return out, invalid("image exceeds the thumbnail source limit")
	}
	file, err := t.OpenAttachment(attachment)
	if err != nil {
		return out, err
	}
	defer file.Close()
	digest, err := hashOpenFile(ctx, file, attachment.Spec.Size)
	if err != nil {
		return out, err
	}
	if digest != attachment.Spec.Revision {
		return out, conflict("attachment content changed")
	}
	thumbnail, err := t.thumbnailForOpenFile(ctx, file, attachment.Spec.Size, digest, edge)
	if err != nil {
		return out, err
	}
	out.Stream, out.Thumbnail, err = t.stageThumbnail(ctx, caller, draftUser, root, attachment.Spec.Worktree, thumbnail)
	return out, err
}

// The caller has already hashed this open source and rewound it. A cache hit
// needs no full-size buffer. Recheck the exact read on a miss before caching it.
func (t *Transfers) thumbnailForOpenFile(ctx context.Context, file *os.File, size uint64, digest string, edge uint32) (*thumbnailCacheEntry, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	if entry := t.cachedThumbnail(thumbnailCacheKey{digest, edge}); entry != nil {
		return entry, nil
	}
	reader := thumbnailReader{ctx, file}
	data := make([]byte, int(size))
	if _, err := io.ReadFull(reader, data); err != nil {
		if errors.Is(err, io.ErrUnexpectedEOF) || errors.Is(err, io.EOF) {
			return nil, conflict("image source changed")
		}
		return nil, err
	}
	var extra [1]byte
	if _, err := io.ReadFull(reader, extra[:]); !errors.Is(err, io.EOF) {
		if err != nil {
			return nil, err
		}
		return nil, conflict("image source changed")
	}
	if revision(data) != digest {
		return nil, conflict("image source changed")
	}
	return t.thumbnailFor(ctx, data, digest, edge)
}

func (t *Transfers) stageAttachmentThumbnail(ctx context.Context, caller protocol.NodeID, draftUser protocol.UserID, root string, attachment protocol.Attachment, data []byte, edge uint32) (protocol.AttachmentDownload, error) {
	out := protocol.AttachmentDownload{Attachment: attachment}
	thumbnail, err := t.thumbnailFor(ctx, data, attachment.Spec.Revision, edge)
	if err != nil {
		return out, err
	}
	out.Stream, out.Thumbnail, err = t.stageThumbnail(ctx, caller, draftUser, root, attachment.Spec.Worktree, thumbnail)
	return out, err
}

func (t *Transfers) stageThumbnail(ctx context.Context, caller protocol.NodeID, draftUser protocol.UserID, root string, worktree protocol.WorktreeID, thumbnail *thumbnailCacheEntry) (protocol.StreamID, *protocol.ThumbnailTransfer, error) {
	select {
	case t.thumbnails.streams <- struct{}{}:
	default:
		return protocol.StreamID{}, nil, &protocol.Fault{Code: protocol.ErrorCodeBusy, Message: "too many image thumbnail transfers"}
	}
	facts := thumbnail.facts
	entry := &transferEntry{download: true, worktree: worktree, draftUser: draftUser, size: facts.Size, revision: facts.Revision, thumbnailReservation: t.thumbnails.streams}
	stream, err := t.stageDownloadBytes(ctx, caller, root, entry, thumbnail.data)
	if err != nil {
		return protocol.StreamID{}, nil, err
	}
	return stream, &facts, nil
}
