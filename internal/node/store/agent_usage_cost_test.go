package store

import (
	"database/sql"
	"encoding/json"
	"math"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestAnthropicCacheWriteCost(t *testing.T) {
	t.Parallel()
	storage, _, _, _, _ := agentHistoryFixture(t, protocol.WorkModeCode)
	ctx := t.Context()
	err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		catalog := []byte(`{"cost":{"input":3,"output":15,"cache_read":0.3,"cache_write":3.75}}`)
		if _, err := db.ExecContext(ctx, "INSERT INTO model_catalog(provider,id,body) VALUES('anthropic','fixture',?1)", catalog); err != nil {
			return err
		}
		for _, test := range []struct {
			name, raw string
			micros    uint64
			write     uint64
		}{
			{"default", `{}`, 768, 150},
			{"five_minutes", `{"cache_creation":{"ephemeral_5m_input_tokens":40,"ephemeral_1h_input_tokens":0}}`, 768, 150},
			{"one_hour", `{"cache_creation":{"ephemeral_5m_input_tokens":0,"ephemeral_1h_input_tokens":40}}`, 858, 240},
			{"mixed", `{"cache_creation":{"ephemeral_5m_input_tokens":10,"ephemeral_1h_input_tokens":30}}`, 836, 218},
			{"incomplete", `{"cache_creation":{"ephemeral_1h_input_tokens":30}}`, 0, 0},
			{"inconsistent", `{"cache_creation":{"ephemeral_5m_input_tokens":20,"ephemeral_1h_input_tokens":30}}`, 0, 0},
		} {
			t.Run(test.name, func(t *testing.T) {
				usage := canonical.Usage{Input: ptr(uint64(200)), Output: ptr(uint64(20)), CacheRead: ptr(uint64(60)), CacheWrite: ptr(uint64(40)), Provider: json.RawMessage(test.raw)}
				cost, err := estimateAgentCost(ctx, db, &modelPrices{}, "anthropic", "anthropic", "fixture", usage, protocol.DefaultBillingRateBps)
				if err != nil {
					t.Fatal(err)
				}
				if test.micros == 0 {
					if cost != nil {
						t.Fatalf("incomplete cache pricing should be unknown: %#v", cost)
					}
					return
				}
				if cost == nil || cost.UsdMicros != test.micros || cost.Breakdown == nil || cost.Breakdown.CacheWrite != test.write {
					t.Fatalf("cost = %#v, want total %d and cache writes %d", cost, test.micros, test.write)
				}
			})
		}
		// The one-hour multiplier applies to the selected context price tier.
		tier := []byte(`{"cost":{"input":3,"output":15,"cache_read":0.3,"cache_write":3.75,"tiers":[{"tier":{"type":"context","size":150},"input":6,"output":30,"cache_read":0.6,"cache_write":7.5}]}}`)
		if _, err := db.ExecContext(ctx, "UPDATE model_catalog SET body=?1 WHERE provider='anthropic' AND id='fixture'", tier); err != nil {
			return err
		}
		usage := canonical.Usage{Input: ptr(uint64(200)), Output: ptr(uint64(20)), CacheRead: ptr(uint64(60)), CacheWrite: ptr(uint64(40)), Provider: json.RawMessage(`{"cache_creation":{"ephemeral_5m_input_tokens":10,"ephemeral_1h_input_tokens":30}}`)}
		cost, err := estimateAgentCost(ctx, db, &modelPrices{}, "anthropic", "anthropic", "fixture", usage, protocol.DefaultBillingRateBps)
		if err != nil {
			return err
		}
		if cost == nil || cost.UsdMicros != 1671 || cost.Breakdown == nil || cost.Breakdown.CacheWrite != 435 {
			t.Fatalf("tiered cost = %#v, want total 1671 and cache writes 435", cost)
		}
		if _, err = db.ExecContext(ctx, "INSERT INTO model_catalog(provider,id,body) VALUES('reseller','fixture',?1)", catalog); err != nil {
			return err
		}
		cost, err = estimateAgentCost(ctx, db, &modelPrices{}, "reseller", "anthropic", "fixture", usage, protocol.DefaultBillingRateBps)
		if err != nil {
			return err
		}
		if cost != nil {
			t.Fatalf("compatible API invented the reseller's one-hour price: %#v", cost)
		}
		// Bedrock bills Claude's one-hour writes as Anthropic does.
		if _, err = db.ExecContext(ctx, "INSERT INTO model_catalog(provider,id,body) VALUES('amazon-bedrock','fixture',?1)", tier); err != nil {
			return err
		}
		cost, err = estimateAgentCost(ctx, db, &modelPrices{}, "bedrock", "bedrock", "fixture", usage, protocol.DefaultBillingRateBps)
		if err != nil {
			return err
		}
		if cost == nil || cost.UsdMicros != 1671 || cost.Breakdown == nil || cost.Breakdown.CacheWrite != 435 {
			// Errorf: a Fatalf here would exit while holding the writer.
			t.Errorf("Bedrock cost = %#v, want total 1671 and cache writes 435", cost)
		}
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
}

func TestAgentCostScalesBeforeRounding(t *testing.T) {
	t.Parallel()
	storage, _, _, _, _ := agentHistoryFixture(t, protocol.WorkModeCode)
	ctx := t.Context()
	err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		for name, body := range map[string]string{
			"fixture":  `{"cost":{"input":0.4,"output":0.4,"cache_read":0.2,"cache_write":0.3}}`,
			"unpriced": `{"cost":{"input":0.4,"output":0.4,"cache_read":null,"cache_write":0.3}}`,
		} {
			if _, err := db.ExecContext(ctx, "INSERT INTO model_catalog(provider,id,body) VALUES('openai',?1,?2)", name, []byte(body)); err != nil {
				return err
			}
		}
		prices := &modelPrices{}
		usage := canonical.Usage{Input: ptr(uint64(3)), Output: ptr(uint64(1)), CacheRead: ptr(uint64(1)), CacheWrite: ptr(uint64(1)), Reasoning: ptr(uint64(1))}
		for _, row := range []struct {
			name   string
			rate   uint32
			micros uint64
			parts  protocol.UsageCostBreakdown
		}{
			{"standard", 10_000, 1, protocol.UsageCostBreakdown{Output: 1}},
			{"markup", 20_000, 3, protocol.UsageCostBreakdown{Input: 1, Output: 1, CacheWrite: 1}},
			{"discount", 5_000, 1, protocol.UsageCostBreakdown{CacheRead: 1}},
			{"free", 0, 0, protocol.UsageCostBreakdown{}},
		} {
			cost, err := estimateAgentCost(ctx, db, prices, "", "responses", "fixture", usage, row.rate)
			if err != nil {
				return err
			}
			if cost == nil || cost.UsdMicros != row.micros || cost.Responses != 1 || cost.Breakdown == nil || *cost.Breakdown != row.parts {
				t.Errorf("%s: cost = %+v, want %d and %+v", row.name, cost, row.micros, row.parts)
			}
		}
		for _, rate := range []uint32{protocol.DefaultBillingRateBps, 0} {
			for _, name := range []string{"unpriced", "missing"} {
				cost, err := estimateAgentCost(ctx, db, prices, "", "responses", name, usage, rate)
				if err != nil {
					return err
				}
				if cost != nil {
					t.Errorf("%s at %d bps: unknown price became %+v", name, rate, cost)
				}
			}
		}
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
}

func TestAgentCostRateRange(t *testing.T) {
	t.Parallel()
	for _, row := range []struct {
		products uint128
		rate     uint32
		want     uint64
		fits     bool
	}{
		{uint128{lo: 1_000_000}, math.MaxUint32, 429_497, true},
		{uint128{hi: 1_000_000}, 10_000, 0, false},
		{uint128{hi: 1_000_000}, 5_000, 1 << 63, true},
		{uint128{hi: math.MaxUint64}, math.MaxUint32, 0, false},
	} {
		got, fits := row.products.micros(row.rate)
		if got != row.want || fits != row.fits {
			t.Errorf("%+v * %d bps = %d, %t; want %d, %t", row.products, row.rate, got, fits, row.want, row.fits)
		}
	}
}

func TestAgentCostRecognizedModelNamespace(t *testing.T) {
	t.Parallel()
	storage, _, _, _, _ := agentHistoryFixture(t, protocol.WorkModeCode)
	ctx := t.Context()
	err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		put := func(supplier, model, price string) error {
			_, err := db.ExecContext(ctx, "INSERT INTO model_catalog(provider,id,body) VALUES(?1,?2,?3)", supplier, model, []byte(price))
			return err
		}
		if err := put("anthropic", "claude-fable-5", `{"cost":{"input":10,"output":50}}`); err != nil {
			return err
		}
		if err := put("openrouter", "claude-fable-5", `{"cost":{"input":1,"output":2}}`); err != nil {
			return err
		}
		usage := canonical.Usage{Input: ptr(uint64(1)), Output: ptr(uint64(1))}
		check := func(preset, model string, want *uint64) error {
			cost, err := estimateAgentCost(ctx, db, &modelPrices{}, preset, "chat_completions", model, usage, protocol.DefaultBillingRateBps)
			if err != nil {
				return err
			}
			if (cost == nil) != (want == nil) || want != nil && cost != nil && cost.UsdMicros != *want {
				t.Errorf("%s / %s: cost = %+v, want %v", preset, model, cost, want)
			}
			return nil
		}
		for name, want := range map[string]*uint64{
			"claude/claude-fable-5":       ptr(uint64(60)),
			"anthropic/claude-fable-5":    ptr(uint64(60)),
			"gateway/claude-fable-5":      nil,
			"claude/other/claude-fable-5": nil,
		} {
			if err := check("openrouter", name, want); err != nil {
				return err
			}
		}
		const exact = "anthropic/claude-fable-5"
		if err := put("openrouter", exact, `{"cost":{"input":12,"output":55}}`); err != nil {
			return err
		}
		if err := check("", exact, ptr(uint64(67))); err != nil {
			return err
		}
		if err := put("gateway", exact, `{"cost":{"input":14,"output":55}}`); err != nil {
			return err
		}
		if err := check("", exact, nil); err != nil {
			return err
		}
		if err := check("openrouter", exact, ptr(uint64(67))); err != nil {
			return err
		}
		if err := put("gateway", "claude/claude-fable-5", `{"limit":{"context":200000}}`); err != nil {
			return err
		}
		return check("", "claude/claude-fable-5", nil)
	})
	if err != nil {
		t.Fatal(err)
	}
}

// The report query keeps only cache_creation of the provider's usage; the
// trimmed object must still decode to the same cache-write split.
func TestUsageQueryKeepsCacheCreation(t *testing.T) {
	t.Parallel()
	storage, _, _, _, _ := agentHistoryFixture(t, protocol.WorkModeCode)
	ctx := t.Context()
	trim := "SELECT " + strings.ReplaceAll(agentUsageTrimmed, "e.usage", "?1")
	err := storage.Node.Read(ctx, func(db *sql.Conn) error {
		for raw, want := range map[string]string{
			`{"input":1,"provider":{"cache_creation":{"ephemeral_5m_input_tokens":10,"ephemeral_1h_input_tokens":30},"id":"msg","content":"large"}}`: `{"ephemeral_5m_input_tokens":10,"ephemeral_1h_input_tokens":30}`,
			`{"input":1,"provider":{"id":"msg"}}`: `null`,
			`{"input":1}`:                         ``,
			`{"input":1,"provider":{"cache_creation":"unavailable"}}`: `"unavailable"`,
			`{"input":1,"provider":{"cache_creation":"{}"}}`:          `"{}"`,
		} {
			var got string
			if err := db.QueryRowContext(ctx, trim, raw).Scan(&got); err != nil {
				return err
			}
			var usage canonical.Usage
			if err := json.Unmarshal([]byte(got), &usage); err != nil {
				t.Fatalf("%s: %v", got, err)
			}
			var provider struct {
				Creation json.RawMessage `json:"cache_creation"`
			}
			if len(usage.Provider) > 0 {
				if err := json.Unmarshal(usage.Provider, &provider); err != nil {
					t.Fatalf("%s: %v", got, err)
				}
			}
			if string(provider.Creation) != want || *usage.Input != 1 {
				t.Fatalf("%s trimmed to %s", raw, got)
			}
		}
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
}
