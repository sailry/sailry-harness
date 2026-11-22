package desktop

import (
	"bytes"
	"context"
	"errors"
	"image"
	"io"
	"net/http"
	"net/url"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/web"
)

// Market card pictures are the public HTTPS addresses a skill directory names for a
// publisher or a skill. Each is fetched once by the Desktop, at most
// marketImageBytes, decoded down to marketImageEdge and kept only while a
// card of the shown catalog names it. A failed fetch keeps the card's tile.
const (
	marketImageBytes     = 512 << 10
	marketImageEdge      = 96
	marketImageFetches   = 6
	marketImageDeadline  = 20 * time.Second
	marketImageRedirects = 4
	// marketImagePixels bounds the size a picture's header may declare: the
	// decoder allocates it before scaling down.
	marketImagePixels = 4 << 20
)

type marketImage struct {
	bitmap *ui.Bitmap
	done   bool
}

type marketImages struct {
	entries map[string]*marketImage
	slots   chan struct{}
	client  *http.Client
}

func newMarketImages() *marketImages {
	return &marketImages{entries: map[string]*marketImage{}, slots: make(chan struct{}, marketImageFetches),
		client: web.NewPublicHTTPSClient(marketImageDeadline, marketImageRedirects)}
}

// image is the picture at address once fetched, else nil while it is
// fetched or when it could not be. Call it on the UI thread.
func (c *catalogController) image(address string) *ui.Bitmap {
	if !c.visible() {
		return nil
	}
	if c.images == nil {
		c.images = newMarketImages()
		context.AfterFunc(c.service.Context, c.images.client.CloseIdleConnections)
	}
	images := c.images
	if entry, exists := images.entries[address]; exists {
		return entry.bitmap
	}
	parsed, err := url.Parse(address)
	if err != nil || parsed.Scheme != "https" || parsed.Host == "" || parsed.User != nil {
		images.entries[address] = &marketImage{done: true}
		return nil
	}
	entry := &marketImage{}
	images.entries[address] = entry
	c.service.Run(func() {
		bitmap, _ := images.fetch(c.service.Context, address)
		c.service.Enqueue(func(*ui.Context) {
			// A pruned entry stays released; the card asks again when shown.
			if images.entries[address] == entry {
				entry.bitmap, entry.done = bitmap, true
			}
		})
	})
	return nil
}

func (images *marketImages) fetch(ctx context.Context, address string) (*ui.Bitmap, error) {
	select {
	case images.slots <- struct{}{}:
	case <-ctx.Done():
		return nil, ctx.Err()
	}
	defer func() { <-images.slots }()
	request, err := http.NewRequestWithContext(ctx, http.MethodGet, address, nil)
	if err != nil {
		return nil, err
	}
	request.Header.Set("User-Agent", "Sailry/"+link.ProductVersion)
	response, err := images.client.Do(request)
	if err != nil {
		return nil, err
	}
	defer response.Body.Close()
	if response.StatusCode < 200 || response.StatusCode >= 300 || response.ContentLength > marketImageBytes {
		return nil, errors.New("market picture is unavailable")
	}
	data, err := io.ReadAll(io.LimitReader(response.Body, marketImageBytes+1))
	if err != nil || len(data) > marketImageBytes {
		return nil, errors.New("market picture is unavailable")
	}
	header, _, err := image.DecodeConfig(bytes.NewReader(data))
	if err != nil || header.Width <= 0 || header.Height <= 0 || header.Width > marketImagePixels/header.Height {
		return nil, errors.New("market picture is unavailable")
	}
	return ui.DecodeBitmapScaled(data, marketImageEdge)
}

// pruneImages releases the pictures no card in addresses names.
func (c *catalogController) pruneImages(addresses map[string]bool) {
	images := c.images
	if images == nil {
		return
	}
	for address := range images.entries {
		if !addresses[address] {
			delete(images.entries, address)
		}
	}
}
