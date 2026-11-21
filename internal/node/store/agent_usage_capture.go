package store

import (
	"context"
	"database/sql"
	"encoding/json"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func captureAgentUsagePricing(ctx context.Context, db *sql.Conn, record *canonical.Record, provider *protocol.ConversationProvider, previous []byte) error {
	if provider == nil {
		return nil
	}
	message := record.MutableMessage()
	if message == nil || message.Usage == nil || message.Usage.Input == nil || message.Usage.Output == nil || message.Usage.Pricing != nil {
		return nil
	}
	execution := record.Metadata.Execution
	if execution == nil || execution.Source == nil || execution.Source.ID != provider.ID || execution.Source.API != provider.Api || execution.Source.Authentication != provider.Authentication {
		return invalid("recorded usage does not match the captured provider")
	}
	var pricing *canonical.UsagePricing
	if previous != nil {
		// An idempotent append must compare with the original estimate, not
		// a new one from today's catalog.
		type storedUsage struct {
			Pricing *canonical.UsagePricing `json:"pricing"`
		}
		var stored struct {
			Usage   storedUsage `json:"usage"`
			Summary struct {
				Usage storedUsage `json:"usage"`
			} `json:"summary"`
		}
		if err := json.Unmarshal(previous, &stored); err != nil {
			return invalid("invalid stored conversation record")
		}
		pricing = stored.Usage.Pricing
		if record.Kind == "compaction" {
			pricing = stored.Summary.Usage.Pricing
		}
		if pricing == nil {
			return nil
		}
	} else {
		pricing = &canonical.UsagePricing{}
		if execution.Media == nil || tokenPricedMedia(string(*execution.Media)) {
			preset := ""
			if provider.Preset != nil && provider.Preset.Source.Kind == "builtin" {
				preset = provider.Preset.ID
			}
			rate := protocol.DefaultBillingRateBps
			if provider.Authentication == protocol.AuthenticationApiKey && provider.BillingRateBps != nil {
				rate = *provider.BillingRateBps
			}
			estimate, err := estimateAgentCost(ctx, db, &modelPrices{}, preset, string(provider.Api), execution.Source.Model, *message.Usage, rate)
			if err != nil {
				return err
			}
			pricing.Estimate = estimate
		}
	}
	usage, reply := *message.Usage, *message
	usage.Pricing, reply.Usage = pricing, &usage
	if record.Kind == "compaction" {
		compaction := *record.Data.(*canonical.Compaction)
		compaction.Summary = reply
		record.Data = &compaction
	} else {
		record.Data = &reply
	}
	return nil
}
