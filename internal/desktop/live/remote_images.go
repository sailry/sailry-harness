package live

import (
	"bytes"
	"context"
	"encoding/hex"
	"image"
	"net/http"
	"net/url"
	"path"
	"strings"
	"time"
	"unicode"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/protocol"
	_ "golang.org/x/image/webp"
	"lukechampine.com/blake3"
)

const (
	remoteImageFetchTimeout   = 20 * time.Second
	remoteImageFetchRedirects = 4
	remoteImageFetchBytes     = 16 << 20
	remoteImageFetchEdge      = 8192
	remoteImageFetchPixels    = 32 << 20
)

func isHTTPImageURL(raw string) bool {
	address, err := url.Parse(strings.TrimSpace(raw))
	return err == nil && strings.EqualFold(address.Scheme, "http") && address.Hostname() != "" && address.User == nil
}

func (r *Runtime) registerRemoteImage(session, id, address, name string) {
	if r.attachments == nil {
		return
	}
	owner, _ := protocol.ParseSessionID(session)
	r.attachments.files[attachmentKey(session, id)] = attachmentDescriptor{
		session:   owner,
		remoteURL: address,
		name:      name,
	}
}

func (r *Runtime) remoteImageHTTPClient() *http.Client {
	state := r.attachments
	state.mu.Lock()
	defer state.mu.Unlock()
	if state.remoteHTTP == nil {
		state.remoteHTTP = newPublicImageHTTPClient(remoteImageFetchTimeout, remoteImageFetchRedirects)
	}
	return state.remoteHTTP
}

func (r *Runtime) fetchRemoteImage(ctx context.Context, address string) ([]byte, string, error) {
	normalized, ok := remoteImageURL(address)
	if !ok {
		return nil, "", fileActionFault(protocol.ErrorCodeInvalidRequest, "remote image address is invalid")
	}
	data, media, err := fetchPublicHTTPSImage(ctx, r.remoteImageHTTPClient(), normalized, remoteImageFetchBytes)
	if err != nil {
		return nil, "", err
	}
	if !strings.HasPrefix(media, "image/") {
		if detected := http.DetectContentType(data); strings.HasPrefix(detected, "image/") {
			media = detected
		}
	}
	return data, media, nil
}

// readRemoteImage reads a remote picture for copy, save or the full-size
// viewer. Only bytes that decode as a supported picture are returned, so an
// HTML page served with a success status is never copied or saved. When the
// descriptor carries the digest of the bytes the thumbnail showed, a changed
// response is a conflict instead of a different picture.
func (r *Runtime) readRemoteImage(ctx context.Context, descriptor attachmentDescriptor) ([]byte, string, error) {
	data, media, err := r.fetchRemoteImage(ctx, descriptor.remoteURL)
	if err != nil {
		return nil, "", err
	}
	if media, err = remoteImageContent(data, media); err != nil {
		return nil, "", err
	}
	if descriptor.revision != nil && remoteImageDigest(data) != *descriptor.revision {
		return nil, "", fileActionFault(protocol.ErrorCodeRevisionConflict, "image source changed")
	}
	return data, media, nil
}

func remoteImageContent(data []byte, media string) (string, error) {
	if strings.EqualFold(media, "image/svg+xml") {
		if _, err := decodeRemoteThumbnail(data, media); err != nil {
			return "", err
		}
		return "image/svg+xml", nil
	}
	if _, err := remoteImageConfig(data); err != nil {
		return "", err
	}
	if !strings.HasPrefix(media, "image/") {
		_, format, _ := image.DecodeConfig(bytes.NewReader(data))
		media = "image/" + format
	}
	return media, nil
}

func remoteImageDigest(data []byte) string {
	sum := blake3.Sum256(data)
	return hex.EncodeToString(sum[:])
}

func remoteImageConfig(data []byte) (image.Config, error) {
	if len(data) > remoteImageFetchBytes {
		return image.Config{}, fileActionFault(protocol.ErrorCodeInvalidRequest, "remote image exceeds its fetch limit")
	}
	config, _, err := image.DecodeConfig(bytes.NewReader(data))
	if err != nil {
		return image.Config{}, fileActionFault(protocol.ErrorCodeInvalidRequest, "remote image could not be decoded")
	}
	if config.Width <= 0 || config.Height <= 0 || config.Width > remoteImageFetchEdge || config.Height > remoteImageFetchEdge || config.Width > remoteImageFetchPixels/config.Height {
		return image.Config{}, fileActionFault(protocol.ErrorCodeInvalidRequest, "remote image dimensions exceed the fetch limit")
	}
	return config, nil
}

func decodeRemoteThumbnail(data []byte, media string) (*attachmentThumbnail, error) {
	if strings.EqualFold(media, "image/svg+xml") {
		svg, err := ui.ParseSVG(data)
		if err != nil {
			return nil, fileActionFault(protocol.ErrorCodeInvalidRequest, "remote image could not be decoded")
		}
		w, h := svg.Size()
		if !(w >= 1 && h >= 1 && w <= remoteImageFetchEdge && h <= remoteImageFetchEdge && w*h <= remoteImageFetchPixels) {
			return nil, fileActionFault(protocol.ErrorCodeInvalidRequest, "remote image dimensions exceed the fetch limit")
		}
		return &attachmentThumbnail{svg: svg, digest: remoteImageDigest(data)}, nil
	}
	config, err := remoteImageConfig(data)
	if err != nil {
		return nil, err
	}
	bitmap, err := ui.DecodeBitmapScaled(data, attachmentThumbnailEdge)
	if err != nil {
		return nil, fileActionFault(protocol.ErrorCodeInvalidRequest, "remote image could not be decoded")
	}
	digest, revision := remoteImageDigest(data), ""
	if max(config.Width, config.Height) > attachmentThumbnailEdge {
		revision = digest
	}
	return &attachmentThumbnail{bitmap: bitmap, originalRevision: revision, digest: digest}, nil
}

func decodeRemoteBitmap(data []byte) (*ui.Bitmap, error) {
	if _, err := remoteImageConfig(data); err != nil {
		return nil, err
	}
	bitmap, err := ui.DecodeBitmap(data)
	if err != nil {
		return nil, fileActionFault(protocol.ErrorCodeInvalidRequest, "remote image could not be decoded")
	}
	return bitmap, nil
}

func remoteImageDisplayName(address, label string) string {
	if text := strings.TrimSpace(label); text != "" {
		return text
	}
	return remoteImageFilename(address, "")
}

func remoteImageFilename(address, label string) string {
	if parsed, err := url.Parse(address); err == nil {
		if base := sanitizeRemoteFilename(path.Base(parsed.Path)); base != "" {
			return base
		}
	}
	if base := sanitizeRemoteFilename(label); base != "" {
		return base
	}
	return "image"
}

func sanitizeRemoteFilename(raw string) string {
	raw = strings.TrimSpace(raw)
	if raw == "" || raw == "." || raw == ".." || raw == "/" {
		return ""
	}
	var out []rune
	for _, r := range raw {
		if len(out) == 80 {
			break
		}
		if r == '/' || r == '\\' || r == 0 || unicode.IsControl(r) {
			out = append(out, '_')
			continue
		}
		out = append(out, r)
	}
	name := strings.Trim(strings.TrimSpace(string(out)), ".")
	if name == "" {
		return ""
	}
	return name
}
