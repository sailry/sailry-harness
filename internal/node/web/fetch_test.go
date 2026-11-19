package web

import (
	"context"
	"errors"
	"io"
	"net"
	"net/http"
	"net/http/httptest"
	"net/netip"
	"net/url"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/protocol"
)

type fixtureTransport func(*http.Request) (*http.Response, error)

func (transport fixtureTransport) RoundTrip(request *http.Request) (*http.Response, error) {
	return transport(request)
}

func fetchFixture() FetchOptions {
	return FetchOptions{Resolve: func(context.Context, string) ([]netip.Addr, error) {
		return []netip.Addr{netip.MustParseAddr("8.8.8.8")}, nil
	}, LocalAddresses: func() ([]netip.Addr, error) { return nil, nil }, Proxy: func(*http.Request) (*url.URL, error) { return nil, nil }}
}
func responseFixture(body, contentType string) *http.Response {
	return &http.Response{StatusCode: 200, Header: http.Header{"Content-Type": []string{contentType}}, Body: io.NopCloser(strings.NewReader(body)), ContentLength: int64(len(body))}
}

func TestPrivateAddressRejected(t *testing.T) {
	t.Parallel()
	for _, input := range []string{"http://127.0.0.1/", "http://[::1]/", "http://[::ffff:127.0.0.1]/", "http://10.0.0.1/", "http://169.254.169.254/", "http://100.64.0.1/", "http://192.0.2.1/", "http://198.19.1.1/", "http://[2001:db8::1]/", "http://[2002:0808:0808::1]/", "http://host.local/", "http://localhost./", "http://child.localhost/",
		// Address spellings a resolver or a proxy reads as loopback or private.
		"http://127.1/", "http://2130706433/", "http://0x7f000001/", "http://0x7f.0.0.1/", "http://0177.0.0.1/", "http://127.0.0.1./", "http://10.1/", "http://0/", "http://0x/",
		// A host that ends in a number is an address or nothing.
		"http://1.2.3.4.5/", "http://256.1.1.1/", "http://1..2/", "http://example.08/", "http://4294967296/"} {
		if _, err := publicURL(input); err == nil {
			t.Fatal("private URL accepted", input)
		}
	}
	for _, input := range []string{"https://8.8.8.8/", "https://[2001:4860:4860::8888]/", "https://example.com/path#fragment", "https://1e100.net/", "https://0x.example/"} {
		if _, err := publicURL(input); err != nil {
			t.Fatal(input, err)
		}
	}
	for input, host := range map[string]string{"https://134744072/dns": "8.8.8.8", "https://8.8.2056:8443/": "8.8.8.8:8443", "https://0x8.010.8.8/": "8.8.8.8"} {
		if parsed, err := publicURL(input); err != nil || parsed.Host != host {
			t.Fatalf("%s reads as %v, %v", input, parsed, err)
		}
	}
	options := fetchFixture()
	options.Resolve = func(context.Context, string) ([]netip.Addr, error) {
		return []netip.Addr{netip.MustParseAddr("8.8.8.8"), netip.MustParseAddr("127.0.0.1")}, nil
	}
	options.Transport = fixtureTransport(func(*http.Request) (*http.Response, error) {
		t.Error("mixed private DNS reached HTTP")
		return responseFixture("bad", "text/plain"), nil
	})
	if _, err := NewFetcher(options).Fetch(context.Background(), protocol.WebRequest{Url: "http://mixed.test/"}); err == nil {
		t.Fatal("mixed DNS accepted")
	}
	if err := checkAddresses([]netip.Addr{netip.MustParseAddr("8.8.8.8")}, []netip.Addr{netip.MustParseAddr("::ffff:8.8.8.8")}); err == nil {
		t.Fatal("local public interface accepted")
	}
}

func TestAutomaticHTTPSRequiresPublicDestination(t *testing.T) {
	t.Parallel()
	options := fetchFixture()
	options.Resolve = func(context.Context, string) ([]netip.Addr, error) {
		return []netip.Addr{netip.IPv6Loopback()}, nil
	}
	options.DialContext = func(context.Context, string, string) (net.Conn, error) {
		t.Error("a refused asset reached the dialer")
		return nil, errors.New("asset was rejected")
	}
	client := newPublicHTTPSClient(NewFetcher(options), time.Second, 4)
	defer client.CloseIdleConnections()
	redirects := fetchFixture()
	redirects.Transport = fixtureTransport(func(request *http.Request) (*http.Response, error) {
		location := map[string]string{"plain.example": "http://picture.example/", "private.example": "https://10.0.0.1/", "loopback.example": "https://127.1/"}[request.URL.Host]
		if location == "" {
			t.Error("a refused redirect reached HTTP", request.URL)
			return responseFixture("bad", "image/png"), nil
		}
		return &http.Response{StatusCode: http.StatusFound, Header: http.Header{"Location": []string{location}}, Body: http.NoBody, Request: request}, nil
	})
	redirected := newPublicHTTPSClient(NewFetcher(redirects), time.Second, 4)
	for _, check := range []struct {
		client *http.Client
		input  string
		code   protocol.ErrorCode
	}{
		{client, "http://picture.example/", protocol.ErrorCodeInvalidRequest},
		{client, "https://user@picture.example/", protocol.ErrorCodeInvalidRequest},
		{client, "https://localhost/", protocol.ErrorCodePermissionDenied},
		{client, "https://127.0.0.1/", protocol.ErrorCodePermissionDenied},
		{client, "https://picture.example/", protocol.ErrorCodePermissionDenied},
		{redirected, "https://plain.example/", protocol.ErrorCodeInvalidRequest},
		{redirected, "https://private.example/", protocol.ErrorCodePermissionDenied},
		{redirected, "https://loopback.example/", protocol.ErrorCodePermissionDenied},
	} {
		response, err := check.client.Get(check.input)
		if response != nil {
			response.Body.Close()
		}
		var fault *protocol.Fault
		if !errors.As(err, &fault) || fault.Code != check.code {
			t.Errorf("automatic asset destination %q: want %s, got %v", check.input, check.code, err)
		}
	}
}

func TestAutomaticHTTPSUsesProxy(t *testing.T) {
	t.Parallel()
	var mu sync.Mutex
	var tunnels []string
	proxyServer := httptest.NewServer(http.HandlerFunc(func(writer http.ResponseWriter, request *http.Request) {
		mu.Lock()
		tunnels = append(tunnels, request.Method+" "+request.Host)
		mu.Unlock()
		writer.WriteHeader(http.StatusForbidden)
	}))
	defer proxyServer.Close()
	proxy, _ := url.Parse(proxyServer.URL)
	options := fetchFixture()
	options.Proxy = func(*http.Request) (*url.URL, error) { return proxy, nil }
	options.Resolve = func(_ context.Context, host string) ([]netip.Addr, error) {
		if host == "private.example" {
			return []netip.Addr{netip.MustParseAddr("192.168.1.10")}, nil
		}
		return []netip.Addr{netip.MustParseAddr("8.8.8.8")}, nil
	}
	options.DialContext = func(ctx context.Context, network, address string) (net.Conn, error) {
		if address != proxy.Host {
			t.Error("an asset bypassed the proxy", address)
			return nil, errors.New("direct dial refused")
		}
		return (&net.Dialer{}).DialContext(ctx, network, address)
	}
	client := newPublicHTTPSClient(NewFetcher(options), 5*time.Second, 4)
	defer client.CloseIdleConnections()
	if response, err := client.Get("https://picture.example/a.png"); err == nil {
		response.Body.Close()
		t.Fatal("refused tunnel succeeded")
	}
	response, err := client.Get("https://private.example/a.png")
	if response != nil {
		response.Body.Close()
	}
	var fault *protocol.Fault
	if !errors.As(err, &fault) || fault.Code != protocol.ErrorCodePermissionDenied {
		t.Fatal("private proxied asset", err)
	}
	mu.Lock()
	defer mu.Unlock()
	if len(tunnels) != 1 || tunnels[0] != "CONNECT picture.example:443" {
		t.Fatal("asset tunnels", tunnels)
	}
}

func TestDNSPinnedAcrossRedirects(t *testing.T) {
	t.Parallel()
	server := httptest.NewServer(http.HandlerFunc(func(writer http.ResponseWriter, request *http.Request) {
		if request.Host == "first.test" {
			writer.Header().Set("Location", "http://second.test/final")
			writer.WriteHeader(http.StatusFound)
			return
		}
		if request.Host != "second.test" {
			t.Error("unexpected destination", request.Host)
		}
		_, _ = io.WriteString(writer, "fixture page")
	}))
	defer server.Close()
	options := fetchFixture()
	options.Resolve = func(_ context.Context, host string) ([]netip.Addr, error) {
		address := "8.8.8.8"
		if host == "second.test" {
			address = "1.1.1.1"
		}
		return []netip.Addr{netip.MustParseAddr(address)}, nil
	}
	var mu sync.Mutex
	var dialed []string
	options.DialContext = func(ctx context.Context, network, address string) (net.Conn, error) {
		mu.Lock()
		dialed = append(dialed, address)
		mu.Unlock()
		return (&net.Dialer{}).DialContext(ctx, network, server.Listener.Addr().String())
	}
	fetcher := NewFetcher(options)
	defer fetcher.Close()
	page, err := fetcher.Fetch(context.Background(), protocol.WebRequest{Url: "http://first.test/"})
	if err != nil || page.Content != "fixture page" || page.Url != "http://second.test/final" {
		t.Fatal(page, err)
	}
	mu.Lock()
	defer mu.Unlock()
	if len(dialed) != 2 || dialed[0] != "8.8.8.8:80" || dialed[1] != "1.1.1.1:80" {
		t.Fatal("connections were not pinned per destination", dialed)
	}
}

func TestProxyNoDirectFallback(t *testing.T) {
	t.Parallel()
	options := fetchFixture()
	proxy, _ := url.Parse("http://proxy.invalid:8888")
	options.Proxy = func(*http.Request) (*url.URL, error) { return proxy, nil }
	options.Resolve = func(context.Context, string) ([]netip.Addr, error) { return nil, errors.New("proxy resolves target") }
	dials := []string{}
	options.DialContext = func(_ context.Context, _ string, address string) (net.Conn, error) {
		dials = append(dials, address)
		return nil, errors.New("fixture proxy is unavailable")
	}
	fetcher := NewFetcher(options)
	defer fetcher.Close()
	if _, err := fetcher.Fetch(context.Background(), protocol.WebRequest{Url: "https://public.test/"}); err == nil {
		t.Fatal("failed proxy request succeeded")
	}
	if len(dials) != 1 || dials[0] != "proxy.invalid:8888" {
		t.Fatal("proxy failure fell back direct", dials)
	}
	if _, err := selectProxy(&url.URL{Scheme: "https", Host: "public.test"}, proxySettings{https: "ftp://proxy.invalid"}); err == nil {
		t.Fatal("unsupported proxy silently fell back direct")
	}
}

func TestWebSnapshotAndExtraction(t *testing.T) {
	t.Parallel()
	now := time.Unix(100, 0)
	options := fetchFixture()
	options.Now = func() time.Time { return now }
	loads := 0
	options.Transport = fixtureTransport(func(*http.Request) (*http.Response, error) {
		loads++
		return responseFixture(strings.Repeat("中", webPageCharacters)+"last", "text/plain"), nil
	})
	fetcher := NewFetcher(options)
	defer fetcher.Close()
	page, err := fetcher.Fetch(context.Background(), protocol.WebRequest{Url: "https://public.test/#fragment"})
	if err != nil || page.NextOffset == nil || *page.NextOffset != webPageCharacters || !page.Untrusted {
		t.Fatal(page, err)
	}
	page, err = fetcher.Fetch(context.Background(), protocol.WebRequest{Url: "https://public.test/", Offset: *page.NextOffset})
	if err != nil || page.Content != "last" || loads != 1 {
		t.Fatal(page, err, loads)
	}
	now = now.Add(15 * time.Minute)
	if _, err = fetcher.Fetch(context.Background(), protocol.WebRequest{Url: "https://public.test/", Offset: webPageCharacters}); err == nil {
		t.Fatal("expired page continued with different snapshot")
	}
	if _, err = fetcher.Fetch(context.Background(), protocol.WebRequest{Url: "https://public.test/"}); err != nil || loads != 2 {
		t.Fatal(err, loads)
	}
	title, markdown, err := convertHTML(`<html><head><title>  Fixture   title </title></head><body><nav>navigation</nav><div role="main"><h1>Content</h1><p>Visible <strong>text</strong></p><div hidden>hidden</div><div hidden role="tabpanel">alternative</div><script>alert(1)</script></div><aside>sidebar</aside></body></html>`)
	if err != nil || title == nil || *title != "Fixture title" || !strings.Contains(markdown, "**text**") || !strings.Contains(markdown, "alternative") || strings.Contains(markdown, "hidden") || strings.Contains(markdown, "navigation") || strings.Contains(markdown, "sidebar") || strings.Contains(markdown, "alert") {
		t.Fatal(title, markdown, err)
	}
	if got := decodeWebBody([]byte{0xff, 0xfe, 0x2d, 0x4e}, "text/plain; charset=iso-8859-1", false); got != "中" {
		t.Fatal("BOM did not override charset", got)
	}
	if got := decodeWebBody([]byte{0xd6, 0xd0}, "text/plain", false); got != "中" {
		t.Fatal("GB18030 fallback changed", got)
	}
	if got := decodeWebBody([]byte("text\x00\x01\n"), "text/plain", false); got != "text��\n" {
		t.Fatal("control normalization changed", got)
	}
	options.Transport = fixtureTransport(func(*http.Request) (*http.Response, error) { return responseFixture("<main></main>", "text/html"), nil })
	renderFetcher := NewFetcher(options)
	defer renderFetcher.Close()
	renderer := &Renderer{Key: protocol.NewSecret("fixture"), Transport: fixtureTransport(func(request *http.Request) (*http.Response, error) {
		if request.Header.Get("X-API-Key") != "fixture" {
			t.Error("renderer omitted captured key")
		}
		return responseFixture(`{"results":[{"url":"https://render.test/","final_url":"http://127.0.0.1/","text":"private"}],"errors":[]}`, "application/json"), nil
	})}
	if _, err := renderFetcher.FetchWith(context.Background(), protocol.WebRequest{Url: "https://render.test/"}, renderer); err == nil {
		t.Fatal("private rendered redirect fell back to sparse content")
	}
}
