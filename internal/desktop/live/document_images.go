package live

import (
	"bytes"
	"context"
	"encoding/base64"
	"encoding/hex"
	"mime"
	"net/http"
	"net/url"
	"path"
	"strings"
	"sync"

	"github.com/egoist/mygo/ui"
	"github.com/egoist/mygo/ui/markdown"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/fileview"
	"github.com/sailry/sailry-harness/internal/protocol"
	"lukechampine.com/blake3"
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
	return &documentImages{
		ctx: ctx, cancel: cancel, owner: owner, controller: owner.controller, version: version,
		origin: documentImageOrigin{entry: owner.localRuntime().controller.Target(), node: owner.controller.Target(), worktree: descriptor.worktree, path: descriptor.path, version: version},
		images: make(map[documentImageKey]*documentImage), slots: make(chan struct{}, 2),
	}
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
	image := d.image(reference)
	return image.source, image.err
}

func (d *documentImages) image(reference string) *documentImage {
	key := documentImageKey{origin: d.origin, path: reference}
	if !strings.HasPrefix(reference, "data:") {
		resolved, ok := fileview.ResolveImagePath(d.origin.path, reference)
		if !ok || d.origin.worktree == (protocol.WorktreeID{}) {
			return &documentImage{err: fileActionFault(protocol.ErrorCodeInvalidRequest, "document image has no worktree-relative source")}
		}
		key.path = resolved
	}
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
	if strings.HasPrefix(reference, "data:") {
		image.data, image.mime, image.err = documentImageData(reference)
		if image.err == nil {
			sum := blake3.Sum256(image.data)
			image.revision = hex.EncodeToString(sum[:])
		}
	} else {
		image.data, image.revision, image.err = d.read(key.path)
		image.mime = mime.TypeByExtension(strings.ToLower(path.Ext(key.path)))
	}
	if image.err == nil {
		if image.mime == "" {
			image.mime = http.DetectContentType(image.data)
		}
		if strings.EqualFold(strings.Split(image.mime, ";")[0], "image/svg+xml") || strings.EqualFold(path.Ext(key.path), ".svg") {
			image.mime = "image/svg+xml"
			image.source, image.err = ui.ParseSVG(image.data)
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
