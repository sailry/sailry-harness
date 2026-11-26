package live

import (
	"context"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
)

type attachmentViewerImage struct {
	key      string
	revision string
	ctx      context.Context
	cancel   context.CancelFunc
	visible  bool
	bitmap   *ui.Bitmap
}

func (view *attachmentViewerImage) release() {
	view.cancel()
	releaseAttachmentBitmap(view.bitmap)
	view.bitmap = nil
}

func (r *Runtime) resolveConversationViewerImage(session string, image conversation.Image) ui.ImageSource {
	if image.Source == nil && image.OriginalRevision == "" {
		image = r.resolveConversationImage(session, image)
	}
	if image.OriginalRevision != "" {
		return r.attachmentViewerSource(session, image.ID, image.OriginalRevision, image.Source)
	}
	return image.Source
}

// Only the selected viewer item owns a decoded original. The thumbnail stays
// visible while it loads or when the original can no longer be fetched.
func (r *Runtime) attachmentViewerSource(session, id, revision string, thumbnail ui.ImageSource) ui.ImageSource {
	state := r.attachments
	if state == nil {
		return thumbnail
	}
	cache := r.attachmentImages(session)
	key := attachmentKey(session, id)
	if ready := cache.images[key]; ready != nil && ready.originalRevision == revision {
		if cache.visible == nil {
			cache.visible = map[string]bool{}
		}
		cache.visible[key] = true
		thumbnail = ready.source()
	} else if w, h := ui.ImageSize(thumbnail); w == 0 || h == 0 {
		thumbnail = nil
	}
	if current := cache.viewer; current != nil {
		if current.key == key && current.revision == revision && (current.bitmap != nil || current.ctx.Err() == nil) {
			current.visible = true
			if current.bitmap != nil {
				return current.bitmap
			}
			return thumbnail
		}
		current.release()
		cache.viewer = nil
	}
	descriptor, ok := state.files[key]
	if !ok || descriptor.remoteURL == "" && !state.imageConnected {
		return thumbnail
	}
	if descriptor.attachment != nil && descriptor.attachment.Spec.Revision != revision {
		return thumbnail
	}
	descriptor.revision = &revision
	ctx, cancel := context.WithCancel(cache.ctx)
	view := &attachmentViewerImage{key: key, revision: revision, ctx: ctx, cancel: cancel, visible: true}
	cache.viewer = view
	r.async(func() {
		if !state.takeImageSlot(ctx) {
			return
		}
		release := state.finishImageSlotOnClose()
		data, _, err := r.readAttachmentImage(ctx, descriptor)
		if err != nil || ctx.Err() != nil {
			release()
			return
		}
		var bitmap *ui.Bitmap
		if descriptor.remoteURL != "" {
			bitmap, err = decodeRemoteBitmap(data)
		} else {
			bitmap, err = decodeAttachmentBitmap(data)
		}
		if err != nil || ctx.Err() != nil {
			release()
			return
		}
		r.enqueue(func(*ui.Context) {
			defer release()
			if ctx.Err() == nil && cache.viewer == view {
				view.bitmap = bitmap
			}
		})
	})
	return thumbnail
}
