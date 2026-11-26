package files

import (
	"bytes"
	"context"
	"image"
	"image/draw"
	"image/jpeg"
	"image/png"
	"io"
	"slices"
	"time"

	"github.com/sailry/sailry-harness/internal/protocol"
	xdraw "golang.org/x/image/draw"
	_ "golang.org/x/image/tiff"
)

const (
	thumbnailSourcePixels = 32 << 20
	thumbnailSourceEdge   = 16384
	thumbnailCacheBytes   = 16 << 20
	thumbnailCacheEntries = 64
	thumbnailCacheTTL     = 30 * time.Second
)

type thumbnailCacheKey struct {
	revision string
	edge     uint32
}

type thumbnailCacheEntry struct {
	data    []byte
	facts   protocol.ThumbnailTransfer
	expires time.Time
	stop    func() bool
}

type thumbnailTransfers struct {
	preparing chan struct{}
	streams   chan struct{}
	cache     map[thumbnailCacheKey]*thumbnailCacheEntry // protected by Transfers.mu
	bytes     int
}

func newThumbnailTransfers() thumbnailTransfers {
	return thumbnailTransfers{
		preparing: make(chan struct{}, 2), streams: make(chan struct{}, 32),
		cache: make(map[thumbnailCacheKey]*thumbnailCacheEntry),
	}
}

// BeginThumbnail bounds preparation before even opening a source or reading
// its canonical bytes. The caller holds the slot through its scope recheck.
func (t *Transfers) BeginThumbnail(ctx context.Context, edge uint32) (context.Context, func(), error) {
	if edge == 0 || edge > protocol.MaxThumbnailEdge {
		return nil, nil, invalid("thumbnail edge exceeds the supported range")
	}
	if err := ctx.Err(); err != nil {
		return nil, nil, err
	}
	ctx, cancel := t.withTimeout(ctx, prepareTimeout)
	select {
	case <-ctx.Done():
		cancel()
		return nil, nil, ctx.Err()
	case t.thumbnails.preparing <- struct{}{}:
		if err := ctx.Err(); err != nil {
			cancel()
			<-t.thumbnails.preparing
			return nil, nil, err
		}
		return ctx, func() { cancel(); <-t.thumbnails.preparing }, nil
	}
}

func (t *Transfers) cachedThumbnail(key thumbnailCacheKey) *thumbnailCacheEntry {
	t.mu.Lock()
	defer t.mu.Unlock()
	if entry := t.thumbnails.cache[key]; entry != nil && entry.expires.After(t.now()) {
		return entry
	}
	return nil
}

// Callers verify the current source under their normal access checks. A
// digest identifies derived bytes; it is not permission to read them.
func (t *Transfers) thumbnailFor(ctx context.Context, data []byte, digest string, edge uint32) (*thumbnailCacheEntry, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	key := thumbnailCacheKey{digest, edge}
	if entry := t.cachedThumbnail(key); entry != nil {
		return entry, nil
	}
	encoded, err := makeThumbnail(ctx, data, int(edge))
	if err != nil {
		return nil, err
	}
	entry := &thumbnailCacheEntry{data: encoded, facts: protocol.ThumbnailTransfer{Size: uint64(len(encoded)), Revision: revision(encoded)}}
	t.mu.Lock()
	defer t.mu.Unlock()
	if t.closed {
		return nil, transferUnavailable()
	}
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	if previous := t.thumbnails.cache[key]; previous != nil && previous.expires.After(t.now()) {
		return previous, nil
	}
	t.dropThumbnailLocked(key)
	cache := &t.thumbnails
	for len(cache.cache) > 0 && (cache.bytes+cap(encoded) > thumbnailCacheBytes || len(cache.cache) >= thumbnailCacheEntries) {
		var oldest thumbnailCacheKey
		var expires time.Time
		for k, value := range cache.cache {
			if expires.IsZero() || value.expires.Before(expires) {
				oldest, expires = k, value.expires
			}
		}
		t.dropThumbnailLocked(oldest)
	}
	entry.expires = t.now().Add(thumbnailCacheTTL)
	cache.cache[key] = entry
	cache.bytes += cap(encoded)
	entry.stop = t.afterFunc(thumbnailCacheTTL, func() {
		t.mu.Lock()
		defer t.mu.Unlock()
		if cache.cache[key] == entry {
			t.dropThumbnailLocked(key)
		}
	})
	return entry, nil
}

func (t *Transfers) dropThumbnailLocked(key thumbnailCacheKey) {
	if entry := t.thumbnails.cache[key]; entry != nil {
		delete(t.thumbnails.cache, key)
		t.thumbnails.bytes -= cap(entry.data)
		entry.stop()
	}
}

func (t *Transfers) clearThumbnails() {
	t.mu.Lock()
	for key := range t.thumbnails.cache {
		t.dropThumbnailLocked(key)
	}
	t.mu.Unlock()
}

type thumbnailReader struct {
	ctx    context.Context
	reader io.Reader
}

func (r thumbnailReader) Read(p []byte) (int, error) {
	if err := r.ctx.Err(); err != nil {
		return 0, err
	}
	return r.reader.Read(p)
}

func makeThumbnail(ctx context.Context, data []byte, edge int) ([]byte, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	if edge < 1 || edge > protocol.MaxThumbnailEdge || len(data) > attachmentLimit {
		return nil, invalid("thumbnail exceeds the preparation limit")
	}
	source := bytes.NewReader(data)
	reader := thumbnailReader{ctx, source}
	config, format, err := image.DecodeConfig(reader)
	if err != nil {
		if ctx.Err() != nil {
			return nil, ctx.Err()
		}
		return nil, invalid("image could not be decoded")
	}
	if config.Width <= 0 || config.Height <= 0 || max(config.Width, config.Height) > thumbnailSourceEdge || config.Width > thumbnailSourcePixels/config.Height {
		return nil, invalid("image dimensions exceed the thumbnail decoding limit")
	}
	if max(config.Width, config.Height) <= edge && len(data) <= protocol.MaxThumbnailBytes {
		return slices.Clone(data), nil
	}
	source.Reset(data)
	decoded, _, err := image.Decode(reader)
	if ctx.Err() != nil {
		return nil, ctx.Err()
	}
	if err != nil {
		return nil, invalid("image could not be decoded")
	}
	bounds := decoded.Bounds()
	width, height := bounds.Dx(), bounds.Dy()
	if width < 1 || height < 1 || width > config.Width || height > config.Height {
		return nil, invalid("image dimensions do not match the source")
	}
	longest := max(width, height)
	side := min(longest, edge)
	w, h := max(1, width*side/longest), max(1, height*side/longest)
	resized := image.NewNRGBA(image.Rect(0, 0, w, h))
	xdraw.BiLinear.Scale(resized, resized.Bounds(), decoded, bounds, draw.Src, nil)
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	var encoded bytes.Buffer
	if format == "jpeg" {
		err = jpeg.Encode(&encoded, orientImage(resized, jpegOrientation(data)), &jpeg.Options{Quality: 85})
	} else {
		encoder := png.Encoder{CompressionLevel: png.BestSpeed}
		err = encoder.Encode(&encoded, resized)
	}
	if err != nil || encoded.Len() > protocol.MaxThumbnailBytes {
		return nil, invalid("image could not be encoded as a thumbnail")
	}
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	return encoded.Bytes(), nil
}
