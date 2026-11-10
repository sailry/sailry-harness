package web

import (
	"context"
	"errors"
	"io"
	"net"
	"net/http"
	"net/netip"
	"net/url"
	"strings"
	"sync"
	"time"
	"unicode/utf8"

	"github.com/sailry/sailry-harness/internal/office"
	"github.com/sailry/sailry-harness/internal/protocol"
)

const webBodyLimit = 10 * 1024 * 1024
const webPageCharacters = 20000
const sparseNotice = "This page has little or no readable text; it appears to be rendered by JavaScript or embedded in a frame. Use a browser tool to read it"

type FetchOptions struct {
	Resolve        func(context.Context, string) ([]netip.Addr, error)
	LocalAddresses func() ([]netip.Addr, error)
	Proxy          func(*http.Request) (*url.URL, error)
	DialContext    func(context.Context, string, string) (net.Conn, error)
	Transport      http.RoundTripper
	Now            func() time.Time
}

type webDocument struct {
	url, contentType, content string
	title                     *string
	status                    uint16
	fetched                   time.Time
}
type fetchSlot struct {
	gate       chan struct{}
	document   *webDocument
	used       time.Time
	references int
}
type fetchClient struct {
	key       string
	transport *http.Transport
}
type Fetcher struct {
	options FetchOptions
	mu      sync.Mutex
	cache   map[string]*fetchSlot
	clients []fetchClient
}

func NewFetcher(options FetchOptions) *Fetcher {
	if options.Resolve == nil {
		options.Resolve = resolvePublic
	}
	if options.LocalAddresses == nil {
		options.LocalAddresses = localAddresses
	}
	if options.Proxy == nil {
		options.Proxy = systemProxy
	}
	if options.Now == nil {
		options.Now = time.Now
	}
	if options.DialContext == nil {
		options.DialContext = (&net.Dialer{Timeout: 10 * time.Second}).DialContext
	}
	return &Fetcher{options: options, cache: map[string]*fetchSlot{}}
}

func (fetcher *Fetcher) Fetch(ctx context.Context, input protocol.WebRequest) (protocol.WebPage, error) {
	return fetcher.FetchWith(ctx, input, nil)
}

func (fetcher *Fetcher) FetchWith(ctx context.Context, input protocol.WebRequest, renderer *Renderer) (protocol.WebPage, error) {
	parsed, err := publicURL(input.Url)
	if err != nil {
		return protocol.WebPage{}, err
	}
	ctx, cancel := context.WithTimeout(ctx, 30*time.Second)
	defer cancel()
	fetcher.mu.Lock()
	slot := fetcher.cache[parsed.String()]
	if slot == nil {
		if input.Offset != 0 {
			fetcher.mu.Unlock()
			return protocol.WebPage{}, restartPage()
		}
		fetcher.trimLocked(99)
		slot = &fetchSlot{gate: make(chan struct{}, 1)}
		slot.gate <- struct{}{}
		fetcher.cache[parsed.String()] = slot
	}
	slot.references++
	slot.used = fetcher.options.Now()
	fetcher.mu.Unlock()
	defer func() { fetcher.mu.Lock(); slot.references--; fetcher.trimLocked(100); fetcher.mu.Unlock() }()
	select {
	case <-ctx.Done():
		return protocol.WebPage{}, fetchContextError(ctx)
	case <-slot.gate:
	}
	defer func() { slot.gate <- struct{}{} }()
	if slot.document != nil && fetcher.options.Now().Sub(slot.document.fetched) >= 15*time.Minute {
		slot.document = nil
	}
	if slot.document == nil {
		if input.Offset != 0 {
			return protocol.WebPage{}, restartPage()
		}
		document, err := fetcher.load(ctx, parsed)
		if err != nil {
			return protocol.WebPage{}, err
		}
		if renderer != nil && sparseDocument(document) {
			rendered, renderErr := renderer.render(ctx, fetcher, document)
			if renderErr == nil {
				document = rendered
			} else {
				var fault *protocol.Fault
				if errors.As(renderErr, &fault) && fault.Code == protocol.ErrorCodePermissionDenied {
					return protocol.WebPage{}, renderErr
				}
			}
		}
		slot.document = &document
	}
	return slot.document.page(input.Offset)
}

func (fetcher *Fetcher) trimLocked(capacity int) {
	for len(fetcher.cache) > capacity {
		oldest := ""
		var used time.Time
		for key, slot := range fetcher.cache {
			if slot.references == 0 && (oldest == "" || slot.used.Before(used)) {
				oldest, used = key, slot.used
			}
		}
		if oldest == "" {
			return
		}
		delete(fetcher.cache, oldest)
	}
}
func restartPage() error {
	return webFault(protocol.ErrorCodeNotFound, "web page is no longer cached; restart with offset 0")
}
func fetchContextError(ctx context.Context) error {
	if errors.Is(ctx.Err(), context.Canceled) {
		return webFault(protocol.ErrorCodeCancelled, "web fetch was cancelled")
	}
	return webUnavailable("web fetch deadline exceeded")
}
func sparseDocument(document webDocument) bool {
	return document.status >= 200 && document.status < 300 && (document.contentType == "text/html" || document.contentType == "application/xhtml+xml") && utf8.RuneCountInString(strings.TrimSpace(document.content)) < 200
}

func (document *webDocument) page(offset uint64) (protocol.WebPage, error) {
	start, characters := 0, uint64(0)
	for characters < offset && start < len(document.content) {
		_, size := utf8.DecodeRuneInString(document.content[start:])
		start += size
		characters++
	}
	if characters != offset {
		return protocol.WebPage{}, webInvalid("web character offset is out of range")
	}
	end, count := start, 0
	for count < webPageCharacters && end < len(document.content) {
		_, size := utf8.DecodeRuneInString(document.content[end:])
		end += size
		count++
	}
	page := protocol.WebPage{Url: document.url, Status: document.status, ContentType: document.contentType, Content: document.content[start:end], Title: document.title, Truncated: end < len(document.content), Untrusted: true}
	if page.Title != nil {
		title := *page.Title
		page.Title = &title
	}
	if page.Truncated {
		next := offset + webPageCharacters
		page.NextOffset = &next
	}
	if offset == 0 && sparseDocument(*document) {
		notice := sparseNotice
		page.Notice = &notice
	}
	return page, nil
}

func (fetcher *Fetcher) load(ctx context.Context, parsed *url.URL) (webDocument, error) {
	for redirects := 0; redirects <= 5; redirects++ {
		request, err := http.NewRequestWithContext(ctx, http.MethodGet, parsed.String(), nil)
		if err != nil {
			return webDocument{}, webInvalid("invalid web URL")
		}
		request.Header.Set("User-Agent", "Sailry/1")
		request.Header.Set("Accept", "text/html, application/json, text/plain, */*;q=0.5")
		proxy, err := fetcher.options.Proxy(request)
		if err != nil {
			return webDocument{}, webUnavailable("unsupported web proxy")
		}
		if err = fetcher.validateDestination(ctx, parsed, proxy != nil); err != nil {
			return webDocument{}, err
		}
		transport, err := fetcher.transport(proxy)
		if err != nil {
			return webDocument{}, err
		}
		response, err := transport.RoundTrip(request)
		if err != nil {
			if ctx.Err() != nil {
				return webDocument{}, fetchContextError(ctx)
			}
			return webDocument{}, webUnavailable("web connection failed")
		}
		if response.StatusCode == 301 || response.StatusCode == 302 || response.StatusCode == 303 || response.StatusCode == 307 || response.StatusCode == 308 {
			response.Body.Close()
			if redirects == 5 {
				return webDocument{}, webUnavailable("web redirect limit exceeded")
			}
			location := response.Header.Get("Location")
			if location == "" {
				return webDocument{}, webUnavailable("web redirect has no valid location")
			}
			target, err := parsed.Parse(location)
			if err != nil {
				return webDocument{}, webInvalid("invalid web redirect URL")
			}
			parsed, err = publicURL(target.String())
			if err != nil {
				return webDocument{}, err
			}
			continue
		}
		document, err := fetcher.readResponse(response, parsed)
		response.Body.Close()
		return document, err
	}
	return webDocument{}, webUnavailable("web redirect limit exceeded")
}

func (fetcher *Fetcher) readResponse(response *http.Response, parsed *url.URL) (webDocument, error) {
	if response.ContentLength > webBodyLimit {
		return webDocument{}, webUnavailable("web response exceeds the size limit")
	}
	if encoding := response.Header.Get("Content-Encoding"); encoding != "" && encoding != "identity" {
		return webDocument{}, webUnavailable("web response uses an unsupported content encoding")
	}
	declared := response.Header.Get("Content-Type")
	if declared == "" {
		declared = "text/plain"
	}
	contentType, _, _ := strings.Cut(declared, ";")
	contentType = strings.ToLower(strings.TrimSpace(contentType))
	isHTML := contentType == "text/html" || contentType == "application/xhtml+xml"
	isPDF := contentType == "application/pdf"
	if !isHTML && !isPDF && !strings.HasPrefix(contentType, "text/") && contentType != "application/json" && !strings.HasSuffix(contentType, "+json") && contentType != "application/xml" && !strings.HasSuffix(contentType, "+xml") {
		return webDocument{}, webUnavailable("unsupported web content type: " + contentType)
	}
	body, err := io.ReadAll(io.LimitReader(response.Body, webBodyLimit+1))
	if err != nil {
		return webDocument{}, webUnavailable("web response was interrupted")
	}
	if len(body) > webBodyLimit {
		return webDocument{}, webUnavailable("web response exceeds the size limit")
	}
	var title *string
	var content string
	if isPDF {
		parts, err := office.Parts("page.pdf", body)
		if err != nil {
			return webDocument{}, err
		}
		sections := make([]string, 0, len(parts))
		for _, part := range parts {
			sections = append(sections, "## "+part.Name+"\n\n"+part.Text)
		}
		content = strings.Join(sections, "\n\n")
	} else {
		content = decodeWebBody(body, declared, isHTML)
		if isHTML {
			title, content, err = convertHTML(content)
			if err != nil {
				return webDocument{}, err
			}
		}
	}
	if len(content) > webBodyLimit {
		return webDocument{}, webUnavailable("web content exceeds the size limit")
	}
	return webDocument{url: parsed.String(), status: uint16(response.StatusCode), contentType: contentType, content: content, title: title, fetched: fetcher.options.Now()}, nil
}
