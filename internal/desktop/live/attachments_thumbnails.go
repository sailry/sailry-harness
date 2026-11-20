package live

import (
	"context"
	"image"
	"sync"

	"github.com/egoist/mygo/ui"
)

const (
	attachmentThumbnailEdge   = 480 // The largest chat tile is 240 DIPs.
	documentThumbnailEdge     = 768
	attachmentThumbnailBudget = 8 << 20 // Pixels across the window's bindings.
)

type attachmentThumbnail struct {
	bitmap           *ui.Bitmap
	originalRevision string
	pixels           int
}

// MyGo can retain the base bitmap and successive half-size levels together.
func thumbnailPixels(w, h int) int {
	pixels := w * h
	for w > 1 || h > 1 {
		w, h = (w+1)/2, (h+1)/2
		pixels += w * h
	}
	return pixels
}

func (r *Runtime) retainedThumbnailPixels() int {
	total := r.attachments.thumbnailPixels
	root := r.localRuntime()
	if root != r && root.attachments != nil {
		total += root.attachments.thumbnailPixels
	}
	for _, bound := range root.bindings {
		if bound != r && bound.attachments != nil {
			total += bound.attachments.thumbnailPixels
		}
	}
	return total
}

type attachmentThumbnailResult func(*attachmentThumbnail, error)

func (r *Runtime) loadAttachmentThumbnail(ctx context.Context, edge int, load func(attachmentThumbnailResult), accept func(*attachmentThumbnail, error) bool) bool {
	state := r.attachments
	reserved := thumbnailPixels(edge, edge)
	if ctx.Err() != nil {
		return false
	}
	if r.retainedThumbnailPixels()+reserved > attachmentThumbnailBudget {
		if root := r.localRuntime(); root.attachments != nil {
			root.attachments.thumbnailBudgetBlocked = true
		}
		return false
	}
	select {
	case state.imageSlots <- struct{}{}:
	default:
		return false
	}
	state.thumbnailPixels += reserved
	release := state.finishImageSlotOnClose()
	finish := func(thumbnail *attachmentThumbnail, err error) {
		r.enqueue(func(*ui.Context) {
			defer release()
			state.thumbnailPixels -= reserved
			if accept(thumbnail, err) && thumbnail != nil {
				thumbnail.pixels = thumbnailPixels(thumbnail.bitmap.Size())
				state.thumbnailPixels += thumbnail.pixels
			}
		})
	}
	if r.async(func() {
		if err := ctx.Err(); err != nil {
			finish(nil, err)
			return
		}
		load(finish)
	}) {
		return true
	}
	state.thumbnailPixels -= reserved
	release()
	return false
}

func (state *attachmentState) finishImageSlotOnClose() func() {
	// Until the frame consumes its result this slot limits the completion
	// queue as well as the decoder. Shutdown may discard the callback.
	release := sync.OnceFunc(func() { <-state.imageSlots })
	stop := context.AfterFunc(state.ctx, release)
	return func() {
		stop()
		release()
	}
}

func (state *attachmentState) releaseThumbnail(thumbnail *attachmentThumbnail) {
	if thumbnail == nil {
		return
	}
	state.thumbnailPixels -= thumbnail.pixels
	thumbnail.pixels = 0
	releaseAttachmentBitmap(thumbnail.bitmap)
}

func releaseAttachmentBitmap(bitmap *ui.Bitmap) {
	if bitmap != nil {
		// The previous frame and an open gallery can still reference the
		// bitmap. Drop its base and mip pixels there as well as in the cache.
		bitmap.Update(image.NewRGBA(image.Rectangle{}))
	}
}
