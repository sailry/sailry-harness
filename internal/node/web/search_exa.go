package web

import (
	"context"
	"errors"
	"io"
	"net/http"
	"strings"
	"sync"
	"time"

	"github.com/modelcontextprotocol/go-sdk/jsonrpc"
	"github.com/modelcontextprotocol/go-sdk/mcp"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func (searcher *Searcher) searchExa(ctx context.Context, auth Auth, input protocol.SearchRequest) (protocol.SearchPage, error) {
	endpoint := searcher.endpoint(protocol.SearchDriverExa, protocol.SearchSettings{})
	if auth.Key != nil {
		ctx, cancel := context.WithTimeout(ctx, 20*time.Second)
		defer cancel()
		request, err := buildSearchRequest(ctx, protocol.SearchDriverExa, endpoint, auth, input, searcher.options.Now())
		if err != nil {
			return protocol.SearchPage{}, err
		}
		value, err := searcher.requestJSON(request, searchBodyLimit)
		if err != nil {
			if errors.Is(ctx.Err(), context.DeadlineExceeded) {
				err = searchUnavailable("search service deadline exceeded")
			}
			return protocol.SearchPage{}, err
		}
		results, err := parseDriverResults(protocol.SearchDriverExa, value, searcher.options.Now())
		return protocol.SearchPage{Results: results}, err
	}
	if endpoint == "https://api.exa.ai/search" {
		endpoint = "https://mcp.exa.ai/mcp"
	}
	return searcher.exaHosted(ctx, endpoint, input)
}

type exaResult struct {
	response *mcp.CallToolResult
	err      error
	cleaned  bool
}

func (searcher *Searcher) exaHosted(ctx context.Context, endpoint string, input protocol.SearchRequest) (protocol.SearchPage, error) {
	operation, cancel := context.WithTimeout(ctx, 20*time.Second)
	defer cancel()
	client, closeIdle := searchClient(searcher.options.Transport)
	defer closeIdle()
	tracker := &searchTracker{transport: client.Transport, now: searcher.options.Now, changed: make(chan struct{})}
	client.Transport = tracker
	transport := &mcp.StreamableClientTransport{Endpoint: endpoint, HTTPClient: client, MaxRetries: -1, MaxEventSize: searchBodyLimit}
	owner := mcp.NewClient(&mcp.Implementation{Name: "Sailry", Version: "1"}, &mcp.ClientOptions{MultiRoundTrip: &mcp.MultiRoundTripOptions{Disabled: true}})
	finished := make(chan exaResult, 1)
	go func() {
		session, err := owner.Connect(operation, transport, nil)
		if err != nil {
			finished <- exaResult{err: err, cleaned: true}
			return
		}
		response, err := session.CallTool(operation, &mcp.CallToolParams{Name: "web_search_exa", Arguments: map[string]any{"query": filteredSearchQuery(input), "numResults": input.Limit}})
		cleaned := session.Close() == nil
		finished <- exaResult{response: response, err: err, cleaned: cleaned}
	}()
	var result exaResult
	ready := false
	select {
	case result = <-finished:
		ready = true
	case <-operation.Done():
	}
	// The SDK owns protocol shutdown; retained HTTP bodies prove its native
	// transport has also released every in-flight request before fallback.
	cleanupDeadline := time.NewTimer(7 * time.Second)
	defer cleanupDeadline.Stop()
	for {
		idle, changed := tracker.idle()
		if ready && idle {
			break
		}
		select {
		case result = <-finished:
			ready = true
		case <-changed:
		case <-cleanupDeadline.C:
			failure := &searchFailure{fault: &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "search service cleanup could not be confirmed"}, uncertain: true}
			return protocol.SearchPage{}, failure
		}
	}
	if !result.cleaned {
		return protocol.SearchPage{}, &searchFailure{fault: &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "search service cleanup could not be confirmed"}, uncertain: true}
	}
	if ctx.Err() != nil {
		return protocol.SearchPage{}, searchCancelled()
	}
	if errors.Is(operation.Err(), context.DeadlineExceeded) {
		return protocol.SearchPage{}, searchUnavailable("search service deadline exceeded")
	}
	if result.err != nil {
		if failure := tracker.failure(); failure != nil {
			return protocol.SearchPage{}, failure
		}
		var rpc *jsonrpc.Error
		if errors.As(result.err, &rpc) {
			return protocol.SearchPage{}, searchInvalid("search service returned an invalid response")
		}
		return protocol.SearchPage{}, searchUnavailable("search service is unreachable")
	}
	if result.response == nil {
		return protocol.SearchPage{}, searchInvalid("search service did not return a final result")
	}
	if result.response.IsError {
		return protocol.SearchPage{}, searchInvalid("search service rejected the query")
	}
	if result.response.NeedsInput() {
		return protocol.SearchPage{}, searchInvalid("search service did not return a final result")
	}
	var text strings.Builder
	for _, block := range result.response.Content {
		if part, ok := block.(*mcp.TextContent); ok {
			if len(part.Text) > searchBodyLimit-text.Len() {
				return protocol.SearchPage{}, searchInvalid("search service response is too large")
			}
			if text.Len() > 0 {
				text.WriteByte('\n')
			}
			text.WriteString(part.Text)
		}
	}
	results, err := formattedExa(text.String())
	return protocol.SearchPage{Results: results}, err
}

func formattedExa(text string) ([]protocol.SearchResult, error) {
	if strings.TrimSpace(text) == "No search results found. Please try a different query." {
		return []protocol.SearchResult{}, nil
	}
	entries := []any{}
	for _, entry := range strings.Split(text, "\n\n---\n\n") {
		var title, url, published *string
		var snippet strings.Builder
		content := false
		for _, line := range strings.Split(strings.TrimSuffix(entry, "\n"), "\n") {
			if content {
				if snippet.Len() > 0 {
					snippet.WriteByte('\n')
				}
				snippet.WriteString(line)
				continue
			}
			if value, ok := strings.CutPrefix(line, "Title: "); ok {
				title = &value
			} else if value, ok := strings.CutPrefix(line, "URL: "); ok {
				url = &value
			} else if value, ok := strings.CutPrefix(line, "Published: "); ok {
				if value != "N/A" {
					published = &value
				}
			} else if line == "Highlights:" {
				content = true
			} else if value, ok := strings.CutPrefix(line, "Text: "); ok {
				content = true
				snippet.WriteString(value)
			}
		}
		if title == nil || url == nil {
			return nil, searchInvalid("search service returned an invalid result")
		}
		value := map[string]any{"title": *title, "url": *url, "text": snippet.String()}
		if published != nil {
			value["publishedDate"] = *published
		}
		entries = append(entries, value)
	}
	return parseSearchResults(map[string]any{"results": entries}, []string{"results"}, "title", "text", "publishedDate")
}

type searchTracker struct {
	transport   http.RoundTripper
	now         func() time.Time
	mu          sync.Mutex
	active      int
	changed     chan struct{}
	lastFailure *searchFailure
}

func (tracker *searchTracker) RoundTrip(request *http.Request) (*http.Response, error) {
	tracker.mu.Lock()
	tracker.active++
	tracker.mu.Unlock()
	request = request.Clone(request.Context())
	request.Header.Set("User-Agent", "Sailry/1")
	response, err := tracker.transport.RoundTrip(request)
	if err != nil {
		tracker.finish()
		return nil, err
	}
	if request.Method == http.MethodPost {
		tracker.mu.Lock()
		tracker.lastFailure = searchStatus(response.StatusCode, response.Header.Get("Retry-After"), tracker.now())
		tracker.mu.Unlock()
	}
	response.Body = &trackedSearchBody{ReadCloser: response.Body, finish: tracker.finish}
	return response, nil
}
func (tracker *searchTracker) finish() {
	tracker.mu.Lock()
	defer tracker.mu.Unlock()
	tracker.active--
	close(tracker.changed)
	tracker.changed = make(chan struct{})
}
func (tracker *searchTracker) idle() (bool, <-chan struct{}) {
	tracker.mu.Lock()
	defer tracker.mu.Unlock()
	return tracker.active == 0, tracker.changed
}
func (tracker *searchTracker) failure() *searchFailure {
	tracker.mu.Lock()
	defer tracker.mu.Unlock()
	return tracker.lastFailure
}

type trackedSearchBody struct {
	io.ReadCloser
	once   sync.Once
	finish func()
}

func (body *trackedSearchBody) Close() error {
	err := body.ReadCloser.Close()
	body.once.Do(body.finish)
	return err
}
func (body *trackedSearchBody) Read(value []byte) (int, error) {
	count, err := body.ReadCloser.Read(value)
	if err != nil {
		_ = body.Close()
	}
	return count, err
}
