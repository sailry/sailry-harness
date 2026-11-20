package live

import (
	"bytes"
	"context"
	"encoding/base64"
	"errors"
	"fmt"
	"image"
	"image/gif"
	"image/jpeg"
	"image/png"
	"io"
	"net/http"
	"strings"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestDocumentImageFormats(t *testing.T) {
	t.Parallel()
	bitmap := image.NewNRGBA(image.Rect(0, 0, 2, 3))
	for _, format := range []struct {
		name, media string
		encode      func(*bytes.Buffer) error
	}{
		{"png", "image/png", func(out *bytes.Buffer) error { return png.Encode(out, bitmap) }},
		{"jpeg", "image/jpeg", func(out *bytes.Buffer) error { return jpeg.Encode(out, bitmap, nil) }},
		{"gif", "image/gif", func(out *bytes.Buffer) error { return gif.Encode(out, bitmap, nil) }},
		{"svg", "image/svg+xml", func(out *bytes.Buffer) error {
			_, err := out.WriteString(`<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 2 3"><path d="M0 0h2v3H0z"/></svg>`)
			return err
		}},
	} {
		t.Run(format.name, func(t *testing.T) {
			var out bytes.Buffer
			if err := format.encode(&out); err != nil {
				t.Fatal(err)
			}
			uri := "data:" + format.media + ";base64," + base64.StdEncoding.EncodeToString(out.Bytes())
			images := &documentImages{ctx: t.Context(), images: make(map[documentImageKey]*documentImage), slots: make(chan struct{}, 2)}
			source, err := images.load(uri)
			if err != nil || source == nil {
				t.Fatalf("load %s: %v", format.name, err)
			}
			if format.name == "svg" {
				if _, ok := source.(*ui.SVG); !ok {
					t.Fatalf("SVG decoded as %T", source)
				}
			} else if _, ok := source.(*ui.Bitmap); !ok {
				t.Fatalf("raster image decoded as %T", source)
			}
			cached, err := images.load(uri)
			if err != nil || cached != source {
				t.Fatalf("image was not reused: %v", err)
			}
			closed, cancel := context.WithCancel(t.Context())
			cancel()
			images.ctx = closed
			if _, err := images.load(uri); err == nil {
				t.Fatal("closed document returned a cached image")
			}
		})
	}
}

const documentImageFixtureOrigin = "https://badges.example.test"
const documentImageFixtureSVG = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 84 20"><rect width="84" height="20" fill="#1681b6"/><text x="42" y="14" text-anchor="middle">passing</text></svg>`

type documentImageFixtureTransport func(*http.Request) (*http.Response, error)

func (f documentImageFixtureTransport) RoundTrip(request *http.Request) (*http.Response, error) {
	return f(request)
}

func documentImageHTTPSFixture(t *testing.T, serve documentImageFixtureTransport) *documentImages {
	client := newDocumentImageHTTPClient()
	public := client.Transport
	client.Transport = documentImageFixtureTransport(func(request *http.Request) (*http.Response, error) {
		if request.URL.Scheme != "https" || request.URL.User != nil || request.URL.Hostname() == "127.0.0.1" {
			return public.RoundTrip(request)
		}
		if request.URL.Host != "badges.example.test" {
			return nil, fmt.Errorf("unexpected document image host %q", request.URL.Host)
		}
		response, err := serve(request)
		if response != nil {
			response.Request = request
		}
		return response, err
	})
	return &documentImages{ctx: t.Context(), https: client, images: make(map[documentImageKey]*documentImage), slots: make(chan struct{}, 2)}
}

func documentImageFixtureResponse(media string, body []byte) *http.Response {
	return &http.Response{StatusCode: http.StatusOK, Header: http.Header{"Content-Type": {media}}, Body: io.NopCloser(bytes.NewReader(body)), ContentLength: int64(len(body))}
}

func TestDocumentHTTPSImageFormats(t *testing.T) {
	t.Parallel()
	var raster bytes.Buffer
	if err := png.Encode(&raster, image.NewNRGBA(image.Rect(0, 0, 2, 3))); err != nil {
		t.Fatal(err)
	}
	var requests int
	images := documentImageHTTPSFixture(t, func(request *http.Request) (*http.Response, error) {
		requests++
		if request.Method != http.MethodGet || request.URL.Fragment != "" || request.Header.Get("Referer") != "" || request.Header.Get("Cookie") != "" || request.Header.Get("Authorization") != "" {
			t.Errorf("image GET %s sent unexpected metadata: %v", request.URL, request.Header)
		}
		switch request.URL.RequestURI() {
		case "/redirect?style=flat-square":
			return &http.Response{StatusCode: http.StatusFound, Header: http.Header{
				"Location": {"/badge?style=flat-square&color=blue"}, "Set-Cookie": {"image=secret; Path=/"},
			}, Body: http.NoBody}, nil
		case "/badge?style=flat-square&color=blue":
			return documentImageFixtureResponse("image/svg+xml;charset=utf-8", []byte(documentImageFixtureSVG)), nil
		case "/fallback.SVG?name=badge.png":
			return documentImageFixtureResponse("application/octet-stream", []byte(documentImageFixtureSVG)), nil
		case "/raster?name=badge.svg":
			return documentImageFixtureResponse("application/octet-stream", raster.Bytes()), nil
		case "/raster.svg":
			return documentImageFixtureResponse("image/png", raster.Bytes()), nil
		default:
			return nil, fmt.Errorf("unexpected image request %s", request.URL)
		}
	})
	for _, path := range []string{"/redirect?style=flat-square", "/fallback.SVG?name=badge.png"} {
		address := documentImageFixtureOrigin + path
		source, err := images.load(address)
		if _, ok := source.(*ui.SVG); !ok || err != nil {
			t.Fatalf("SVG %s decoded as %T: %v", path, source, err)
		}
		if cached, err := images.load(address + "#different-fragment"); err != nil || cached != source {
			t.Fatalf("SVG %s was not reused: %v", path, err)
		}
	}
	for _, path := range []string{"/raster?name=badge.svg", "/raster.svg"} {
		loaded, err := images.load(documentImageFixtureOrigin + path)
		if _, ok := loaded.(*ui.Bitmap); !ok || err != nil {
			t.Fatalf("raster %s decoded as %T: %v", path, loaded, err)
		}
	}
	if requests != 5 {
		t.Fatalf("image cache or redirect resulted in %d requests, want 5", requests)
	}
}

func TestDocumentHTTPSRefusesUnsafeImages(t *testing.T) {
	t.Parallel()
	images := documentImageHTTPSFixture(t, func(request *http.Request) (*http.Response, error) {
		location := map[string]string{
			"/local-redirect": "https://127.0.0.1/badge",
			"/plain-redirect": "http://badges.example.test/badge",
			"/user-redirect":  "https://user:secret@badges.example.test/badge",
		}[request.URL.Path]
		if location == "" {
			return nil, fmt.Errorf("unexpected image request %s", request.URL)
		}
		return &http.Response{StatusCode: http.StatusFound, Header: http.Header{"Location": {location}}, Body: http.NoBody}, nil
	})
	for _, check := range []struct {
		address string
		code    protocol.ErrorCode
	}{
		{"//badges.example.test/badge", protocol.ErrorCodeInvalidRequest},
		{"http://badges.example.test/badge", protocol.ErrorCodeInvalidRequest},
		{"https://user@badges.example.test/badge", protocol.ErrorCodeInvalidRequest},
		{"https://127.0.0.1/badge", protocol.ErrorCodePermissionDenied},
		{documentImageFixtureOrigin + "/local-redirect", protocol.ErrorCodePermissionDenied},
		{documentImageFixtureOrigin + "/plain-redirect", protocol.ErrorCodeInvalidRequest},
		{documentImageFixtureOrigin + "/user-redirect", protocol.ErrorCodeInvalidRequest},
	} {
		if source, err := images.load(check.address); source != nil || faultCode(err) != check.code {
			t.Errorf("unsafe image %s produced %T, %v; want %v", check.address, source, err, check.code)
		}
	}
}

func TestDocumentHTTPSImageLimits(t *testing.T) {
	t.Parallel()
	var raster bytes.Buffer
	if err := png.Encode(&raster, image.NewGray(image.Rect(0, 0, documentImageFetchEdge+1, 1))); err != nil {
		t.Fatal(err)
	}
	images := documentImageHTTPSFixture(t, func(request *http.Request) (*http.Response, error) {
		switch request.URL.Path {
		case "/large-header":
			response := documentImageFixtureResponse("image/svg+xml", []byte(documentImageFixtureSVG))
			response.ContentLength = documentImageFetchBytes + 1
			return response, nil
		case "/large-body":
			return &http.Response{StatusCode: http.StatusOK, Header: http.Header{"Content-Type": {"image/svg+xml"}},
				Body: io.NopCloser(strings.NewReader(documentImageFixtureSVG + strings.Repeat(" ", documentImageFetchBytes))), ContentLength: -1}, nil
		case "/large-svg":
			return documentImageFixtureResponse("image/svg+xml", fmt.Appendf(nil, `<svg viewBox="0 0 %d 2"><path d="M0 0h1v2H0z"/></svg>`, documentImageFetchEdge+1)), nil
		case "/large-raster":
			return documentImageFixtureResponse("image/png", raster.Bytes()), nil
		default:
			return nil, fmt.Errorf("unexpected image request %s", request.URL)
		}
	})
	for _, path := range []string{"/large-header", "/large-body", "/large-svg", "/large-raster"} {
		if source, err := images.load(documentImageFixtureOrigin + path); source != nil || faultCode(err) != protocol.ErrorCodeInvalidRequest || !strings.Contains(err.Error(), "fetch limit") {
			t.Errorf("image limit %s produced %T, %v", path, source, err)
		}
	}
}

func TestDocumentCloseCancelsHTTPSImage(t *testing.T) {
	t.Parallel()
	started := make(chan struct{})
	images := documentImageHTTPSFixture(t, func(request *http.Request) (*http.Response, error) {
		close(started)
		<-request.Context().Done()
		return nil, request.Context().Err()
	})
	deadline, stop := context.WithTimeout(t.Context(), 5*time.Second)
	defer stop()
	images.ctx, images.cancel = context.WithCancel(deadline)
	defer images.cancel()
	done := make(chan error, 1)
	go func() {
		_, err := images.load(documentImageFixtureOrigin + "/slow")
		done <- err
	}()
	select {
	case <-started:
	case err := <-done:
		t.Fatalf("image failed before the request: %v", err)
	case <-deadline.Done():
		t.Fatal("image request did not start")
	}
	images.cancel()
	select {
	case err := <-done:
		if !errors.Is(err, context.Canceled) {
			t.Fatalf("closed document image failed with %v, want cancellation", err)
		}
	case <-deadline.Done():
		t.Fatal("image request survived document close")
	}
}

func TestDocumentHTMLKeepsImageSandbox(t *testing.T) {
	t.Parallel()
	var requests int
	images := documentImageHTTPSFixture(t, func(request *http.Request) (*http.Response, error) {
		requests++
		return documentImageFixtureResponse("image/svg+xml", []byte(documentImageFixtureSVG)), nil
	})
	if _, err := images.load(documentImageFixtureOrigin + "/cached"); err != nil {
		t.Fatal(err)
	}
	result := images.inlineHTML(`<picture><source srcset="https://example.com/a.svg"><img src="../private.svg" srcset="https://example.com/b.svg" alt="Missing image"></picture>` +
		`<img src="` + documentImageFixtureOrigin + `/cached" alt="Cached badge"><img src="` + documentImageFixtureOrigin + `/new" alt="New badge">` +
		`<img src="data:image/gif;base64,AAAA" alt="Embedded image"><script>window.localOnly=true</script>`)
	if strings.Contains(result, "srcset") || strings.Contains(result, "../private.svg") || !strings.Contains(result, `alt="Missing image"`) {
		t.Fatalf("unknown image source escaped the placeholder: %s", result)
	}
	if requests != 1 || strings.Contains(result, documentImageFixtureOrigin) || strings.Contains(result, "data:image/svg+xml") || !strings.Contains(result, `alt="Cached badge"`) || !strings.Contains(result, `alt="New badge"`) {
		t.Fatalf("sandboxed HTML fetched or inlined a remote image: %s (requests=%d)", result, requests)
	}
	if !strings.Contains(result, `src="data:image/gif;base64,AAAA"`) {
		t.Fatalf("image rewriting removed an existing data URL: %s", result)
	}
	if !strings.Contains(result, "<script>window.localOnly=true</script>") {
		t.Fatal("image rewriting changed the sandboxed document's scripts")
	}
	if !strings.Contains(documentPage(result, true, documentMount{}), "img-src data: blob:") {
		t.Fatal("document image loading relaxed the sandbox policy")
	}
}
