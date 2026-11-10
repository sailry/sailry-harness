package store

import (
	"context"
	"database/sql"
	"math"
	"strings"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func providerQuotaMatches(node protocol.NodeID, saved providerConfiguration, quota protocol.ConversationQuotaSnapshot) bool {
	provider := saved.Provider
	if provider == nil || !saved.Listed || !provider.Enabled || provider.Authentication != protocol.AuthenticationChatGpt ||
		provider.Api != protocol.ConversationModelApiResponses || strings.TrimRight(provider.Endpoint, "/") != "https://api.openai.com/v1" ||
		quota.Provider != provider.ID || quota.Credential.Node != node || provider.Credential == nil || *provider.Credential != quota.Credential {
		return false
	}
	credential, exists := saved.Authorizations[quota.Credential.ID]
	return exists && credential.Authentication == protocol.AuthenticationChatGpt && credential.Key != nil && credential.Revision == quota.Revision
}

func validateProviderQuota(provider protocol.ProviderID, quota protocol.ConversationQuotaSnapshot) error {
	if quota.Provider != provider || quota.Revision == 0 || quota.Revision > math.MaxInt64 || quota.ObservedMs > math.MaxInt64 {
		return invalid("invalid provider quota observation")
	}
	for _, window := range []*protocol.ConversationQuotaWindow{quota.FiveHour, quota.SevenDay} {
		if window != nil && window.ResetAtMs != nil && (*window.ResetAtMs == 0 || *window.ResetAtMs > math.MaxInt64) {
			return invalid("invalid provider quota observation")
		}
	}
	return nil
}

func providerQuotaPresent(quota protocol.ConversationQuotaSnapshot) bool {
	return quota.FiveHour != nil || quota.SevenDay != nil
}

func ReconcileProviderQuotas(ctx context.Context, db *sql.Conn, node protocol.NodeID) ([]protocol.Event, error) {
	values, err := readBodies[providerConfiguration](ctx, db, "SELECT body FROM providers ORDER BY rowid")
	if err != nil {
		return nil, err
	}
	changes := []protocol.Event{}
	for _, saved := range values {
		if saved.Quota == nil || providerQuotaMatches(node, saved, *saved.Quota) {
			continue
		}
		visible := providerQuotaPresent(*saved.Quota)
		saved.Quota = nil
		if err := saveProviderConfiguration(ctx, db, saved); err != nil {
			return nil, err
		}
		if visible {
			changes = append(changes, protocol.Event{Kind: "provider_quota_changed", Data: protocol.EventProviderQuotaChangedPayload{Provider: saved.ID}})
		}
	}
	return changes, nil
}
