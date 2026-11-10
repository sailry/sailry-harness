package web

import (
	"bytes"
	"context"
	"encoding/json"
	"io"
	"math"
	"net"
	"net/http"
	"net/url"
	"strconv"
	"strings"
	"time"
	"unicode/utf8"

	"github.com/sailry/sailry-harness/internal/protocol"
)

const searchBodyLimit = 1024 * 1024

func (searcher *Searcher) endpoint(driver protocol.SearchDriver, settings protocol.SearchSettings) string {
	if endpoint := searcher.options.Endpoints[driver]; endpoint != "" {
		return endpoint
	}
	switch driver {
	case protocol.SearchDriverExa:
		return "https://api.exa.ai/search"
	case protocol.SearchDriverTavily:
		return "https://api.tavily.com/search"
	case protocol.SearchDriverBrave:
		return "https://api.search.brave.com/res/v1/web/search"
	case protocol.SearchDriverSearxng:
		if endpoint := settings.Providers[driver].Endpoint; endpoint != nil {
			return *endpoint
		}
	case protocol.SearchDriverBocha:
		return "https://api.bochaai.com/v1/web-search"
	case protocol.SearchDriverTinyfish:
		return "https://api.search.tinyfish.ai"
	case protocol.SearchDriverVolcengine:
		return "https://open.feedcoopapi.com/search_api/web_search"
	case protocol.SearchDriverDeepseek:
		return "https://api.deepseek.com/v1/tools/search"
	}
	return ""
}

func (searcher *Searcher) searchHTTP(ctx context.Context, driver protocol.SearchDriver, plan SearchPlan, input protocol.SearchRequest) ([]protocol.SearchResult, error) {
	request, err := buildSearchRequest(ctx, driver, searcher.endpoint(driver, plan.Settings), plan.Credentials[driver], input, searcher.options.Now())
	if err != nil {
		return nil, err
	}
	value, err := searcher.requestJSON(request, searchBodyLimit)
	if err != nil {
		return nil, err
	}
	return parseDriverResults(driver, value, searcher.options.Now())
}

func buildSearchRequest(ctx context.Context, driver protocol.SearchDriver, endpoint string, auth Auth, input protocol.SearchRequest, now time.Time) (*http.Request, error) {
	method := http.MethodPost
	parameters := url.Values{}
	body := map[string]any{}
	headers := http.Header{}
	key := func() (string, error) {
		if auth.Key == nil {
			return "", searchUnauthorized()
		}
		return auth.Key.Expose(), nil
	}
	freshness := func(day, week, month, year string) string {
		if input.Freshness == nil {
			return ""
		}
		switch *input.Freshness {
		case protocol.SearchFreshnessHour, protocol.SearchFreshnessDay:
			return day
		case protocol.SearchFreshnessWeek:
			return week
		case protocol.SearchFreshnessMonth:
			return month
		case protocol.SearchFreshnessYear:
			return year
		}
		return ""
	}
	if driver != protocol.SearchDriverSearxng {
		secret, err := key()
		if err != nil {
			return nil, err
		}
		switch driver {
		case protocol.SearchDriverBrave:
			headers.Set("X-Subscription-Token", secret)
		case protocol.SearchDriverTinyfish:
			headers.Set("X-API-Key", secret)
		case protocol.SearchDriverExa:
			headers.Set("x-api-key", secret)
		default:
			headers.Set("Authorization", "Bearer "+secret)
		}
	}
	switch driver {
	case protocol.SearchDriverBrave:
		fits := func(query string) bool {
			return utf8.RuneCountInString(query) <= 600 && len(strings.Fields(query)) <= 75
		}
		if !fits(input.Query) {
			return nil, searchServiceLimit()
		}
		query := filteredSearchQuery(input)
		if !fits(query) {
			query = input.Query
		}
		method = http.MethodGet
		parameters.Set("q", query)
		parameters.Set("count", strconv.Itoa(int(input.Limit)))
		if input.Freshness != nil {
			parameters.Set("freshness", freshness("pd", "pw", "pm", "py"))
		}
	case protocol.SearchDriverTavily:
		body = map[string]any{"query": input.Query, "max_results": input.Limit, "search_depth": "basic", "include_answer": false, "include_raw_content": false, "include_published_date": true}
		if input.Freshness != nil {
			body["time_range"] = freshness("day", "week", "month", "year")
		}
		if len(input.IncludeDomains) > 0 {
			body["include_domains"] = input.IncludeDomains
		}
		if len(input.ExcludeDomains) > 0 {
			body["exclude_domains"] = input.ExcludeDomains
		}
	case protocol.SearchDriverSearxng:
		parsed, err := url.Parse(endpoint)
		if err != nil || parsed.Hostname() == "" {
			return nil, searchInvalid("SearXNG endpoint is invalid")
		}
		if strings.HasSuffix(parsed.Path, "/") {
			parsed = parsed.ResolveReference(&url.URL{Path: "search"})
		}
		endpoint = parsed.String()
		method = http.MethodGet
		parameters.Set("q", filteredSearchQuery(input))
		parameters.Set("format", "json")
		if input.Freshness != nil {
			period := freshness("day", "month", "month", "year")
			parameters.Set("time_range", period)
		}
		if auth.Key != nil {
			headers.Set("X-API-Key", auth.Key.Expose())
		}
	case protocol.SearchDriverExa:
		body = map[string]any{"query": input.Query, "numResults": input.Limit, "type": "auto", "contents": map[string]any{"text": map[string]any{"maxCharacters": 4000}}}
		if input.Freshness != nil {
			body["startPublishedDate"] = searchSince(*input.Freshness, now).UTC().Format("2006-01-02T15:04:05.999999999+00:00")
		}
		if len(input.IncludeDomains) > 0 {
			body["includeDomains"] = input.IncludeDomains
		}
		if len(input.ExcludeDomains) > 0 {
			body["excludeDomains"] = input.ExcludeDomains
		}
	case protocol.SearchDriverBocha:
		body = map[string]any{"query": input.Query, "count": input.Limit, "summary": false}
		if input.Freshness != nil {
			body["freshness"] = freshness("oneDay", "oneWeek", "oneMonth", "oneYear")
		}
		if len(input.IncludeDomains) > 0 {
			body["include"] = strings.Join(input.IncludeDomains, "|")
		}
		if len(input.ExcludeDomains) > 0 {
			body["exclude"] = strings.Join(input.ExcludeDomains, "|")
		}
	case protocol.SearchDriverTinyfish:
		method = http.MethodGet
		parameters.Set("query", input.Query)
		if len(input.IncludeDomains) > 0 {
			parameters.Set("include_domains", strings.Join(input.IncludeDomains, ","))
		}
		if len(input.ExcludeDomains) > 0 {
			parameters.Set("exclude_domains", strings.Join(input.ExcludeDomains, ","))
		}
		if input.Freshness != nil {
			parameters.Set("recency_minutes", strconv.FormatInt(int64(now.Sub(searchSince(*input.Freshness, now))/time.Minute), 10))
		}
	case protocol.SearchDriverVolcengine:
		if utf8.RuneCountInString(input.Query) > 100 {
			return nil, searchServiceLimit()
		}
		filter := map[string]any{"NeedUrl": true}
		if len(input.IncludeDomains) > 0 {
			filter["Sites"] = strings.Join(input.IncludeDomains, "|")
		}
		if len(input.ExcludeDomains) > 0 {
			filter["BlockHosts"] = strings.Join(input.ExcludeDomains[:min(len(input.ExcludeDomains), 5)], "|")
		}
		body = map[string]any{"Query": input.Query, "SearchType": "web", "Count": min(input.Limit, 50), "Filter": filter}
		if input.Freshness != nil {
			body["TimeRange"] = freshness("OneDay", "OneWeek", "OneMonth", "OneYear")
		}
	case protocol.SearchDriverDeepseek:
		if utf8.RuneCountInString(input.Query) > 128 {
			return nil, searchServiceLimit()
		}
		query := filteredSearchQuery(input)
		if utf8.RuneCountInString(query) > 128 {
			query = input.Query
		}
		if input.Freshness != nil {
			filtered := query + " after:" + searchSince(*input.Freshness, now).UTC().Format("2006-01-02")
			if utf8.RuneCountInString(filtered) <= 128 {
				query = filtered
			}
		}
		body = map[string]any{"query": query, "count": min(input.Limit, 50), "need_content": false}
	default:
		return nil, searchInvalid("invalid search driver")
	}
	var reader io.Reader
	if method == http.MethodPost {
		encoded, err := json.Marshal(body)
		if err != nil {
			return nil, searchInvalid("search request is invalid")
		}
		reader = bytes.NewReader(encoded)
		headers.Set("Content-Type", "application/json")
	}
	parsed, err := url.Parse(endpoint)
	if err != nil || parsed.Hostname() == "" {
		return nil, searchInvalid("search request is invalid")
	}
	if len(parameters) > 0 {
		query := parsed.Query()
		for name, values := range parameters {
			for _, value := range values {
				query.Add(name, value)
			}
		}
		parsed.RawQuery = query.Encode()
	}
	request, err := http.NewRequestWithContext(ctx, method, parsed.String(), reader)
	if err != nil {
		return nil, searchInvalid("search request is invalid")
	}
	request.Header = headers
	request.Header.Set("User-Agent", "Sailry/1")
	if driver == protocol.SearchDriverSearxng && auth.Basic != nil {
		user, password, ok := strings.Cut(auth.Basic.Expose(), ":")
		if !ok {
			return nil, searchUnauthorized()
		}
		request.SetBasicAuth(user, password)
	}
	return request, nil
}

func (searcher *Searcher) requestJSON(request *http.Request, limit int) (any, error) {
	client, closeIdle := searchClient(searcher.options.Transport)
	defer closeIdle()
	response, err := client.Do(request)
	if err != nil {
		return nil, searchUnavailable("search service is unreachable")
	}
	defer response.Body.Close()
	if failure := searchStatus(response.StatusCode, response.Header.Get("Retry-After"), searcher.options.Now()); failure != nil {
		return nil, failure
	}
	if response.ContentLength > int64(limit) {
		return nil, searchInvalid("search service response is too large")
	}
	value, err := io.ReadAll(io.LimitReader(response.Body, int64(limit)+1))
	if err != nil {
		return nil, searchUnavailable("search response was interrupted")
	}
	if len(value) > limit {
		return nil, searchInvalid("search service response is too large")
	}
	var decoded any
	decoder := json.NewDecoder(bytes.NewReader(value))
	decoder.UseNumber()
	if err := decoder.Decode(&decoded); err != nil {
		return nil, searchInvalid("search service returned invalid JSON")
	}
	if err := decoder.Decode(new(any)); err != io.EOF {
		return nil, searchInvalid("search service returned invalid JSON")
	}
	return decoded, nil
}

func searchClient(transport http.RoundTripper) (*http.Client, func()) {
	closeIdle := func() {}
	if transport == nil {
		native := &http.Transport{Proxy: systemProxy, DialContext: (&net.Dialer{Timeout: 5 * time.Second}).DialContext, TLSHandshakeTimeout: 5 * time.Second, ForceAttemptHTTP2: true}
		transport = native
		closeIdle = native.CloseIdleConnections
	}
	return &http.Client{Transport: transport, Timeout: 18 * time.Second, CheckRedirect: func(*http.Request, []*http.Request) error { return http.ErrUseLastResponse }}, closeIdle
}

func searchStatus(status int, retry string, now time.Time) *searchFailure {
	if status == 429 {
		deadline := retryDeadline(retry, now)
		return &searchFailure{fault: coolingDown(deadline), temporary: true, retryAt: &deadline}
	}
	if status == 401 || status == 403 {
		return searchUnauthorized()
	}
	if status >= 500 {
		return searchUnavailable("search service is unavailable")
	}
	if status < 200 || status >= 300 {
		return searchInvalid("search service rejected the request")
	}
	return nil
}
func retryDeadline(value string, now time.Time) time.Time {
	value = strings.TrimSpace(value)
	if seconds, err := strconv.ParseUint(value, 10, 64); err == nil && seconds <= math.MaxInt64/uint64(time.Second) {
		return now.Add(time.Duration(seconds) * time.Second)
	}
	if deadline, err := http.ParseTime(value); err == nil && deadline.Unix() >= 0 {
		if deadline.Before(now) {
			return now
		}
		return deadline
	}
	return now.Add(time.Minute)
}

func jsonAt(value any, path ...string) any {
	for _, name := range path {
		object, ok := value.(map[string]any)
		if !ok {
			return nil
		}
		value = object[name]
	}
	return value
}
func jsonString(value any) string { text, _ := value.(string); return text }
func jsonUint(value any) (uint64, bool) {
	switch value := value.(type) {
	case json.Number:
		number, err := strconv.ParseUint(string(value), 10, 64)
		return number, err == nil
	case float64:
		if value >= 0 && value < math.Exp2(64) && value == math.Trunc(value) {
			return uint64(value), true
		}
	case uint64:
		return value, true
	case int:
		if value >= 0 {
			return uint64(value), true
		}
	}
	return 0, false
}
func searchText(value string, limit int) string {
	end, count := 0, 0
	for end < len(value) && count < limit {
		_, size := utf8.DecodeRuneInString(value[end:])
		end += size
		count++
	}
	return value[:end]
}

func parseSearchResults(value any, path []string, title, snippet, date string) ([]protocol.SearchResult, error) {
	entries, ok := jsonAt(value, path...).([]any)
	if !ok {
		return nil, searchInvalid("search service response has no result list")
	}
	results := make([]protocol.SearchResult, 0, len(entries))
	for _, entry := range entries {
		text, ok := jsonAt(entry, "url").(string)
		if !ok {
			return nil, searchInvalid("search result URL is missing")
		}
		parsed, err := url.Parse(text)
		if err != nil || parsed.Hostname() == "" || (parsed.Scheme != "http" && parsed.Scheme != "https") || parsed.User != nil && parsed.User.Username() != "" {
			return nil, searchInvalid("search result URL is invalid")
		}
		if parsed.User != nil {
			if _, ok := parsed.User.Password(); ok {
				return nil, searchInvalid("search result URL is invalid")
			}
		}
		name, ok := jsonAt(entry, title).(string)
		if !ok {
			name = text
		}
		result := protocol.SearchResult{Title: searchText(name, 512), Url: text, Snippet: searchText(jsonString(jsonAt(entry, snippet)), 4000)}
		if published := jsonString(jsonAt(entry, date)); published != "" {
			value := searchText(published, 128)
			result.Published = &value
		}
		results = append(results, result)
	}
	return results, nil
}

func parseDriverResults(driver protocol.SearchDriver, value any, now time.Time) ([]protocol.SearchResult, error) {
	switch driver {
	case protocol.SearchDriverBrave:
		return parseSearchResults(value, []string{"web", "results"}, "title", "description", "page_age")
	case protocol.SearchDriverTavily:
		return parseSearchResults(value, []string{"results"}, "title", "content", "published_date")
	case protocol.SearchDriverSearxng:
		return parseSearchResults(value, []string{"results"}, "title", "content", "publishedDate")
	case protocol.SearchDriverExa:
		return parseSearchResults(value, []string{"results"}, "title", "text", "publishedDate")
	case protocol.SearchDriverTinyfish, protocol.SearchDriverDeepseek:
		return parseSearchResults(value, []string{"results"}, "title", "snippet", "date")
	case protocol.SearchDriverBocha:
		if code, ok := jsonUint(jsonAt(value, "code")); ok && code != 200 {
			if code == 429 {
				return nil, searchStatus(429, "", now)
			}
			if code == 401 || code == 403 {
				return nil, searchUnauthorized()
			}
			if code >= 500 {
				return nil, searchUnavailable("search service is unavailable")
			}
			return nil, searchInvalid("search service rejected the request")
		}
		return parseSearchResults(value, []string{"data", "webPages", "value"}, "name", "snippet", "datePublished")
	case protocol.SearchDriverVolcengine:
		if metadata := jsonAt(value, "ResponseMetadata", "Error"); metadata != nil {
			code, ok := jsonUint(jsonAt(metadata, "CodeN"))
			if !ok {
				code, _ = strconv.ParseUint(jsonString(jsonAt(metadata, "Code")), 10, 64)
			}
			switch code {
			case 10401, 10403:
				return nil, searchUnauthorized()
			case 700429:
				return nil, searchStatus(429, "", now)
			case 10500:
				return nil, searchUnavailable("search service is unavailable")
			default:
				return nil, searchInvalid("search service rejected the request")
			}
		}
		entries, ok := jsonAt(value, "Result", "WebResults").([]any)
		if !ok {
			if count, ok := jsonUint(jsonAt(value, "Result", "ResultCount")); ok && count == 0 {
				return []protocol.SearchResult{}, nil
			}
			return nil, searchInvalid("search service response has no result list")
		}
		normalized := make([]any, 0, len(entries))
		for _, entry := range entries {
			snippet := jsonString(jsonAt(entry, "Summary"))
			if snippet == "" {
				snippet = jsonString(jsonAt(entry, "Snippet"))
			}
			normalized = append(normalized, map[string]any{"url": jsonAt(entry, "Url"), "title": jsonAt(entry, "Title"), "snippet": snippet, "published": jsonAt(entry, "PublishTime")})
		}
		return parseSearchResults(map[string]any{"results": normalized}, []string{"results"}, "title", "snippet", "published")
	}
	return nil, searchInvalid("invalid search driver")
}
