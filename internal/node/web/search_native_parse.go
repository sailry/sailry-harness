package web

import (
	"net/url"
	"strings"
	"time"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func nativeResponsesParse(value map[string]any, now time.Time) (protocol.SearchPage, error) {
	page := protocol.SearchPage{Results: []protocol.SearchResult{}}
	if value["error"] != nil {
		return page, nativeUpstream(value["error"], now)
	}
	if jsonString(value["status"]) != "completed" {
		return page, nativeInvalid("model search response did not complete")
	}
	output, ok := value["output"].([]any)
	if !ok {
		return page, nativeInvalid("model search response has no output")
	}
	var parts []string
	searched := false
	for _, item := range output {
		switch jsonString(jsonAt(item, "type")) {
		case "web_search_call":
			if jsonString(jsonAt(item, "status")) != "completed" {
				return page, nativeInvalid("native web search did not complete")
			}
			searched = true
			sources, _ := jsonAt(item, "action", "sources").([]any)
			for _, source := range sources {
				if sourceURL, ok := jsonAt(source, "url").(string); ok {
					if err := nativePush(&page.Results, sourceURL, jsonString(jsonAt(source, "title")), ""); err != nil {
						return page, err
					}
				}
			}
		case "message":
			content, _ := jsonAt(item, "content").([]any)
			for _, block := range content {
				if jsonString(jsonAt(block, "type")) != "output_text" {
					continue
				}
				parts = append(parts, jsonString(jsonAt(block, "text")))
				annotations, _ := jsonAt(block, "annotations").([]any)
				for _, citation := range annotations {
					if jsonString(jsonAt(citation, "type")) != "url_citation" {
						continue
					}
					sourceURL, present := jsonAt(citation, "url").(string)
					if !present {
						return page, nativeInvalid("model search citation URL is missing")
					}
					// Generated answer spans are not source excerpts.
					if err := nativePush(&page.Results, sourceURL, jsonString(jsonAt(citation, "title")), ""); err != nil {
						return page, err
					}
				}
			}
		}
	}
	if !searched {
		return page, nativeInvalid("model returned no web search metadata")
	}
	page.Summary = nativeSummary(parts)
	return page, nil
}

func nativeAnthropicParse(value map[string]any, now time.Time) (protocol.SearchPage, error) {
	page := protocol.SearchPage{Results: []protocol.SearchResult{}}
	if err := nativeAnthropicCheck(value, now); err != nil {
		return page, err
	}
	if jsonString(value["stop_reason"]) != "end_turn" {
		return page, nativeInvalid("model search response did not complete")
	}
	content, ok := value["content"].([]any)
	if !ok {
		return page, nativeInvalid("model search response has no content")
	}
	var parts []string
	searched := false
	var toolError any
	for _, block := range content {
		switch jsonString(jsonAt(block, "type")) {
		case "web_search_tool_result":
			result := jsonAt(block, "content")
			if jsonString(jsonAt(result, "type")) == "web_search_tool_result_error" {
				toolError = result
				continue
			}
			entries, ok := result.([]any)
			if !ok {
				return page, nativeInvalid("model search result list is invalid")
			}
			searched = true
			for _, entry := range entries {
				if jsonString(jsonAt(entry, "type")) != "web_search_result" {
					continue
				}
				sourceURL, present := jsonAt(entry, "url").(string)
				if !present {
					return page, nativeInvalid("model search result URL is missing")
				}
				if err := nativePush(&page.Results, sourceURL, jsonString(jsonAt(entry, "title")), ""); err != nil {
					return page, err
				}
			}
		case "text":
			parts = append(parts, jsonString(jsonAt(block, "text")))
			citations, _ := jsonAt(block, "citations").([]any)
			for _, citation := range citations {
				if jsonString(jsonAt(citation, "type")) != "web_search_result_location" {
					continue
				}
				sourceURL, present := jsonAt(citation, "url").(string)
				if !present {
					return page, nativeInvalid("model search citation URL is missing")
				}
				if err := nativePush(&page.Results, sourceURL, jsonString(jsonAt(citation, "title")), jsonString(jsonAt(citation, "cited_text"))); err != nil {
					return page, err
				}
			}
		}
	}
	if !searched {
		if toolError != nil {
			return page, nativeUpstream(toolError, now)
		}
		return page, nativeInvalid("model returned no web search metadata")
	}
	page.Summary = nativeSummary(parts)
	return page, nil
}

func nativeAnthropicCheck(value map[string]any, now time.Time) error {
	if jsonString(value["type"]) == "error" {
		if upstream, present := value["error"]; present {
			return nativeUpstream(upstream, now)
		}
		return nativeUpstream(value, now)
	}
	content, _ := value["content"].([]any)
	for _, block := range content {
		result := jsonAt(block, "content")
		if jsonString(jsonAt(block, "type")) == "web_search_tool_result" && jsonString(jsonAt(result, "type")) == "web_search_tool_result_error" && jsonString(jsonAt(result, "error_code")) != "max_uses_exceeded" {
			return nativeUpstream(result, now)
		}
	}
	return nil
}

func nativeGeminiParse(value map[string]any, now time.Time) (protocol.SearchPage, error) {
	page := protocol.SearchPage{Results: []protocol.SearchResult{}}
	if value["error"] != nil {
		return page, nativeUpstream(value["error"], now)
	}
	candidates, _ := value["candidates"].([]any)
	if len(candidates) == 0 {
		return page, nativeInvalid("model search response has no candidate")
	}
	candidate := candidates[0]
	if jsonString(jsonAt(candidate, "finishReason")) != "STOP" {
		return page, nativeInvalid("model search response did not complete")
	}
	metadata := jsonAt(candidate, "groundingMetadata")
	chunks, chunkList := jsonAt(metadata, "groundingChunks").([]any)
	_, queryList := jsonAt(metadata, "webSearchQueries").([]any)
	if !chunkList && !queryList {
		return page, nativeInvalid("model returned no web search metadata")
	}
	for _, chunk := range chunks {
		web, ok := jsonAt(chunk, "web").(map[string]any)
		if !ok {
			continue
		}
		sourceURL, present := web["uri"].(string)
		if !present {
			return page, nativeInvalid("model search source URL is missing")
		}
		if err := nativePush(&page.Results, sourceURL, jsonString(web["title"]), ""); err != nil {
			return page, err
		}
	}
	var parts []string
	blocks, _ := jsonAt(candidate, "content", "parts").([]any)
	for _, block := range blocks {
		if thought, _ := jsonAt(block, "thought").(bool); !thought {
			parts = append(parts, jsonString(jsonAt(block, "text")))
		}
	}
	page.Summary = nativeSummary(parts)
	return page, nil
}

func nativeSourceURL(value string) (string, bool) {
	parsed, err := url.Parse(value)
	if err != nil || parsed.Hostname() == "" || parsed.User != nil || !strings.EqualFold(parsed.Scheme, "http") && !strings.EqualFold(parsed.Scheme, "https") {
		return "", false
	}
	parsed.Scheme = strings.ToLower(parsed.Scheme)
	parsed.Host = strings.ToLower(parsed.Host)
	if parsed.Port() == "80" && parsed.Scheme == "http" || parsed.Port() == "443" && parsed.Scheme == "https" {
		parsed.Host = parsed.Hostname()
		if strings.Contains(parsed.Host, ":") {
			parsed.Host = "[" + parsed.Host + "]"
		}
	}
	if parsed.Path == "" {
		parsed.Path = "/"
	}
	return parsed.String(), true
}

func nativePush(results *[]protocol.SearchResult, value, title, snippet string) error {
	canonical, ok := nativeSourceURL(value)
	if !ok {
		return nativeInvalid("model search result URL is invalid")
	}
	if strings.TrimSpace(title) == "" {
		title = ""
	}
	if strings.TrimSpace(snippet) == "" {
		snippet = ""
	}
	for index := range *results {
		result := &(*results)[index]
		if result.Url != canonical {
			continue
		}
		if result.Title == result.Url && title != "" {
			result.Title = searchText(title, 512)
		}
		if snippet != "" && !strings.Contains(result.Snippet, snippet) {
			if result.Snippet != "" {
				result.Snippet += "\n"
			}
			result.Snippet = searchText(result.Snippet+snippet, 4000)
		}
		return nil
	}
	if title == "" {
		title = canonical
	}
	*results = append(*results, protocol.SearchResult{Url: canonical, Title: searchText(title, 512), Snippet: searchText(snippet, 4000)})
	return nil
}

func nativeSummary(parts []string) *string {
	selected := parts[:0]
	for _, part := range parts {
		if strings.TrimSpace(part) != "" {
			selected = append(selected, part)
		}
	}
	if len(selected) == 0 {
		return nil
	}
	value := searchText(strings.Join(selected, "\n\n"), 16000)
	return &value
}

func nativeProtect(page protocol.SearchPage, secrets []protocol.Secret) error {
	contains := func(value string) bool {
		for _, secret := range secrets {
			if secret.Expose() != "" && strings.Contains(value, secret.Expose()) {
				return true
			}
		}
		return false
	}
	if page.Summary != nil && contains(*page.Summary) {
		return nativeInvalid("model search response contains protected data")
	}
	for _, result := range page.Results {
		if contains(result.Url) || contains(result.Title) || contains(result.Snippet) {
			return nativeInvalid("model search response contains protected data")
		}
	}
	return nil
}
