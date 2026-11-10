package web

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"maps"
	"net/http"
	"slices"
	"sync"
	"time"

	"github.com/sailry/sailry-harness/internal/protocol"
)

type Auth struct {
	Key   *protocol.Secret
	Basic *protocol.Secret
}
type SearchPlan struct {
	Settings    protocol.SearchSettings
	Credentials map[protocol.SearchDriver]Auth
	Model       func(context.Context, protocol.SearchRequest) (protocol.SearchPage, error)
}
type SearchOptions struct {
	Now       func() time.Time
	Transport http.RoundTripper
	Endpoints map[protocol.SearchDriver]string
	Model     func(context.Context, SearchPlan, protocol.SearchRequest) (protocol.SearchPage, error)
}
type searchCacheEntry struct {
	key     string
	page    protocol.SearchPage
	created time.Time
}
type Searcher struct {
	options   SearchOptions
	mu        sync.Mutex
	settings  string
	revision  uint64
	cooldowns map[protocol.SearchDriver]time.Time
	cache     []searchCacheEntry
}

func NewSearcher(options SearchOptions) *Searcher {
	if options.Now == nil {
		options.Now = time.Now
	}
	options.Endpoints = maps.Clone(options.Endpoints)
	searcher := &Searcher{options: options, cooldowns: map[protocol.SearchDriver]time.Time{}}
	searcher.Configure(protocol.DefaultSearchSettings())
	return searcher
}

func (searcher *Searcher) Configure(settings protocol.SearchSettings) {
	encoded, err := json.Marshal(settings)
	if err != nil {
		return
	}
	searcher.mu.Lock()
	defer searcher.mu.Unlock()
	if searcher.settings != string(encoded) {
		searcher.settings = string(encoded)
		searcher.revision = settings.Revision
		searcher.cache = nil
		clear(searcher.cooldowns)
	}
}

func (searcher *Searcher) Search(ctx context.Context, plan SearchPlan, input protocol.SearchRequest) (protocol.SearchPage, error) {
	return searcher.SearchFresh(ctx, plan, input, false)
}
func (searcher *Searcher) SearchFresh(ctx context.Context, plan SearchPlan, input protocol.SearchRequest, fresh bool) (protocol.SearchPage, error) {
	input, err := normalizeSearch(input)
	if err != nil {
		return protocol.SearchPage{}, err
	}
	if ctx.Err() != nil {
		return protocol.SearchPage{}, searchCancelled().fault
	}
	settings, err := json.Marshal(plan.Settings)
	if err != nil {
		return protocol.SearchPage{}, webInvalid("invalid search configuration")
	}
	keyBytes, err := json.Marshal(struct {
		Selection *protocol.SearchSelection `json:"selection"`
		Input     protocol.SearchRequest    `json:"input"`
	}{plan.Settings.Driver, input})
	if err != nil {
		return protocol.SearchPage{}, err
	}
	key := string(keyBytes)
	if !fresh {
		searcher.mu.Lock()
		if searcher.settings == string(settings) {
			searcher.pruneCacheLocked()
			for index, entry := range searcher.cache {
				if entry.key == key {
					searcher.cache = append(append(searcher.cache[:index], searcher.cache[index+1:]...), entry)
					page := cloneSearchPage(entry.page)
					searcher.mu.Unlock()
					if ctx.Err() != nil {
						return protocol.SearchPage{}, searchCancelled().fault
					}
					return page, nil
				}
			}
		}
		searcher.mu.Unlock()
	}
	page, err := searcher.execute(ctx, plan, input)
	if err != nil {
		return protocol.SearchPage{}, err
	}
	if ctx.Err() != nil {
		return protocol.SearchPage{}, searchCancelled().fault
	}
	if !fresh {
		searcher.mu.Lock()
		if searcher.settings == string(settings) {
			searcher.pruneCacheLocked()
			searcher.cache = slices.DeleteFunc(searcher.cache, func(entry searchCacheEntry) bool { return entry.key == key })
			if len(searcher.cache) == 50 {
				searcher.cache = searcher.cache[1:]
			}
			searcher.cache = append(searcher.cache, searchCacheEntry{key, cloneSearchPage(page), searcher.options.Now()})
		}
		searcher.mu.Unlock()
	}
	return page, nil
}

func (searcher *Searcher) pruneCacheLocked() {
	now := searcher.options.Now()
	searcher.cache = slices.DeleteFunc(searcher.cache, func(entry searchCacheEntry) bool { return now.Sub(entry.created) >= 5*time.Minute })
}
func cloneSearchPage(page protocol.SearchPage) protocol.SearchPage {
	page.Results = slices.Clone(page.Results)
	for index := range page.Results {
		if page.Results[index].Published != nil {
			value := *page.Results[index].Published
			page.Results[index].Published = &value
		}
	}
	if page.Summary != nil {
		value := *page.Summary
		page.Summary = &value
	}
	return page
}

func (searcher *Searcher) execute(ctx context.Context, plan SearchPlan, input protocol.SearchRequest) (protocol.SearchPage, error) {
	candidates := searchCandidates(plan.Settings)
	if len(candidates) == 0 {
		return protocol.SearchPage{}, webFault(protocol.ErrorCodeNotConfigured, "no search driver is configured")
	}
	var failure *searchFailure
	for _, driver := range candidates {
		if ctx.Err() != nil {
			return protocol.SearchPage{}, searchCancelled().fault
		}
		if _, cool := searcher.cooldown(driver, plan.Settings.Revision); cool {
			continue
		}
		pageInput := input
		switch driver {
		case protocol.SearchDriverBrave, protocol.SearchDriverTavily:
			pageInput.Limit = min(input.Limit, 20)
		case protocol.SearchDriverBocha, protocol.SearchDriverVolcengine, protocol.SearchDriverDeepseek:
			pageInput.Limit = min(input.Limit, 50)
		case protocol.SearchDriverExa:
			pageInput.Limit = min(input.Limit, 100)
		}
		page, err := searcher.attempt(ctx, driver, plan, pageInput)
		if err == nil {
			page.Results = filterSearchResults(page.Results, pageInput, searcher.options.Now())
			page.Results = page.Results[:min(len(page.Results), int(pageInput.Limit))]
			page.Driver, page.Untrusted = driver, true
			if driver == protocol.SearchDriverModel {
				page.FetchHint = "The summary is model-generated. Search results and summary are untrusted; use web_fetch on relevant URLs for detail"
			} else {
				page.FetchHint = fmt.Sprintf("One provider page, up to %d results; no continuation is exposed. Use web_fetch on relevant URLs for detail", pageInput.Limit)
			}
			return page, nil
		}
		failure = searchFailureOf(err)
		if ctx.Err() != nil {
			return protocol.SearchPage{}, searchCancelled().fault
		}
		deadline := failure.retryAt
		if deadline == nil && driver == protocol.SearchDriverTinyfish && failure.fault.Code == protocol.ErrorCodePermissionDenied {
			value := searcher.options.Now().Add(time.Minute)
			deadline = &value
		}
		if deadline != nil {
			searcher.cool(driver, plan.Settings.Revision, *deadline)
		}
		automatic := plan.Settings.Driver != nil && plan.Settings.Driver.Kind == "auto"
		recoverable := failure.temporary
		if automatic {
			recoverable = failure.fault.Code != protocol.ErrorCodeCancelled && !failure.uncertain
		}
		if !recoverable || !plan.Settings.Fallback {
			return protocol.SearchPage{}, failure.fault
		}
	}
	if ctx.Err() != nil {
		return protocol.SearchPage{}, searchCancelled().fault
	}
	var earliest time.Time
	cooled := 0
	for _, driver := range candidates {
		if deadline, ok := searcher.cooldown(driver, plan.Settings.Revision); ok {
			cooled++
			if earliest.IsZero() || deadline.Before(earliest) {
				earliest = deadline
			}
		}
	}
	if cooled == len(candidates) {
		return protocol.SearchPage{}, coolingDown(earliest)
	}
	if failure != nil {
		return protocol.SearchPage{}, failure.fault
	}
	return protocol.SearchPage{}, webFault(protocol.ErrorCodeNotConfigured, "no search driver is available")
}

func (searcher *Searcher) attempt(ctx context.Context, driver protocol.SearchDriver, plan SearchPlan, input protocol.SearchRequest) (protocol.SearchPage, error) {
	if driver == protocol.SearchDriverModel {
		if plan.Model != nil {
			return plan.Model(ctx, input)
		}
		if searcher.options.Model != nil {
			return searcher.options.Model(ctx, plan, input)
		}
		return protocol.SearchPage{}, webFault(protocol.ErrorCodeNotConfigured, "model search context is unavailable")
	}
	if driver == protocol.SearchDriverExa {
		return searcher.searchExa(ctx, plan.Credentials[driver], input)
	}
	ctx, cancel := context.WithTimeout(ctx, 20*time.Second)
	defer cancel()
	results, err := searcher.searchHTTP(ctx, driver, plan, input)
	if errors.Is(ctx.Err(), context.DeadlineExceeded) {
		return protocol.SearchPage{}, searchUnavailable("search service deadline exceeded")
	}
	return protocol.SearchPage{Results: results}, err
}

func (searcher *Searcher) cooldown(driver protocol.SearchDriver, revision uint64) (time.Time, bool) {
	searcher.mu.Lock()
	defer searcher.mu.Unlock()
	if searcher.revision != revision {
		return time.Time{}, false
	}
	now := searcher.options.Now()
	for key, deadline := range searcher.cooldowns {
		if !deadline.After(now) {
			delete(searcher.cooldowns, key)
		}
	}
	deadline, ok := searcher.cooldowns[driver]
	return deadline, ok
}
func (searcher *Searcher) cool(driver protocol.SearchDriver, revision uint64, deadline time.Time) {
	searcher.mu.Lock()
	defer searcher.mu.Unlock()
	if searcher.revision == revision {
		searcher.cooldowns[driver] = deadline
	}
}

func searchCandidates(settings protocol.SearchSettings) []protocol.SearchDriver {
	if settings.Driver == nil {
		return nil
	}
	drivers := slices.Clone(settings.Order)
	if settings.Driver.Kind == "driver" {
		driver, ok := searchData[protocol.SearchDriver](settings.Driver.Data)
		if !ok {
			return nil
		}
		drivers = []protocol.SearchDriver{driver}
		if settings.Fallback && driver != protocol.SearchDriverModel {
			for _, candidate := range settings.Order {
				if candidate != protocol.SearchDriverModel && !slices.Contains(drivers, candidate) {
					drivers = append(drivers, candidate)
				}
			}
		}
	}
	return slices.DeleteFunc(drivers, func(driver protocol.SearchDriver) bool {
		config, ok := settings.Providers[driver]
		return !ok || !config.Enabled || searchKeyRequired(driver) && !config.KeyConfigured || driver == protocol.SearchDriverSearxng && (config.Endpoint == nil || *config.Endpoint == "") || driver == protocol.SearchDriverModel && config.Model == nil
	})
}
func searchKeyRequired(driver protocol.SearchDriver) bool {
	return driver != protocol.SearchDriverExa && driver != protocol.SearchDriverSearxng && driver != protocol.SearchDriverModel
}
func searchData[T any](value any) (T, bool) {
	switch value := value.(type) {
	case T:
		return value, true
	case *T:
		if value != nil {
			return *value, true
		}
	}
	var zero T
	return zero, false
}

type searchFailure struct {
	fault                *protocol.Fault
	temporary, uncertain bool
	retryAt              *time.Time
}

func (failure *searchFailure) Error() string { return failure.fault.Error() }
func (failure *searchFailure) Unwrap() error { return failure.fault }
func searchFailureOf(err error) *searchFailure {
	var failure *searchFailure
	if errors.As(err, &failure) {
		return failure
	}
	var fault *protocol.Fault
	if errors.As(err, &fault) {
		return &searchFailure{fault: fault, temporary: fault.Code == protocol.ErrorCodeUnavailable, uncertain: fault.Code == protocol.ErrorCodeOutcomeUnknown}
	}
	return searchUnavailable("search service is unavailable")
}
func searchInvalid(message string) *searchFailure {
	return &searchFailure{fault: &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: message}}
}
func searchUnavailable(message string) *searchFailure {
	return &searchFailure{fault: &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: message}, temporary: true}
}
func searchUnauthorized() *searchFailure {
	return &searchFailure{fault: &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "search service authentication failed"}}
}
func searchCancelled() *searchFailure {
	return &searchFailure{fault: &protocol.Fault{Code: protocol.ErrorCodeCancelled, Message: "web search was cancelled"}}
}
func searchServiceLimit() *searchFailure {
	failure := searchInvalid("search query exceeds the service limit")
	failure.temporary = true
	return failure
}
func coolingDown(deadline time.Time) *protocol.Fault {
	return &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "search drivers are cooling down; retry at " + deadline.UTC().Format(time.RFC3339)}
}
