package live

import (
	"bytes"
	"context"
	"encoding/base64"
	"errors"
	"fmt"
	"image"
	"net/http"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"testing"
	"testing/synctest"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
	"github.com/sailry/sailry-harness/internal/protocol"
)

const remoteImageFixtureOrigin = "https://cdn.example.test"

// Valid 1x1 lossless WebP (VP8L) image.
const remoteImageFixtureWebPBase64 = "UklGRh4AAABXRUJQVlA4TBEAAAAvAAAAAAfQ//73v/+BiOh/AAA="

func remoteImageHTTPSClient(t *testing.T, serve documentImageFixtureTransport) *http.Client {
	t.Helper()
	client := newPublicImageHTTPClient(remoteImageFetchTimeout, remoteImageFetchRedirects)
	public := client.Transport
	client.Transport = documentImageFixtureTransport(func(request *http.Request) (*http.Response, error) {
		if request.URL.Scheme != "https" || request.URL.User != nil || request.URL.Hostname() == "127.0.0.1" {
			return public.RoundTrip(request)
		}
		if request.URL.Host != "cdn.example.test" {
			return nil, fmt.Errorf("unexpected remote image host %q", request.URL.Host)
		}
		response, err := serve(request)
		if response != nil {
			response.Request = request
		}
		return response, err
	})
	return client
}

func TestRemoteConversationImagesFormatsAndViewer(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		r, _, settle := readRecoveryFixture(t)
		session := protocol.SessionID{9}
		id := session.String()
		r.conversationSeen = map[protocol.SessionID]bool{session: true}
		cv := &viewmodel.Conversation{ID: id}

		largePNG := cardPNG(t, 1536, 768)
		webpBytes, err := base64.StdEncoding.DecodeString(remoteImageFixtureWebPBase64)
		if err != nil {
			t.Fatal(err)
		}
		requests := map[string]int{}
		r.attachments.remoteHTTP = remoteImageHTTPSClient(t, func(request *http.Request) (*http.Response, error) {
			if request.Method != http.MethodGet || request.URL.Fragment != "" ||
				request.Header.Get("Referer") != "" || request.Header.Get("Cookie") != "" || request.Header.Get("Authorization") != "" {
				t.Errorf("remote image GET %s sent unexpected metadata: %v", request.URL, request.Header)
			}
			uri := request.URL.RequestURI()
			requests[uri]++
			switch uri {
			case "/gen/poster.png?sig=abc&exp=123":
				return documentImageFixtureResponse("image/png", largePNG), nil
			case "/assets/desktop-home.webp":
				return documentImageFixtureResponse("application/octet-stream", webpBytes), nil
			case "/badges/status.svg":
				return documentImageFixtureResponse("application/octet-stream", []byte(documentImageFixtureSVG)), nil
			default:
				return nil, fmt.Errorf("unexpected remote image request %s", uri)
			}
		})

		// Large PNG with query string and fragment: scaled thumbnail + on-demand full-res viewer.
		pngDest := remoteImageFixtureOrigin + "/gen/poster.png?sig=abc&exp=123#frag"
		embed, ok := r.chatEmbed(cv, pngDest, "Generated poster", true)
		if !ok || embed.Image == nil || embed.Image.Name != "Generated poster" {
			t.Fatalf("PNG chatEmbed = (%+v, %v)", embed, ok)
		}
		r.resolveConversationImage(id, *embed.Image)
		settle()
		seenPNG := r.resolveConversationImage(id, *embed.Image)
		if seenPNG.Width != 480 || seenPNG.Height != 240 || seenPNG.OriginalRevision == "" || seenPNG.Missing {
			t.Fatalf("PNG thumbnail = %+v", seenPNG)
		}
		if w, h := ui.ImageSize(seenPNG.Source); w != 480 || h != 240 {
			t.Fatalf("PNG thumbnail source = %gx%g, want 480x240", w, h)
		}
		if got := r.resolveConversationViewerImage(id, seenPNG); got != seenPNG.Source {
			t.Fatal("viewer did not show thumbnail while loading full-res remote PNG")
		}
		settle()
		viewerPNG := r.resolveConversationViewerImage(id, seenPNG)
		if w, h := ui.ImageSize(viewerPNG); w != 1536 || h != 768 {
			t.Fatalf("viewer PNG = %gx%g, want 1536x768", w, h)
		}
		if requests["/gen/poster.png?sig=abc&exp=123"] != 2 {
			t.Fatalf("PNG requests = %d, want 2 (thumbnail + viewer)", requests["/gen/poster.png?sig=abc&exp=123"])
		}

		// WebP served as octet-stream: decoded via webp decoder; small size needs no viewer re-fetch.
		webpDest := remoteImageFixtureOrigin + "/assets/desktop-home.webp"
		webpEmbed, ok := r.chatEmbed(cv, webpDest, "Sailry desktop", true)
		if !ok || webpEmbed.Image == nil {
			t.Fatalf("WebP chatEmbed = (%+v, %v)", webpEmbed, ok)
		}
		r.resolveConversationImage(id, *webpEmbed.Image)
		settle()
		seenWebP := r.resolveConversationImage(id, *webpEmbed.Image)
		if _, ok := seenWebP.Source.(*ui.Bitmap); !ok || seenWebP.Width != 1 || seenWebP.Height != 1 || seenWebP.OriginalRevision != "" {
			t.Fatalf("WebP thumbnail = %+v (%T)", seenWebP, seenWebP.Source)
		}
		if got := r.resolveConversationViewerImage(id, seenWebP); got != seenWebP.Source {
			t.Fatal("small WebP viewer did not reuse thumbnail source")
		}
		if requests["/assets/desktop-home.webp"] != 1 {
			t.Fatalf("WebP requests = %d, want 1", requests["/assets/desktop-home.webp"])
		}

		// SVG served as octet-stream on .svg path: parsed as *ui.SVG; viewer reuses vector directly.
		svgDest := remoteImageFixtureOrigin + "/badges/status.svg"
		svgEmbed, ok := r.chatEmbed(cv, svgDest, "", true)
		if !ok || svgEmbed.Image == nil || svgEmbed.Image.Name != "status.svg" {
			t.Fatalf("SVG chatEmbed = (%+v, %v)", svgEmbed, ok)
		}
		r.resolveConversationImage(id, *svgEmbed.Image)
		settle()
		seenSVG := r.resolveConversationImage(id, *svgEmbed.Image)
		if _, ok := seenSVG.Source.(*ui.SVG); !ok || seenSVG.Width != 84 || seenSVG.Height != 20 || seenSVG.OriginalRevision != "" {
			t.Fatalf("SVG thumbnail = %+v (%T)", seenSVG, seenSVG.Source)
		}
		if got := r.resolveConversationViewerImage(id, seenSVG); got != seenSVG.Source {
			t.Fatal("SVG viewer did not reuse vector source")
		}
		if requests["/badges/status.svg"] != 1 {
			t.Fatalf("SVG requests = %d, want 1", requests["/badges/status.svg"])
		}

		// Off-screen eviction releases all raster pixels and pixel budget.
		r.pruneAttachmentImages()
		r.pruneAttachmentImages()
		for _, retained := range []ui.ImageSource{seenPNG.Source, viewerPNG, seenWebP.Source} {
			if w, h := ui.ImageSize(retained); w != 0 || h != 0 {
				t.Fatalf("evicted remote bitmap retained %gx%g pixels", w, h)
			}
		}
		if r.retainedThumbnailPixels() != 0 {
			t.Fatalf("retainedThumbnailPixels = %d, want 0 after eviction", r.retainedThumbnailPixels())
		}
	})
}

func TestRemoteConversationImageActions(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		r, _, settle := readRecoveryFixture(t)
		session := protocol.SessionID{10}
		id := session.String()
		r.conversationSeen = map[protocol.SessionID]bool{session: true}
		cv := &viewmodel.Conversation{ID: id}
		pngBytes := cardPNG(t, 12, 8)
		dest := remoteImageFixtureOrigin + "/exports/result.png?v=1"
		r.attachments.remoteHTTP = remoteImageHTTPSClient(t, func(request *http.Request) (*http.Response, error) {
			if request.URL.RequestURI() != "/exports/result.png?v=1" {
				return nil, fmt.Errorf("unexpected request %s", request.URL)
			}
			return documentImageFixtureResponse("application/octet-stream", pngBytes), nil
		})

		embed, ok := r.chatEmbed(cv, dest, "Result", true)
		if !ok || embed.Image == nil {
			t.Fatalf("chatEmbed = (%+v, %v)", embed, ok)
		}
		descriptor, ok := r.attachmentDescriptor(id, embed.Image.ID)
		if !ok || descriptor.remoteURL != dest || descriptor.name != "result.png" {
			t.Fatalf("descriptor = (%+v, %v)", descriptor, ok)
		}

		var copied []byte
		var copiedMime string
		saveTarget := filepath.Join(t.TempDir(), "saved.png")
		r.SetNativeFileActions(NativeFileActions{
			CopyImage: func(data []byte, mime string) {
				copied = append([]byte(nil), data...)
				copiedMime = mime
			},
			SaveFile: func(name string, selected func(string)) {
				if name != "result.png" {
					t.Errorf("SaveFile default name = %q, want result.png", name)
				}
				selected(saveTarget)
			},
		})

		// ImageOpen opens the remote URL in the browser.
		var openErr error
		tt := ui.NewTester(func(c *ui.Context) {
			theme.Apply(c, "", theme.Light)
			ui.Row(c).Gap(8).Children(func() {
				if ui.ButtonBase(c).Size(80, 30).Label("open").Clicked() {
					r.attachmentImageDescriptorAction(c, kit.ImageOpen, descriptor)
				}
				if ui.ButtonBase(c).Size(80, 30).Label("copy").Clicked() {
					r.attachmentImageDescriptorAction(c, kit.ImageCopy, descriptor)
				}
				if ui.ButtonBase(c).Size(80, 30).Label("save").Clicked() {
					r.attachmentImageDescriptorAction(c, kit.ImageSave, descriptor)
				}
			})
		}, 400, 300)
		tt.Frame()
		openErr = tt.Click("open")
		if openErr != nil {
			t.Fatal(openErr)
		}
		tt.Frame()
		if got := tt.OpenedURLs(); !slices.Equal(got, []string{dest}) {
			t.Fatalf("ImageOpen opened %q, want [%q]", got, dest)
		}

		// ImageCopy fetches bytes and detects image/png even from application/octet-stream.
		if err := tt.Click("copy"); err != nil {
			t.Fatal(err)
		}
		tt.Frame()
		settle()
		if !bytes.Equal(copied, pngBytes) || copiedMime != "image/png" {
			t.Fatalf("ImageCopy got %d bytes (%q), want %d bytes (image/png)", len(copied), copiedMime, len(pngBytes))
		}

		// ImageSave writes fetched bytes atomically to the selected path.
		if err := tt.Click("save"); err != nil {
			t.Fatal(err)
		}
		tt.Frame()
		settle()
		saved, err := os.ReadFile(saveTarget)
		if err != nil {
			t.Fatalf("reading saved remote image: %v", err)
		}
		if !bytes.Equal(saved, pngBytes) {
			t.Fatalf("saved remote image mismatch: got %d bytes, want %d", len(saved), len(pngBytes))
		}
	})
}

func TestRemoteConversationImageFailuresAndSecurity(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		r, _, settle := readRecoveryFixture(t)
		session := protocol.SessionID{11}
		id := session.String()
		r.conversationSeen = map[protocol.SessionID]bool{session: true}
		cv := &viewmodel.Conversation{ID: id}

		var requested []string
		r.attachments.remoteHTTP = remoteImageHTTPSClient(t, func(request *http.Request) (*http.Response, error) {
			requested = append(requested, request.URL.String())
			switch request.URL.Path {
			case "/not-found.png":
				return &http.Response{StatusCode: http.StatusNotFound, Header: http.Header{}, Body: http.NoBody}, nil
			case "/forbidden.png":
				return &http.Response{StatusCode: http.StatusForbidden, Header: http.Header{}, Body: http.NoBody}, nil
			case "/server-error.png":
				return &http.Response{StatusCode: http.StatusServiceUnavailable, Header: http.Header{}, Body: http.NoBody}, nil
			case "/page.html":
				return documentImageFixtureResponse("text/html", []byte("<html>not an image</html>")), nil
			case "/oversized.png":
				resp := documentImageFixtureResponse("image/png", cardPNG(t, 2, 2))
				resp.ContentLength = remoteImageFetchBytes + 1
				return resp, nil
			case "/local-redirect.png":
				return &http.Response{StatusCode: http.StatusFound, Header: http.Header{"Location": {"https://127.0.0.1/secret.png"}}, Body: http.NoBody}, nil
			default:
				return nil, fmt.Errorf("unexpected request %s", request.URL)
			}
		})

		// Non-HTTPS http:// URL degrades immediately to a clickable link without any fetch.
		if embed, ok := r.chatEmbed(cv, "http://cdn.example.test/plain.png", "Plain", true); !ok || embed != (conversation.Embed{}) {
			t.Fatalf("http:// chatEmbed = (%+v, %v), want empty fallback embed", embed, ok)
		}
		// Credentials in URL are rejected completely without fetching.
		if embed, ok := r.chatEmbed(cv, "https://user:secret@cdn.example.test/private.png", "Secret", true); ok || embed != (conversation.Embed{}) {
			t.Fatalf("credentials chatEmbed = (%+v, %v), want declined", embed, ok)
		}
		if len(requested) != 0 {
			t.Fatalf("rejected URLs triggered HTTP requests: %v", requested)
		}

		// Permanent failures (404, 403, HTML, oversized, private IP, redirect to private IP) degrade to clickable links.
		for _, target := range []string{
			remoteImageFixtureOrigin + "/not-found.png",
			remoteImageFixtureOrigin + "/forbidden.png",
			remoteImageFixtureOrigin + "/page.html",
			remoteImageFixtureOrigin + "/oversized.png",
			"https://127.0.0.1/secret.png",
			remoteImageFixtureOrigin + "/local-redirect.png",
		} {
			embed, ok := r.chatEmbed(cv, target, "Fail", true)
			if !ok || embed.Image == nil {
				t.Fatalf("%s initial chatEmbed = (%+v, %v)", target, embed, ok)
			}
			r.resolveConversationImage(id, *embed.Image)
			settle()
			cache := r.attachmentImages(id)
			key := attachmentKey(id, embed.Image.ID)
			if !cache.missing[key] {
				t.Errorf("%s did not mark missing (failed=%v)", target, cache.failed[key])
			}
			fallback, ok := r.chatEmbed(cv, target, "Fail", true)
			if !ok || fallback != (conversation.Embed{}) {
				t.Errorf("%s after failure chatEmbed = (%+v, %v), want clickable link fallback", target, fallback, ok)
			}
		}

		// Transient 503 marks failed (not missing) and still degrades to clickable link instead of spinning forever.
		transient := remoteImageFixtureOrigin + "/server-error.png"
		embed, ok := r.chatEmbed(cv, transient, "Retryable", true)
		if !ok || embed.Image == nil {
			t.Fatalf("transient initial chatEmbed = (%+v, %v)", embed, ok)
		}
		r.resolveConversationImage(id, *embed.Image)
		settle()
		cache := r.attachmentImages(id)
		key := attachmentKey(id, embed.Image.ID)
		if !cache.failed[key] || cache.missing[key] {
			t.Fatalf("503 status: failed=%v missing=%v, want failed=true missing=false", cache.failed[key], cache.missing[key])
		}
		if fallback, ok := r.chatEmbed(cv, transient, "Retryable", true); !ok || fallback != (conversation.Embed{}) {
			t.Fatalf("503 after failure chatEmbed = (%+v, %v), want clickable link fallback", fallback, ok)
		}
	})
}

func TestRemoteConversationImageCancelsOnClose(t *testing.T) {
	t.Parallel()
	r, _, _ := readRecoveryFixture(t)
	session := protocol.SessionID{12}
	id := session.String()
	r.conversationSeen = map[protocol.SessionID]bool{session: true}
	cv := &viewmodel.Conversation{ID: id}

	started := make(chan struct{})
	cancelled := make(chan error, 1)
	r.attachments.remoteHTTP = remoteImageHTTPSClient(t, func(request *http.Request) (*http.Response, error) {
		close(started)
		<-request.Context().Done()
		cancelled <- request.Context().Err()
		return nil, request.Context().Err()
	})

	embed, ok := r.chatEmbed(cv, remoteImageFixtureOrigin+"/slow.png", "Slow", true)
	if !ok || embed.Image == nil {
		t.Fatalf("chatEmbed = (%+v, %v)", embed, ok)
	}
	r.resolveConversationImage(id, *embed.Image)

	select {
	case <-started:
	case <-time.After(5 * time.Second):
		t.Fatal("remote image request did not start")
	}

	r.closeAttachments()
	select {
	case err := <-cancelled:
		if !errors.Is(err, context.Canceled) {
			t.Fatalf("closeAttachments ended request with %v, want context.Canceled", err)
		}
	case <-time.After(5 * time.Second):
		t.Fatal("remote image request survived closeAttachments")
	}
}

func TestRemoteConversationImageActionsMatchShownPicture(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		r, _, settle := readRecoveryFixture(t)
		session := protocol.SessionID{13}
		id := session.String()
		r.conversationSeen = map[protocol.SessionID]bool{session: true}
		cv := &viewmodel.Conversation{ID: id}
		shown, changed := cardPNG(t, 12, 8), cardPNG(t, 14, 8)
		served := shown
		media := "image/png"
		r.attachments.remoteHTTP = remoteImageHTTPSClient(t, func(*http.Request) (*http.Response, error) {
			return documentImageFixtureResponse(media, served), nil
		})
		var copied [][]byte
		saveTarget := filepath.Join(t.TempDir(), "saved.png")
		r.SetNativeFileActions(NativeFileActions{
			CopyImage: func(data []byte, _ string) { copied = append(copied, append([]byte(nil), data...)) },
			SaveFile:  func(_ string, selected func(string)) { selected(saveTarget) },
		})
		embed, ok := r.chatEmbed(cv, remoteImageFixtureOrigin+"/live/result.png", "Result", true)
		if !ok || embed.Image == nil {
			t.Fatalf("chatEmbed = (%+v, %v)", embed, ok)
		}
		var action kit.ImageAction
		tt := ui.NewTester(func(c *ui.Context) {
			theme.Apply(c, "", theme.Light)
			if ui.ButtonBase(c).Size(80, 30).Label("act").Clicked() {
				r.attachmentImageAction(&conversationActions{r: r, c: c, cv: cv}, action, *embed.Image)
			}
		}, 200, 100)
		act := func(a kit.ImageAction) {
			t.Helper()
			action = a
			tt.Frame()
			if err := tt.Click("act"); err != nil {
				t.Fatal(err)
			}
			tt.Frame()
			settle()
			tt.Frame()
		}

		// Before a thumbnail has shown there is no picture to match.
		act(kit.ImageCopy)
		if len(copied) != 0 {
			t.Fatalf("copied %d images before the thumbnail showed", len(copied))
		}
		r.resolveConversationImage(id, *embed.Image)
		settle()
		if seen := r.resolveConversationImage(id, *embed.Image); seen.Source == nil {
			t.Fatalf("thumbnail did not load: %+v", seen)
		}

		// A success status serving HTML is neither copied nor saved.
		served, media = []byte("<html>login</html>"), "text/html"
		act(kit.ImageCopy)
		act(kit.ImageSave)
		if len(copied) != 0 {
			t.Fatalf("copied HTML as an image")
		}
		if _, err := os.Stat(saveTarget); !errors.Is(err, os.ErrNotExist) {
			t.Fatalf("HTML response was saved: %v", err)
		}

		// A different picture at the same address conflicts with the one shown.
		served, media = changed, "image/png"
		act(kit.ImageCopy)
		act(kit.ImageSave)
		if len(copied) != 0 {
			t.Fatalf("copied a picture that differs from the shown one")
		}
		if _, err := os.Stat(saveTarget); !errors.Is(err, os.ErrNotExist) {
			t.Fatalf("changed picture was saved: %v", err)
		}

		// The shown bytes copy and save.
		served, media = shown, "application/octet-stream"
		act(kit.ImageCopy)
		act(kit.ImageSave)
		if len(copied) != 1 || !bytes.Equal(copied[0], shown) {
			t.Fatalf("copied %d images, want the shown picture", len(copied))
		}
		if saved, err := os.ReadFile(saveTarget); err != nil || !bytes.Equal(saved, shown) {
			t.Fatalf("saved picture = %d bytes, %v", len(saved), err)
		}
	})
}

func TestRemoteImageContentRejectsNonImages(t *testing.T) {
	t.Parallel()
	for _, tc := range []struct{ data, media string }{
		{"<html>not an image</html>", "text/html"},
		{"<html>not an image</html>", "image/png"},
		{"<html>not an image</html>", "image/svg+xml"},
	} {
		if media, err := remoteImageContent([]byte(tc.data), tc.media); err == nil {
			t.Fatalf("remoteImageContent(%q) = %q, want error", tc.media, media)
		}
	}
	if media, err := remoteImageContent(cardPNG(t, 2, 2), "application/octet-stream"); err != nil || media != "image/png" {
		t.Fatalf("remoteImageContent(PNG) = %q, %v", media, err)
	}
}

func TestRemoteImageFilenameAndDisplayName(t *testing.T) {
	t.Parallel()
	if got := remoteImageDisplayName("https://cdn.example.test/a/b/desktop-home.webp", "带壁纸首页"); got != "带壁纸首页" {
		t.Fatalf("displayName with label = %q", got)
	}
	if got := remoteImageDisplayName("https://cdn.example.test/a/b/desktop-home.webp?sig=1", ""); got != "desktop-home.webp" {
		t.Fatalf("displayName without label = %q", got)
	}
	if got := remoteImageFilename("https://cdn.example.test/a/b/desktop-home.webp?sig=1", "alt"); got != "desktop-home.webp" {
		t.Fatalf("filename from path = %q", got)
	}
	if got := remoteImageFilename("https://cdn.example.test/", "generated/poster"); got != "generated_poster" || strings.ContainsAny(got, `/\`) {
		t.Fatalf("filename from label = %q", got)
	}
	if got := remoteImageFilename("https://cdn.example.test/", ""); got != "image" {
		t.Fatalf("default filename = %q", got)
	}
	_ = image.Rectangle{}
}
