package live

import (
	"bytes"
	"context"
	"encoding/base64"
	"encoding/hex"
	"io"
	"mime"
	"net/http"
	"net/url"
	"path"
	"strings"
	"sync"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/egoist/mygo/ui/markdown"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/fileview"
	"github.com/sailry/sailry-harness/internal/node/web"
	"github.com/sailry/sailry-harness/internal/protocol"
	"lukechampine.com/blake3"
)

const (
	documentImageFetchTimeout   = 20 * time.Second
	documentImageFetchRedirects = 4
	documentImageFetchBytes     = 4 << 20
	documentImageFetchEdge      = 8192
	documentImageFetchPixels    = 8 << 20
)

type documentImageOrigin struct {
	entry, node protocol.NodeID
	worktree    protocol.WorktreeID
	path        string
	version     string
}

type documentImageKey struct {
	origin documentImageOrigin
	path   string
}

type documentImage struct {
	ready    chan struct{}
	data     []byte
	mime     string
	revision string
	source   ui.ImageSource
	err      error
}

// documentImages belongs to one open document, including its immutable route.
// Loaded images remain available offline; closing the document cancels its IO.
type documentImages struct {
	ctx        context.Context
	cancel     context.CancelFunc
	owner      *Runtime
	controller *client.Client
	https      *http.Client
	origin     documentImageOrigin
	version    string
	markdown   *markdown.Options
	connected  bool
	mu         sync.Mutex
	images     map[documentImageKey]*documentImage
	bytes      int
	slots      chan struct{}
}

func newDocumentImages(ctx context.Context, owner *Runtime, descriptor attachmentDescriptor, version string) *documentImages {
	ctx, cancel := context.WithCancel(ctx)
	https := newDocumentImageHTTPClient()
	context.AfterFunc(ctx, https.CloseIdleConnections)
	return &documentImages{
		ctx: ctx, cancel: cancel, owner: owner, controller: owner.controller, https: https, version: version,
		origin: documentImageOrigin{entry: owner.localRuntime().controller.Target(), node: owner.controller.Target(), worktree: descriptor.worktree, path: descriptor.path, version: version},
		images: make(map[documentImageKey]*documentImage), slots: make(chan struct{}, 2),
	}
}

func newPublicImageHTTPClient(timeout time.Duration, maxRedirects int) *http.Client {
	client := web.NewPublicHTTPSClient(timeout, maxRedirects)
	checkRedirect := client.CheckRedirect
	client.CheckRedirect = func(request *http.Request, via []*http.Request) error {
		request.Header.Del("Referer")
		return checkRedirect(request, via)
	}
	return client
}

func newDocumentImageHTTPClient() *http.Client {
	return newPublicImageHTTPClient(documentImageFetchTimeout, documentImageFetchRedirects)
}

func remoteImageURL(raw string) (string, bool) {
	address, err := url.Parse(strings.TrimSpace(raw))
	if err != nil || !strings.EqualFold(address.Scheme, "https") || address.Hostname() == "" || address.User != nil {
		return "", false
	}
	address.Scheme = "https"
	address.Fragment, address.RawFragment = "", ""
	return address.String(), true
}

// options is called on the UI owner. A reconnect retries failed placeholders
// while the document's successful, versioned images keep their original bytes.
func (d *documentImages) options() *markdown.Options {
	connected := d.controller.Connected()
	if d.markdown == nil || connected && !d.connected {
		d.mu.Lock()
		for key, image := range d.images {
			select {
			case <-image.ready:
				if image.err != nil {
					delete(d.images, key)
				}
			default:
			}
		}
		d.mu.Unlock()
		d.markdown = &markdown.Options{LoadImage: d.load}
	}
	d.connected = connected
	return d.markdown
}

func (d *documentImages) load(reference string) (ui.ImageSource, error) {
	if address, ok := remoteImageURL(reference); ok {
		image := d.image(address, true)
		return image.source, image.err
	}
	image := d.localImage(reference)
	return image.source, image.err
}

func (d *documentImages) localImage(reference string) *documentImage {
	if !strings.HasPrefix(reference, "data:") {
		resolved, ok := fileview.ResolveImagePath(d.origin.path, reference)
		if !ok || d.origin.worktree == (protocol.WorktreeID{}) {
			return &documentImage{err: fileActionFault(protocol.ErrorCodeInvalidRequest, "document image has no worktree-relative source")}
		}
		reference = resolved
	}
	return d.image(reference, false)
}

func (d *documentImages) image(location string, remote bool) *documentImage {
	key := documentImageKey{origin: d.origin, path: location}
	d.mu.Lock()
	image := d.images[key]
	if image != nil {
		d.mu.Unlock()
		select {
		case <-image.ready:
			if d.ctx.Err() != nil {
				return &documentImage{err: d.ctx.Err()}
			}
			return image
		case <-d.ctx.Done():
			return &documentImage{err: d.ctx.Err()}
		}
	}
	if len(d.images) >= 48 {
		d.mu.Unlock()
		return &documentImage{err: fileActionFault(protocol.ErrorCodeInvalidRequest, "document image cache is full")}
	}
	image = &documentImage{ready: make(chan struct{})}
	d.images[key] = image
	d.mu.Unlock()
	defer close(image.ready)
	select {
	case d.slots <- struct{}{}:
		defer func() { <-d.slots }()
	case <-d.ctx.Done():
		image.err = d.ctx.Err()
		return image
	}
	switch {
	case remote:
		image.data, image.mime, image.err = d.readHTTPS(location)
	case strings.HasPrefix(location, "data:"):
		image.data, image.mime, image.err = documentImageData(location)
		if image.err == nil {
			sum := blake3.Sum256(image.data)
			image.revision = hex.EncodeToString(sum[:])
		}
	default:
		image.data, image.revision, image.err = d.read(location)
		image.mime = mime.TypeByExtension(strings.ToLower(path.Ext(location)))
	}
	if image.err == nil {
		if image.mime == "" {
			image.mime = http.DetectContentType(image.data)
		}
		if strings.EqualFold(strings.Split(image.mime, ";")[0], "image/svg+xml") || !remote && strings.EqualFold(path.Ext(location), ".svg") {
			image.mime = "image/svg+xml"
			svg, err := ui.ParseSVG(image.data)
			if remote && err == nil {
				w, h := svg.Size()
				if !(w >= 1 && h >= 1 && w <= documentImageFetchEdge && h <= documentImageFetchEdge && w*h <= documentImageFetchPixels) {
					err = fileActionFault(protocol.ErrorCodeInvalidRequest, "document image dimensions exceed the fetch limit")
				}
			}
			image.source, image.err = svg, err
		} else if remote {
			image.source, image.err = decodeRemoteDocumentBitmap(image.data)
		} else {
			image.source, image.err = decodeAttachmentBitmap(image.data)
		}
	}
	if d.ctx.Err() != nil {
		image.err = d.ctx.Err()
	}
	d.mu.Lock()
	defer d.mu.Unlock()
	cost := len(image.data)
	if bitmap, ok := image.source.(*ui.Bitmap); ok && bitmap != nil {
		width, height := bitmap.Size()
		cost += width * height * 4
	}
	if image.err == nil && d.bytes+cost > attachmentMaximum {
		image.err = fileActionFault(protocol.ErrorCodeInvalidRequest, "document images exceed the cache limit")
	}
	if image.err != nil {
		image.data, image.source = nil, nil
	} else {
		d.bytes += cost
	}
	return image
}

func (d *documentImages) readHTTPS(address string) ([]byte, string, error) {
	return fetchPublicHTTPSImage(d.ctx, d.https, address, documentImageFetchBytes)
}

func fetchPublicHTTPSImage(ctx context.Context, client *http.Client, address string, maxBytes int64) ([]byte, string, error) {
	request, err := http.NewRequestWithContext(ctx, http.MethodGet, address, nil)
	if err != nil {
		return nil, "", err
	}
	response, err := client.Do(request)
	if err != nil {
		return nil, "", err
	}
	defer response.Body.Close()
	if response.StatusCode < 200 || response.StatusCode >= 300 {
		code := protocol.ErrorCodeUnavailable
		switch response.StatusCode {
		case http.StatusNotFound, http.StatusGone:
			code = protocol.ErrorCodeNotFound
		case http.StatusUnauthorized, http.StatusForbidden:
			code = protocol.ErrorCodePermissionDenied
		case http.StatusBadRequest, http.StatusMethodNotAllowed, http.StatusNotAcceptable,
			http.StatusRequestURITooLong, http.StatusUnsupportedMediaType, http.StatusUnprocessableEntity,
			http.StatusUnavailableForLegalReasons:
			code = protocol.ErrorCodeInvalidRequest
		}
		return nil, "", fileActionFault(code, "remote image is unavailable")
	}
	if response.ContentLength > maxBytes {
		return nil, "", fileActionFault(protocol.ErrorCodeInvalidRequest, "image exceeds its fetch limit")
	}
	data, err := io.ReadAll(io.LimitReader(response.Body, maxBytes+1))
	if err != nil {
		return nil, "", err
	}
	if int64(len(data)) > maxBytes {
		return nil, "", fileActionFault(protocol.ErrorCodeInvalidRequest, "image exceeds its fetch limit")
	}
	media, _, _ := mime.ParseMediaType(response.Header.Get("Content-Type"))
	if !strings.HasPrefix(media, "image/") {
		target := request.URL
		if response.Request != nil {
			target = response.Request.URL
		}
		if strings.EqualFold(path.Ext(target.Path), ".svg") {
			media = "image/svg+xml"
		}
	}
	return data, media, nil
}

func decodeRemoteDocumentBitmap(data []byte) (*ui.Bitmap, error) {
	config, err := attachmentImageConfig(data)
	if err != nil {
		return nil, err
	}
	if config.Width > documentImageFetchEdge || config.Height > documentImageFetchEdge || config.Width > documentImageFetchPixels/config.Height {
		return nil, fileActionFault(protocol.ErrorCodeInvalidRequest, "document image dimensions exceed the fetch limit")
	}
	return ui.DecodeBitmap(data)
}

func (d *documentImages) read(relative string) ([]byte, string, error) {
	output, err := d.controller.Do(d.ctx, protocol.Command{Kind: "download_file", Data: &protocol.CommandDownloadFilePayload{Worktree: d.origin.worktree, Path: relative}})
	if err != nil {
		return nil, "", err
	}
	value, ok := detail[protocol.FileDownload](output.Data)
	if !ok || output.Kind != "file_download" {
		return nil, "", fileActionFault(protocol.ErrorCodeInvalidRequest, "document image download is invalid")
	}
	if value.Worktree != d.origin.worktree || value.Path != relative || value.Size > attachmentMaximum {
		d.owner.cancelAttachmentStream(value.Stream)
		return nil, "", fileActionFault(protocol.ErrorCodeInvalidRequest, "document image source changed or exceeds the size limit")
	}
	sink := &imageDownloadSink{limit: value.Size}
	if err := d.controller.Download(d.ctx, value.Stream, value.Size, value.Revision, sink, nil); err != nil {
		return nil, "", err
	}
	return sink.data, value.Revision, nil
}

func documentImageData(reference string) ([]byte, string, error) {
	metadata, encoded, ok := strings.Cut(strings.TrimPrefix(reference, "data:"), ",")
	if !ok || len(encoded) > attachmentMaximum*4/3+4 {
		return nil, "", fileActionFault(protocol.ErrorCodeInvalidRequest, "document image data is invalid")
	}
	metadata, base64Data := strings.CutSuffix(metadata, ";base64")
	media, _, err := mime.ParseMediaType(metadata)
	if err != nil || !strings.HasPrefix(media, "image/") {
		return nil, "", fileActionFault(protocol.ErrorCodeInvalidRequest, "document image data is not an image")
	}
	var data []byte
	if base64Data {
		data, err = base64.StdEncoding.DecodeString(encoded)
	} else {
		var decoded string
		decoded, err = url.PathUnescape(encoded)
		data = []byte(decoded)
	}
	if err != nil || len(data) > attachmentMaximum || len(bytes.TrimSpace(data)) == 0 {
		return nil, "", fileActionFault(protocol.ErrorCodeInvalidRequest, "document image data is invalid")
	}
	return data, media, nil
}
