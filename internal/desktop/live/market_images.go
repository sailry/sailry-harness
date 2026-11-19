package live

import (
	"bytes"
	"context"
	"errors"
	"image"
	"io"
	"net/http"
	"net/url"
	"time"

	"github.com/egoist/mygo/netproxy"
	"github.com/egoist/mygo/ui"
)

// Market card pictures are the https addresses a skill directory names for a
// publisher or a skill. Each is fetched once by the Desktop, at most
// marketImageBytes, decoded down to marketImageEdge and kept only while a
// card of the shown catalog names it. A failed fetch keeps the card's tile.
const (
	marketImageBytes    = 512 << 10
	marketImageEdge     = 96
	marketImageFetches  = 6
	marketImageDeadline = 20 * time.Second
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
		client: &http.Client{Transport: netproxy.Transport(), Timeout: marketImageDeadline,
			CheckRedirect: func(request *http.Request, via []*http.Request) error {
				if request.URL.Scheme != "https" || len(via) >= 5 {
					return errors.New("market picture redirect is refused")
				}
				return nil
			}}}
}

// marketImage is the picture at address once fetched, else nil while it is
// fetched or when it could not be. Call it on the UI thread.
func (r *Runtime) marketImage(address string) *ui.Bitmap {
	root := r.localRuntime()
	images := root.packages.images
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
	root.async(func() {
		bitmap, _ := images.fetch(root.ctx, address)
		root.enqueue(func(*ui.Context) {
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
	request.Header.Set("User-Agent", "Sailry/0.1")
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
	if err != nil || header.Width <= 0 || header.Height <= 0 || header.Width*header.Height > marketImagePixels {
		return nil, errors.New("market picture is unavailable")
	}
	return ui.DecodeBitmapScaled(data, marketImageEdge)
}

// pruneMarketImages releases the pictures no card in addresses names.
func (r *Runtime) pruneMarketImages(addresses map[string]bool) {
	images := r.localRuntime().packages.images
	for address := range images.entries {
		if !addresses[address] {
			delete(images.entries, address)
		}
	}
}
