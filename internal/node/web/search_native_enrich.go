package web

import (
	"context"
	"net/url"
	"slices"
	"time"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func nativeSelect(results []protocol.SearchResult, input protocol.SearchRequest, now time.Time) []protocol.SearchResult {
	selected := make([]protocol.SearchResult, 0, len(results))
	for _, result := range results {
		filters := input
		parsed, err := url.Parse(result.Url)
		if err == nil && parsed.Hostname() == "vertexaisearch.cloud.google.com" && len(parsed.Path) >= len("/grounding-api-redirect/") && parsed.Path[:len("/grounding-api-redirect/")] == "/grounding-api-redirect/" {
			// The redirect host does not identify the cited source's domain.
			filters.IncludeDomains, filters.ExcludeDomains = nil, nil
		}
		if len(filterSearchResults([]protocol.SearchResult{result}, filters, now)) > 0 {
			selected = append(selected, result)
		}
	}
	return selected[:min(len(selected), int(input.Limit))]
}

func nativeEnrich(ctx context.Context, results []protocol.SearchResult, fetch func(context.Context, protocol.WebRequest) (protocol.WebPage, error)) ([]protocol.SearchResult, error) {
	type job struct {
		index int
		url   string
	}
	type read struct {
		index int
		page  protocol.WebPage
		err   error
	}
	jobs := make(chan job, len(results))
	count := 0
	for index, result := range results {
		if result.Snippet == "" {
			jobs <- job{index, result.Url}
			count++
		}
	}
	close(jobs)
	operation, cancel := context.WithTimeout(ctx, 10*time.Second)
	defer cancel()
	reads := make(chan read, count)
	for worker := 0; worker < min(count, 4); worker++ {
		go func() {
			for next := range jobs {
				if operation.Err() != nil {
					return
				}
				page, err := fetch(operation, protocol.WebRequest{Url: next.url})
				reads <- read{next.index, page, err}
			}
		}()
	}
	results = slices.Clone(results)
collect:
	for completed := 0; completed < count; completed++ {
		if ctx.Err() != nil {
			return nil, searchCancelled()
		}
		select {
		case <-ctx.Done():
			return nil, searchCancelled()
		case <-operation.Done():
			break collect
		case next := <-reads:
			if next.err == nil && next.page.Status >= 200 && next.page.Status < 300 {
				if finalURL, ok := nativeSourceURL(next.page.Url); ok {
					results[next.index].Url = finalURL
					results[next.index].Snippet = searchText(next.page.Content, 4000)
				}
			}
		}
	}
	if ctx.Err() != nil {
		return nil, searchCancelled()
	}
	seen := map[string]bool{}
	return slices.DeleteFunc(results, func(result protocol.SearchResult) bool {
		if seen[result.Url] {
			return true
		}
		seen[result.Url] = true
		return false
	}), nil
}
