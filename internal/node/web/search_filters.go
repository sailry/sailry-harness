package web

import (
	"net/netip"
	"net/url"
	"slices"
	"strconv"
	"strings"
	"time"

	"github.com/sailry/sailry-harness/internal/protocol"
	"golang.org/x/net/idna"
)

func normalizeSearch(input protocol.SearchRequest) (protocol.SearchRequest, error) {
	if strings.TrimSpace(input.Query) == "" || strings.ContainsRune(input.Query, 0) || input.Limit == 0 {
		return input, webInvalid("invalid search query or result limit")
	}
	if len(input.IncludeDomains) > 20 || len(input.ExcludeDomains) > 20 {
		return input, webInvalid("search domain lists exceed the host limit")
	}
	normalize := func(values []string) ([]string, error) {
		values = slices.Clone(values)
		for index, value := range values {
			host, err := searchDomain(value)
			if err != nil {
				return nil, err
			}
			values[index] = host
		}
		slices.Sort(values)
		return slices.Compact(values), nil
	}
	var err error
	input.IncludeDomains, err = normalize(input.IncludeDomains)
	if err != nil {
		return input, err
	}
	input.ExcludeDomains, err = normalize(input.ExcludeDomains)
	if err != nil {
		return input, err
	}
	for _, domain := range input.ExcludeDomains {
		if slices.Contains(input.IncludeDomains, domain) {
			return input, webInvalid("search domain filters overlap")
		}
	}
	input.Query = strings.TrimSpace(input.Query)
	return input, nil
}

func searchDomain(value string) (string, error) {
	if strings.Contains(value, "%") || strings.TrimSpace(value) != value {
		return "", webInvalid("search domain filters require hostnames")
	}
	host, err := idna.Lookup.ToASCII(strings.ToLower(value))
	if err != nil {
		return "", webInvalid("search domain filters require hostnames")
	}
	host = strings.TrimSuffix(host, ".")
	if host == "" || len(host) > 253 {
		return "", webInvalid("search domain filters require hostnames")
	}
	if _, err := netip.ParseAddr(host); err == nil {
		return "", webInvalid("search domain filters require hostnames")
	}
	labels := strings.Split(host, ".")
	if _, err := strconv.ParseUint(labels[len(labels)-1], 0, 64); err == nil {
		return "", webInvalid("search domain filters require hostnames")
	}
	for _, label := range labels {
		if label == "" || len(label) > 63 || strings.HasPrefix(label, "-") || strings.HasSuffix(label, "-") {
			return "", webInvalid("search domain filters require hostnames")
		}
		for _, value := range []byte(label) {
			if !(value >= 'a' && value <= 'z' || value >= '0' && value <= '9' || value == '-') {
				return "", webInvalid("search domain filters require hostnames")
			}
		}
	}
	return host, nil
}

func filteredSearchQuery(input protocol.SearchRequest) string {
	query := input.Query
	if len(input.IncludeDomains) > 0 {
		sites := make([]string, len(input.IncludeDomains))
		for index, domain := range input.IncludeDomains {
			sites[index] = "site:" + domain
		}
		query += " "
		if len(sites) == 1 {
			query += sites[0]
		} else {
			query += "(" + strings.Join(sites, " OR ") + ")"
		}
	}
	for _, domain := range input.ExcludeDomains {
		query += " -site:" + domain
	}
	return query
}

func searchSince(freshness protocol.SearchFreshness, now time.Time) time.Time {
	switch freshness {
	case protocol.SearchFreshnessHour:
		return now.Add(-time.Hour)
	case protocol.SearchFreshnessDay:
		return now.Add(-24 * time.Hour)
	case protocol.SearchFreshnessWeek:
		return now.Add(-7 * 24 * time.Hour)
	case protocol.SearchFreshnessMonth:
		return now.Add(-31 * 24 * time.Hour)
	case protocol.SearchFreshnessYear:
		return now.Add(-365 * 24 * time.Hour)
	}
	return now
}

func filterSearchResults(results []protocol.SearchResult, input protocol.SearchRequest, now time.Time) []protocol.SearchResult {
	return slices.DeleteFunc(results, func(result protocol.SearchResult) bool {
		parsed, err := url.Parse(result.Url)
		if err != nil || parsed.Hostname() == "" {
			return true
		}
		host := strings.ToLower(parsed.Hostname())
		matches := func(domain string) bool { return host == domain || strings.HasSuffix(host, "."+domain) }
		if slices.ContainsFunc(input.ExcludeDomains, matches) || len(input.IncludeDomains) > 0 && !slices.ContainsFunc(input.IncludeDomains, matches) {
			return true
		}
		if input.Freshness == nil || result.Published == nil {
			return false
		}
		cutoff := searchSince(*input.Freshness, now)
		if date, err := time.Parse("2006-01-02", *result.Published); err == nil {
			return date.Format("2006-01-02") < cutoff.UTC().Format("2006-01-02")
		}
		for _, layout := range []string{time.RFC3339, time.RFC1123Z, time.RFC1123} {
			if date, err := time.Parse(layout, *result.Published); err == nil {
				return date.Before(cutoff)
			}
		}
		return false
	})
}
