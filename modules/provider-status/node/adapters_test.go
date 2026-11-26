package providerstatusnode

import (
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestDecodeClaudeUsage(t *testing.T) {
	t.Parallel()
	now := time.Date(2026, 4, 15, 12, 0, 0, 0, time.UTC)
	body := `{
		"seven_day": {"utilization": "42.5", "resets_at": "2026-04-20T00:00:00Z"},
		"five_hour": {"utilization": 115, "resets_at": "2026-04-15T15:30:00Z"},
		"seven_day_sonnet": {"utilization": 10, "resets_at": null},
		"extra_usage": {"used_credits": 1250, "monthly_limit": "5000", "currency": "USD", "is_enabled": true}
	}`
	usage, err := DecodeAccountUsage(protocol.AuthenticationClaudeOAuth, protocol.PluginHttpResponse{
		Status: 200,
		Body:   body,
	}, now)
	if err != nil {
		t.Fatalf("DecodeAccountUsage: %v", err)
	}
	if len(usage.Windows) != 3 {
		t.Fatalf("windows len = %d, want 3", len(usage.Windows))
	}
	if usage.Windows[0].Key != "five_hour" || usage.Windows[0].Used == nil || *usage.Windows[0].Used != 115 {
		t.Fatalf("window[0] = %+v", usage.Windows[0])
	}
	if usage.Windows[1].Key != "seven_day" || usage.Windows[1].Used == nil || *usage.Windows[1].Used != 42.5 {
		t.Fatalf("window[1] = %+v", usage.Windows[1])
	}
	if usage.Windows[2].Key != "seven_day_sonnet" || usage.Windows[2].ResetMs != nil {
		t.Fatalf("window[2] = %+v", usage.Windows[2])
	}
	if usage.Extra == nil || usage.Extra.Used == nil || *usage.Extra.Used != 1250 ||
		usage.Extra.Limit == nil || *usage.Extra.Limit != 5000 ||
		usage.Extra.Currency == nil || *usage.Extra.Currency != "USD" ||
		usage.Extra.Enabled == nil || !*usage.Extra.Enabled {
		t.Fatalf("extra = %+v", usage.Extra)
	}
}

func TestDecodeCodexUsage(t *testing.T) {
	t.Parallel()
	now := time.UnixMilli(1_700_000_000_000)
	body := `{
		"rate_limit": {
			"primary_window": {"limit_window_seconds": 18000, "used_percent": 32, "reset_at": 1700003600},
			"secondary_window": {"limit_window_seconds": 604800, "used_percent": "65.5", "reset_after_seconds": 3600}
		},
		"credits": {"balance": "12.50", "unlimited": false, "has_credits": true}
	}`
	usage, err := DecodeAccountUsage(protocol.AuthenticationChatGptOAuth, protocol.PluginHttpResponse{
		Status: 200,
		Body:   body,
	}, now)
	if err != nil {
		t.Fatalf("DecodeAccountUsage: %v", err)
	}
	if len(usage.Windows) != 2 {
		t.Fatalf("windows len = %d, want 2", len(usage.Windows))
	}
	if usage.Windows[0].Key != "five_hour" || *usage.Windows[0].Used != 32 || *usage.Windows[0].ResetMs != 1700003600000 {
		t.Fatalf("window[0] = %+v", usage.Windows[0])
	}
	if usage.Windows[1].Key != "seven_day" || *usage.Windows[1].Used != 65.5 || *usage.Windows[1].ResetMs != now.UnixMilli()+3600000 {
		t.Fatalf("window[1] = %+v", usage.Windows[1])
	}
	if usage.Credits == nil || *usage.Credits.Balance != 12.5 || *usage.Credits.Unlimited || !*usage.Credits.Available {
		t.Fatalf("credits = %+v", usage.Credits)
	}
}

func TestDecodeAccountUsageErrors(t *testing.T) {
	t.Parallel()
	now := time.Now()
	cases := []struct {
		name string
		auth protocol.Authentication
		resp protocol.PluginHttpResponse
	}{
		{"http 429", protocol.AuthenticationClaudeOAuth, protocol.PluginHttpResponse{Status: 429, Body: `{"error":{"message":"rate limited"}}`}},
		{"error object in 200", protocol.AuthenticationChatGpt, protocol.PluginHttpResponse{Status: 200, Body: `{"error":"bad token"}`}},
		{"empty json", protocol.AuthenticationClaudeOAuth, protocol.PluginHttpResponse{Status: 200, Body: `{}`}},
		{"negative number", protocol.AuthenticationClaudeOAuth, protocol.PluginHttpResponse{Status: 200, Body: `{"five_hour":{"utilization":-5}}`}},
		{"bad string number", protocol.AuthenticationChatGpt, protocol.PluginHttpResponse{Status: 200, Body: `{"credits":{"balance":"12e3"}}`}},
		{"non-object", protocol.AuthenticationChatGpt, protocol.PluginHttpResponse{Status: 200, Body: `[]`}},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			if _, err := DecodeAccountUsage(tc.auth, tc.resp, now); err == nil {
				t.Fatalf("expected error for %s", tc.name)
			}
		})
	}
}
