package web

import (
	"context"
	"encoding/json"
	"io"
	"net/http"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func searchSettings(drivers ...protocol.SearchDriver) protocol.SearchSettings {
	settings := protocol.DefaultSearchSettings()
	settings.Revision = 1
	settings.Order = drivers
	for driver, config := range settings.Providers {
		config.Enabled = false
		settings.Providers[driver] = config
	}
	for _, driver := range drivers {
		config := settings.Providers[driver]
		config.Enabled, config.KeyConfigured = true, true
		if driver == protocol.SearchDriverModel {
			config.Model = &protocol.SearchModelConfig{Model: "fixture"}
		}
		settings.Providers[driver] = config
	}
	return settings
}
func searchAuth() Auth { key := protocol.NewSecret("fixture-secret"); return Auth{Key: &key} }

func TestSearchChannelOrder(t *testing.T) {
	t.Parallel()
	for _, automatic := range []bool{true, false} {
		t.Run(map[bool]string{true: "automatic advances past permanent failure", false: "selected channel only falls back temporarily"}[automatic], func(t *testing.T) {
			settings := searchSettings(protocol.SearchDriverBrave, protocol.SearchDriverTavily)
			if !automatic {
				settings.Driver = &protocol.SearchSelection{Kind: "driver", Data: protocol.SearchDriverBrave}
			}
			called := []string{}
			searcher := NewSearcher(SearchOptions{Endpoints: map[protocol.SearchDriver]string{protocol.SearchDriverBrave: "https://brave.test/", protocol.SearchDriverTavily: "https://tavily.test/"}, Transport: fixtureTransport(func(request *http.Request) (*http.Response, error) {
				called = append(called, request.URL.Host)
				if request.URL.Host == "brave.test" {
					response := responseFixture(`{}`, "application/json")
					response.StatusCode = 400
					return response, nil
				}
				return responseFixture(`{"results":[{"url":"https://example.com/","title":"Source","content":"Excerpt"}]}`, "application/json"), nil
			})})
			searcher.Configure(settings)
			page, err := searcher.Search(context.Background(), SearchPlan{Settings: settings, Credentials: map[protocol.SearchDriver]Auth{protocol.SearchDriverBrave: searchAuth(), protocol.SearchDriverTavily: searchAuth()}}, protocol.SearchRequest{Query: "fixture", Limit: 5})
			if automatic {
				if err != nil || page.Driver != protocol.SearchDriverTavily || len(called) != 2 {
					t.Fatal(page, err, called)
				}
			} else if err == nil || len(called) != 1 {
				t.Fatal("selected permanent failure fell back", page, err, called)
			}
		})
	}
	t.Run("cooldown cache and captured revision", func(t *testing.T) {
		now := time.Unix(100, 0)
		settings := searchSettings(protocol.SearchDriverBrave, protocol.SearchDriverTavily)
		brave, tavily := 0, 0
		searcher := NewSearcher(SearchOptions{Now: func() time.Time { return now }, Endpoints: map[protocol.SearchDriver]string{protocol.SearchDriverBrave: "https://brave.test/", protocol.SearchDriverTavily: "https://tavily.test/"}, Transport: fixtureTransport(func(request *http.Request) (*http.Response, error) {
			if request.URL.Host == "brave.test" {
				brave++
				response := responseFixture(`{}`, "application/json")
				response.StatusCode = 429
				response.Header.Set("Retry-After", "60")
				return response, nil
			}
			tavily++
			return responseFixture(`{"results":[]}`, "application/json"), nil
		})})
		searcher.Configure(settings)
		plan := SearchPlan{Settings: settings, Credentials: map[protocol.SearchDriver]Auth{protocol.SearchDriverBrave: searchAuth(), protocol.SearchDriverTavily: searchAuth()}}
		input := protocol.SearchRequest{Query: " fixture ", Limit: 5}
		if _, err := searcher.Search(context.Background(), plan, input); err != nil {
			t.Fatal(err)
		}
		if _, err := searcher.Search(context.Background(), plan, input); err != nil {
			t.Fatal(err)
		}
		if brave != 1 || tavily != 1 {
			t.Fatal("normalized cache missed", brave, tavily)
		}
		input.Query = "second"
		if _, err := searcher.Search(context.Background(), plan, input); err != nil {
			t.Fatal(err)
		}
		if brave != 1 || tavily != 2 {
			t.Fatal("cooling channel was retried", brave, tavily)
		}
		settings.Revision++
		searcher.Configure(settings)
		plan.Settings = settings
		if _, err := searcher.SearchFresh(context.Background(), plan, input, true); err != nil {
			t.Fatal(err)
		}
		if brave != 2 || tavily != 3 {
			t.Fatal("new settings did not clear cooldown", brave, tavily)
		}
		now = now.Add(61 * time.Second)
		if _, err := searcher.SearchFresh(context.Background(), plan, input, true); err != nil {
			t.Fatal(err)
		}
		if brave != 3 || tavily != 4 {
			t.Fatal("cooldown did not expire", brave, tavily)
		}
	})
	t.Run("model selection owns frozen callback", func(t *testing.T) {
		settings := searchSettings(protocol.SearchDriverModel, protocol.SearchDriverBrave)
		settings.Driver = &protocol.SearchSelection{Kind: "driver", Data: protocol.SearchDriverModel}
		searcher := NewSearcher(SearchOptions{Transport: fixtureTransport(func(*http.Request) (*http.Response, error) {
			t.Error("model search started a REST channel")
			return responseFixture(`{}`, "application/json"), nil
		})})
		searcher.Configure(settings)
		calls := 0
		plan := SearchPlan{Settings: settings, Model: func(_ context.Context, input protocol.SearchRequest) (protocol.SearchPage, error) {
			calls++
			summary := "Captured model summary"
			return protocol.SearchPage{Summary: &summary, Results: []protocol.SearchResult{{Title: "Source", Url: "https://example.com/", Snippet: input.Query}}}, nil
		}}
		page, err := searcher.Search(context.Background(), plan, protocol.SearchRequest{Query: "fixture", Limit: 5})
		if err != nil || calls != 1 || page.Driver != protocol.SearchDriverModel || page.Summary == nil || !page.Untrusted {
			t.Fatal(page, err, calls)
		}
	})
}

func TestSearchDriverContracts(t *testing.T) {
	t.Parallel()
	now := time.Date(2026, 10, 30, 12, 0, 0, 0, time.UTC)
	freshness := protocol.SearchFreshnessHour
	input := protocol.SearchRequest{Query: "official release", Limit: 20, Freshness: &freshness, IncludeDomains: []string{"example.com"}, ExcludeDomains: []string{"private.example.com"}}
	for _, driver := range []protocol.SearchDriver{protocol.SearchDriverBrave, protocol.SearchDriverTavily, protocol.SearchDriverSearxng, protocol.SearchDriverExa, protocol.SearchDriverBocha, protocol.SearchDriverTinyfish, protocol.SearchDriverVolcengine, protocol.SearchDriverDeepseek} {
		t.Run(string(driver), func(t *testing.T) {
			request, err := buildSearchRequest(context.Background(), driver, "https://service.test/", searchAuth(), input, now)
			if err != nil {
				t.Fatal(err)
			}
			if request.Header.Get("User-Agent") != "Sailry/1" {
				t.Fatal("user agent changed")
			}
			var body map[string]any
			if request.Body != nil {
				data, err := io.ReadAll(request.Body)
				if err != nil {
					t.Fatal(err)
				}
				if err = json.Unmarshal(data, &body); err != nil {
					t.Fatal(err)
				}
			}
			switch driver {
			case protocol.SearchDriverBrave:
				if request.Method != "GET" || request.URL.Query().Get("freshness") != "pd" || !strings.Contains(request.URL.Query().Get("q"), "site:example.com") || request.Header.Get("X-Subscription-Token") == "" {
					t.Fatal(request)
				}
			case protocol.SearchDriverTavily:
				if body["include_answer"] != false || body["include_raw_content"] != false || body["include_published_date"] != true || body["time_range"] != "day" {
					t.Fatal(body)
				}
			case protocol.SearchDriverSearxng:
				if request.URL.Path != "/search" || request.URL.Query().Get("format") != "json" || request.URL.Query().Get("time_range") != "day" {
					t.Fatal(request.URL)
				}
			case protocol.SearchDriverExa:
				if body["type"] != "auto" || body["startPublishedDate"] != "2026-10-30T11:00:00+00:00" || jsonAt(body, "contents", "text", "maxCharacters") != float64(4000) {
					t.Fatal(body)
				}
			case protocol.SearchDriverBocha:
				if body["summary"] != false || body["freshness"] != "oneDay" || body["include"] != "example.com" {
					t.Fatal(body)
				}
			case protocol.SearchDriverTinyfish:
				if request.URL.Query().Get("recency_minutes") != "60" || request.URL.Query().Has("limit") || request.URL.Query().Get("query") != input.Query {
					t.Fatal(request.URL)
				}
			case protocol.SearchDriverVolcengine:
				if body["TimeRange"] != "OneDay" || jsonAt(body, "Filter", "NeedUrl") != true || jsonAt(body, "Filter", "BlockHosts") != "private.example.com" {
					t.Fatal(body)
				}
			case protocol.SearchDriverDeepseek:
				if body["need_content"] != false || !strings.HasSuffix(jsonString(body["query"]), " after:2026-10-30") {
					t.Fatal(body)
				}
			}
		})
	}
	for _, driver := range []protocol.SearchDriver{protocol.SearchDriverBrave, protocol.SearchDriverVolcengine, protocol.SearchDriverDeepseek} {
		long := input
		long.Query = strings.Repeat("x", 601)
		if _, err := buildSearchRequest(context.Background(), driver, "https://service.test/", searchAuth(), long, now); err == nil {
			t.Fatal("service query limit ignored", driver)
		}
	}
	results, err := parseDriverResults(protocol.SearchDriverVolcengine, map[string]any{"Result": map[string]any{"WebResults": []any{map[string]any{"Url": "https://example.com/", "Title": "Source", "Summary": "Relevant", "Snippet": "Short", "PublishTime": "2026-10-30"}}}}, now)
	if err != nil || len(results) != 1 || results[0].Snippet != "Relevant" {
		t.Fatal(results, err)
	}
	if _, err := normalizeSearch(protocol.SearchRequest{Query: "query", Limit: 5, IncludeDomains: []string{"EXAMPLE.COM."}, ExcludeDomains: []string{"example.com"}}); err == nil {
		t.Fatal("normalized domain overlap ignored")
	}
	filtered := filterSearchResults([]protocol.SearchResult{{Url: "https://example.com/"}, {Url: "https://private.example.com/"}, {Url: "https://example.com.evil/"}}, input, now)
	if len(filtered) != 1 {
		t.Fatal(filtered)
	}
}

func TestHostedExaLifecycle(t *testing.T) {
	t.Parallel()
	var mu sync.Mutex
	calls := 0
	transport := fixtureTransport(func(request *http.Request) (*http.Response, error) {
		if request.Method != http.MethodPost {
			response := responseFixture("", "application/json")
			response.StatusCode = 405
			return response, nil
		}
		var frame struct {
			ID     json.RawMessage `json:"id"`
			Method string          `json:"method"`
			Params map[string]any  `json:"params"`
		}
		if err := json.NewDecoder(request.Body).Decode(&frame); err != nil {
			return nil, err
		}
		var result any
		switch frame.Method {
		case "server/discover":
			data, _ := json.Marshal(map[string]any{"jsonrpc": "2.0", "id": frame.ID, "error": map[string]any{"code": -32601, "message": "Method not found"}})
			return responseFixture(string(data), "application/json"), nil
		case "initialize":
			result = map[string]any{"protocolVersion": "2025-11-25", "capabilities": map[string]any{"tools": map[string]any{}}, "serverInfo": map[string]any{"name": "Fixture", "version": "1"}}
		case "notifications/initialized":
			response := responseFixture("", "application/json")
			response.StatusCode = 202
			return response, nil
		case "tools/call":
			mu.Lock()
			calls++
			mu.Unlock()
			if frame.Params["name"] != "web_search_exa" {
				t.Error("incorrect hosted tool", frame.Params)
			}
			result = map[string]any{"content": []any{map[string]any{"type": "text", "text": "Title: Public source\nURL: https://example.com/\nPublished: N/A\nText: Fixture excerpt"}}}
		default:
			t.Error("unexpected hosted request", frame.Method)
			result = map[string]any{}
		}
		data, _ := json.Marshal(map[string]any{"jsonrpc": "2.0", "id": frame.ID, "result": result})
		return responseFixture(string(data), "application/json"), nil
	})
	searcher := NewSearcher(SearchOptions{Transport: transport})
	settings := searchSettings(protocol.SearchDriverExa)
	config := settings.Providers[protocol.SearchDriverExa]
	config.KeyConfigured = false
	settings.Providers[protocol.SearchDriverExa] = config
	searcher.Configure(settings)
	page, err := searcher.Search(context.Background(), SearchPlan{Settings: settings}, protocol.SearchRequest{Query: "fixture", Limit: 5})
	mu.Lock()
	defer mu.Unlock()
	if err != nil || calls != 1 || len(page.Results) != 1 || page.Results[0].Snippet != "Fixture excerpt" || page.Results[0].Published != nil {
		t.Fatal(page, err, calls)
	}
}
