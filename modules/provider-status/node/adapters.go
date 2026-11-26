package providerstatusnode

import (
	"encoding/json"
	"fmt"
	"math"
	"regexp"
	"strconv"
	"strings"
	"time"

	"github.com/sailry/sailry-harness/internal/protocol"
)

const (
	// UsageCacheTTLMs matches the 3-minute account usage cache budget.
	UsageCacheTTLMs = 180_000
	maxSafeInteger  = (1 << 53) - 1
)

var numericPattern = regexp.MustCompile(`^\d+(?:\.\d+)?$`)

var claudeWindowOrder = []string{
	"five_hour",
	"seven_day",
	"seven_day_sonnet",
	"seven_day_opus",
	"seven_day_oauth_apps",
	"seven_day_cowork",
}

// EndpointFor returns the fixed OAuth account usage endpoint for auth.
func EndpointFor(auth protocol.Authentication) (protocol.PluginModelAuthEndpoint, bool) {
	ttl := uint32(UsageCacheTTLMs)
	switch auth {
	case protocol.AuthenticationChatGpt, protocol.AuthenticationChatGptOAuth:
		return protocol.PluginModelAuthEndpoint{
			Method:     "GET",
			Url:        "https://chatgpt.com/backend-api/wham/usage",
			Headers:    map[string]string{"Accept": "application/json", "User-Agent": "Sailry/0.1"},
			CacheTtlMs: &ttl,
		}, true
	case protocol.AuthenticationClaudeOAuth:
		return protocol.PluginModelAuthEndpoint{
			Method: "GET",
			Url:    "https://api.anthropic.com/api/oauth/usage",
			Headers: map[string]string{
				"Accept":         "application/json",
				"User-Agent":     "claude-cli/2.1.280",
				"anthropic-beta": "oauth-2025-04-20",
			},
			CacheTtlMs: &ttl,
		}, true
	default:
		return protocol.PluginModelAuthEndpoint{}, false
	}
}

// DecodeAccountUsage parses and normalizes an upstream OAuth usage HTTP reply.
func DecodeAccountUsage(auth protocol.Authentication, resp protocol.PluginHttpResponse, now time.Time) (protocol.ProviderAccountUsage, error) {
	switch auth {
	case protocol.AuthenticationClaudeOAuth:
		return decodeClaudeUsage(resp)
	case protocol.AuthenticationChatGpt, protocol.AuthenticationChatGptOAuth:
		return decodeCodexUsage(resp, now.UnixMilli())
	default:
		return protocol.ProviderAccountUsage{}, &protocol.Fault{
			Code:    protocol.ErrorCodeNotConfigured,
			Message: "account usage is unavailable for this authentication method",
		}
	}
}

func decodeResponseObject(resp protocol.PluginHttpResponse) (map[string]any, error) {
	if resp.Status < 200 || resp.Status >= 300 {
		msg := extractErrorMessage(resp.Body)
		if msg == "" {
			msg = fmt.Sprintf("provider usage request returned status %d", resp.Status)
		}
		return nil, &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: msg}
	}
	var raw any
	if err := json.Unmarshal([]byte(resp.Body), &raw); err != nil {
		return nil, &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "invalid provider usage response"}
	}
	obj, ok := raw.(map[string]any)
	if !ok || obj == nil {
		return nil, &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "invalid provider usage response"}
	}
	if errVal, has := obj["error"]; has && isTruthy(errVal) {
		msg := extractErrorFromMap(obj)
		if msg == "" {
			msg = "provider usage returned an error"
		}
		return nil, &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: msg}
	}
	return obj, nil
}

func extractErrorMessage(body string) string {
	var raw any
	if err := json.Unmarshal([]byte(body), &raw); err != nil {
		return ""
	}
	obj, ok := raw.(map[string]any)
	if !ok {
		return ""
	}
	return extractErrorFromMap(obj)
}

func extractErrorFromMap(obj map[string]any) string {
	if errObj, ok := obj["error"].(map[string]any); ok {
		if msg, ok := errObj["message"].(string); ok && strings.TrimSpace(msg) != "" {
			return msg
		}
	}
	if errStr, ok := obj["error"].(string); ok && strings.TrimSpace(errStr) != "" {
		return errStr
	}
	if msg, ok := obj["message"].(string); ok && strings.TrimSpace(msg) != "" {
		return msg
	}
	return ""
}

func isTruthy(v any) bool {
	if v == nil {
		return false
	}
	switch x := v.(type) {
	case bool:
		return x
	case float64:
		return x != 0 && !math.IsNaN(x)
	case string:
		return x != ""
	default:
		return true
	}
}

func parseNonNegativeNumber(v any) (*float64, error) {
	if v == nil {
		return nil, nil
	}
	var out float64
	switch x := v.(type) {
	case float64:
		out = x
	case string:
		if !numericPattern.MatchString(x) {
			return nil, &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "invalid provider usage number"}
		}
		parsed, err := strconv.ParseFloat(x, 64)
		if err != nil {
			return nil, &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "invalid provider usage number"}
		}
		out = parsed
	default:
		return nil, &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "invalid provider usage number"}
	}
	if math.IsNaN(out) || math.IsInf(out, 0) || out < 0 {
		return nil, &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "invalid provider usage number"}
	}
	return &out, nil
}

func parseISOInstantMs(v any) (*int64, error) {
	if v == nil {
		return nil, nil
	}
	s, ok := v.(string)
	if !ok {
		return nil, &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "invalid provider usage reset"}
	}
	parsed, err := time.Parse(time.RFC3339Nano, s)
	if err != nil {
		parsed, err = time.Parse(time.RFC3339, s)
		if err != nil {
			return nil, &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "invalid provider usage reset"}
		}
	}
	ms := parsed.UnixMilli()
	if ms <= 0 || ms > maxSafeInteger {
		return nil, &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "invalid provider usage reset"}
	}
	return &ms, nil
}

func secondsToMs(v any) (*int64, error) {
	num, err := parseNonNegativeNumber(v)
	if err != nil || num == nil {
		return nil, err
	}
	scaled := *num * 1000
	if math.IsNaN(scaled) || math.IsInf(scaled, 0) || scaled != math.Trunc(scaled) || scaled > maxSafeInteger {
		return nil, &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "invalid provider usage reset"}
	}
	ms := int64(scaled)
	return &ms, nil
}

func decodeClaudeUsage(resp protocol.PluginHttpResponse) (protocol.ProviderAccountUsage, error) {
	obj, err := decodeResponseObject(resp)
	if err != nil {
		return protocol.ProviderAccountUsage{}, err
	}
	hasKnown := false
	for _, k := range claudeWindowOrder {
		if _, ok := obj[k]; ok {
			hasKnown = true
			break
		}
	}
	if _, ok := obj["extra_usage"]; ok {
		hasKnown = true
	}
	if !hasKnown {
		return protocol.ProviderAccountUsage{}, &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "Claude usage windows are missing"}
	}
	windows := make([]protocol.ProviderAccountUsageWindow, 0, len(claudeWindowOrder))
	for _, key := range claudeWindowOrder {
		raw, ok := obj[key]
		if !ok || raw == nil {
			continue
		}
		win, ok := raw.(map[string]any)
		if !ok || win == nil {
			return protocol.ProviderAccountUsage{}, &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "invalid Claude usage window"}
		}
		used, err := parseNonNegativeNumber(win["utilization"])
		if err != nil {
			return protocol.ProviderAccountUsage{}, err
		}
		resetMs, err := parseISOInstantMs(win["resets_at"])
		if err != nil {
			return protocol.ProviderAccountUsage{}, err
		}
		windows = append(windows, protocol.ProviderAccountUsageWindow{
			Key:     key,
			Used:    used,
			ResetMs: resetMs,
		})
	}
	var extra *protocol.ProviderAccountUsageExtra
	if rawExtra, ok := obj["extra_usage"]; ok && rawExtra != nil {
		extraObj, ok := rawExtra.(map[string]any)
		if !ok || extraObj == nil {
			return protocol.ProviderAccountUsage{}, &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "invalid Claude extra usage"}
		}
		used, err := parseNonNegativeNumber(extraObj["used_credits"])
		if err != nil {
			return protocol.ProviderAccountUsage{}, err
		}
		limit, err := parseNonNegativeNumber(extraObj["monthly_limit"])
		if err != nil {
			return protocol.ProviderAccountUsage{}, err
		}
		var currency *string
		if s, ok := extraObj["currency"].(string); ok {
			currency = &s
		}
		var enabled *bool
		if b, ok := extraObj["is_enabled"].(bool); ok {
			enabled = &b
		}
		extra = &protocol.ProviderAccountUsageExtra{
			Used:     used,
			Limit:    limit,
			Currency: currency,
			Enabled:  enabled,
		}
	}
	return protocol.ProviderAccountUsage{
		Windows: windows,
		Credits: nil,
		Extra:   extra,
	}, nil
}

func decodeCodexUsage(resp protocol.PluginHttpResponse, nowMs int64) (protocol.ProviderAccountUsage, error) {
	obj, err := decodeResponseObject(resp)
	if err != nil {
		return protocol.ProviderAccountUsage{}, err
	}
	_, hasRateLimit := obj["rate_limit"]
	_, hasCredits := obj["credits"]
	if !hasRateLimit && !hasCredits {
		return protocol.ProviderAccountUsage{}, &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "GPT usage windows are missing"}
	}
	var limitObj map[string]any
	if rawLimit := obj["rate_limit"]; rawLimit != nil {
		var ok bool
		limitObj, ok = rawLimit.(map[string]any)
		if !ok || limitObj == nil {
			return protocol.ProviderAccountUsage{}, &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "invalid GPT rate limit"}
		}
	}
	windows := make([]protocol.ProviderAccountUsageWindow, 0, 2)
	for _, name := range []string{"primary_window", "secondary_window"} {
		if limitObj == nil {
			continue
		}
		rawWin := limitObj[name]
		if rawWin == nil {
			continue
		}
		win, ok := rawWin.(map[string]any)
		if !ok || win == nil {
			return protocol.ProviderAccountUsage{}, &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "invalid GPT usage window"}
		}
		duration, err := parseNonNegativeNumber(win["limit_window_seconds"])
		if err != nil {
			return protocol.ProviderAccountUsage{}, err
		}
		key := "secondary"
		if name == "primary_window" {
			key = "primary"
		}
		if duration != nil {
			switch *duration {
			case 18000:
				key = "five_hour"
			case 604800:
				key = "seven_day"
			}
		}
		resetMs, err := secondsToMs(win["reset_at"])
		if err != nil {
			return protocol.ProviderAccountUsage{}, err
		}
		if resetMs == nil && win["reset_after_seconds"] != nil {
			delta, err := secondsToMs(win["reset_after_seconds"])
			if err != nil {
				return protocol.ProviderAccountUsage{}, err
			}
			if delta != nil {
				sum := nowMs + *delta
				if sum <= 0 || sum > maxSafeInteger {
					return protocol.ProviderAccountUsage{}, &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "invalid provider usage reset"}
				}
				resetMs = &sum
			}
		}
		used, err := parseNonNegativeNumber(win["used_percent"])
		if err != nil {
			return protocol.ProviderAccountUsage{}, err
		}
		windows = append(windows, protocol.ProviderAccountUsageWindow{
			Key:     key,
			Used:    used,
			ResetMs: resetMs,
		})
	}
	var credits *protocol.ProviderAccountUsageCredits
	if rawCredits := obj["credits"]; rawCredits != nil {
		credObj, ok := rawCredits.(map[string]any)
		if !ok || credObj == nil {
			return protocol.ProviderAccountUsage{}, &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "invalid GPT credits"}
		}
		balance, err := parseNonNegativeNumber(credObj["balance"])
		if err != nil {
			return protocol.ProviderAccountUsage{}, err
		}
		var unlimited *bool
		if b, ok := credObj["unlimited"].(bool); ok {
			unlimited = &b
		}
		var available *bool
		if b, ok := credObj["has_credits"].(bool); ok {
			available = &b
		}
		credits = &protocol.ProviderAccountUsageCredits{
			Balance:   balance,
			Unlimited: unlimited,
			Available: available,
		}
	}
	return protocol.ProviderAccountUsage{
		Windows: windows,
		Credits: credits,
		Extra:   nil,
	}, nil
}
