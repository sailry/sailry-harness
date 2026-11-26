package providerstatusnode

import (
	"context"
	"slices"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/node/moduleapi"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type fakeProviderHost struct {
	moduleapi.Host
	now  time.Time
	read func(provider protocol.ProviderID, expectedRev uint64, namespace string, resolve func(protocol.Authentication) (protocol.PluginModelAuthEndpoint, bool)) (protocol.Authentication, protocol.PluginHttpResponse, error)
}

func (h *fakeProviderHost) Now() time.Time { return h.now }

func (h *fakeProviderHost) ReadProviderAccountHTTP(_ context.Context, provider protocol.ProviderID, expectedRev uint64, namespace string, resolve func(protocol.Authentication) (protocol.PluginModelAuthEndpoint, bool)) (protocol.Authentication, protocol.PluginHttpResponse, error) {
	return h.read(provider, expectedRev, namespace, resolve)
}

func TestProviderStatusModuleAndCommands(t *testing.T) {
	t.Parallel()
	m := Module()
	if m.ID() != "provider-status" || !slices.Equal(m.ReplacesPlugins(), []string{"provider-status"}) {
		t.Fatalf("identity mismatch: id=%q replaces=%v", m.ID(), m.ReplacesPlugins())
	}
	if len(m.Catalog()) != 0 {
		t.Fatalf("expected empty tool catalog, got %+v", m.Catalog())
	}
	tools, err := m.ToolsForTurn(t.Context(), nil, nil)
	if err != nil || len(tools) != 0 {
		t.Fatalf("ToolsForTurn = %v, %v", tools, err)
	}
	ch := m.(moduleapi.CommandHandler)
	if !ch.HandlesCommand("read_provider_account_usage") || ch.HandlesCommand("other") {
		t.Fatal("HandlesCommand mismatch")
	}

	var local protocol.NodeID
	local[0] = 1
	provID := protocol.NewProviderID()
	now := time.UnixMilli(1_700_000_000_000)

	// Invalid nil payload fails cleanly.
	if _, _, err := ch.ExecuteCommand(t.Context(), &fakeProviderHost{now: now}, local, protocol.NewRequest(local, protocol.Command{
		Kind: "read_provider_account_usage",
	})); err == nil {
		t.Fatal("expected error for nil payload")
	}

	// Successful Claude OAuth usage command execution verifies EndpointFor + cache TTL + decoding.
	host := &fakeProviderHost{
		now: now,
		read: func(provider protocol.ProviderID, expectedRev uint64, namespace string, resolve func(protocol.Authentication) (protocol.PluginModelAuthEndpoint, bool)) (protocol.Authentication, protocol.PluginHttpResponse, error) {
			if provider != provID || expectedRev != 7 || namespace != "provider-status" {
				t.Fatalf("unexpected args: provider=%s rev=%d ns=%q", provider, expectedRev, namespace)
			}
			ep, ok := resolve(protocol.AuthenticationClaudeOAuth)
			if !ok || ep.Url != "https://api.anthropic.com/api/oauth/usage" || ep.CacheTtlMs == nil || *ep.CacheTtlMs != UsageCacheTTLMs {
				t.Fatalf("unexpected Claude endpoint: %+v (ok=%v)", ep, ok)
			}
			if _, ok := resolve(protocol.AuthenticationApiKey); ok {
				t.Fatal("api_key auth should not resolve an account usage endpoint")
			}
			return protocol.AuthenticationClaudeOAuth, protocol.PluginHttpResponse{
				Status: 200,
				Body:   `{"five_hour":{"utilization":25,"resets_at":"2026-04-15T15:30:00Z"}}`,
			}, nil
		},
	}
	out, events, err := ch.ExecuteCommand(t.Context(), host, local, protocol.NewRequest(local, protocol.Command{
		Kind: "read_provider_account_usage",
		Data: &protocol.CommandReadProviderAccountUsagePayload{Provider: provID, ExpectedRevision: 7},
	}))
	if err != nil {
		t.Fatalf("ExecuteCommand: %v", err)
	}
	if len(events) != 0 || out.Kind != "provider_account_usage" {
		t.Fatalf("output = %+v, events = %+v", out, events)
	}
	usage := out.Data.(protocol.ProviderAccountUsage)
	if len(usage.Windows) != 1 || usage.Windows[0].Key != "five_hour" || *usage.Windows[0].Used != 25 {
		t.Fatalf("usage = %+v", usage)
	}

	// Host error propagates unchanged.
	hostErr := &protocol.Fault{Code: protocol.ErrorCodeRevisionConflict, Message: "provider changed"}
	host.read = func(protocol.ProviderID, uint64, string, func(protocol.Authentication) (protocol.PluginModelAuthEndpoint, bool)) (protocol.Authentication, protocol.PluginHttpResponse, error) {
		return "", protocol.PluginHttpResponse{}, hostErr
	}
	if _, _, err := ch.ExecuteCommand(t.Context(), host, local, protocol.NewRequest(local, protocol.Command{
		Kind: "read_provider_account_usage",
		Data: &protocol.CommandReadProviderAccountUsagePayload{Provider: provID, ExpectedRevision: 7},
	})); err != hostErr {
		t.Fatalf("expected propagated fault, got %v", err)
	}
}
