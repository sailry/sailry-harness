package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"math"
	"math/big"
	"slices"
	"sort"
	"strings"
	"time"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func readAgentUsage(ctx context.Context, db *sql.Conn, node protocol.NodeID, query protocol.UsageQuery) (protocol.UsageReport, error) {
	report := protocol.DefaultUsageReport()
	report.Node = node
	report.Query = query
	if query.StartMs < 0 || query.EndMs <= query.StartMs || query.EndMs-query.StartMs > 366*86400000 || query.UtcOffsetMinutes > 840 || query.UtcOffsetMinutes < -840 || query.Before != nil && (query.Before.TimestampMs < query.StartMs || query.Before.TimestampMs >= query.EndMs) || len(query.Projects) > 128 || len(query.Worktrees) > 128 || len(query.Providers) > 128 || len(query.Models) > 128 {
		return report, invalid("usage time range or filters exceed the query limits")
	}
	for _, model := range query.Models {
		if strings.TrimSpace(model) == "" || len(model) > 256 {
			return report, invalid("usage time range or filters exceed the query limits")
		}
	}
	if err := db.QueryRowContext(ctx, "SELECT coalesce(max(cursor),0) FROM events").Scan(&report.Cursor); err != nil {
		return report, err
	}
	offset := int64(query.UtcOffsetMinutes) * 60000
	report.Days = usageBuckets(query.StartMs, query.EndMs, 86400000, offset)
	recentStart := max(query.StartMs, query.EndMs-2*86400000)
	report.Recent = usageBuckets(recentStart, query.EndMs, 1800000, offset)
	projects, _ := encode(query.Projects)
	worktrees, _ := encode(query.Worktrees)
	providers, _ := encode(query.Providers)
	models, _ := encode(query.Models)
	if query.Projects == nil {
		projects = []byte("[]")
	}
	if query.Worktrees == nil {
		worktrees = []byte("[]")
	}
	if query.Providers == nil {
		providers = []byte("[]")
	}
	if query.Models == nil {
		models = []byte("[]")
	}
	rows, err := db.QueryContext(ctx, agentUsageRows, time.UnixMilli(query.StartMs).UTC().Format("2006-01-02T15:04:05"), time.UnixMilli(query.EndMs).UTC().Format("2006-01-02T15:04:05"), string(projects), string(worktrees), string(providers), string(models), 100001)
	if err != nil {
		return report, err
	}
	type usageRow struct {
		time                          string
		tokens                        protocol.ConversationUsage
		provider, model               string
		project                       sql.NullString
		worktree                      string
		api, authentication, endpoint sql.NullString
		usage                         string
		elapsed                       sql.NullInt64
		media                         sql.NullString
		sequence                      uint64
		session, turn                 string
		first                         sql.NullInt64
		providerName                  sql.NullString
		scope                         string
		resource, assistant           sql.NullString
	}
	captured := []usageRow{}
	for rows.Next() {
		if len(captured) == 100000 {
			rows.Close()
			return report, invalid("usage report exceeds the row limit; narrow the time range or filters")
		}
		var row usageRow
		if err = rows.Scan(&row.time, &row.tokens.Input, &row.tokens.Output, &row.tokens.CachedInput, &row.tokens.Reasoning, &row.provider, &row.model, &row.project, &row.worktree, &row.api, &row.authentication, &row.endpoint, &row.usage, &row.elapsed, &row.media, &row.sequence, &row.session, &row.turn, &row.first, &row.providerName, &row.scope, &row.resource, &row.assistant); err != nil {
			rows.Close()
			return report, err
		}
		captured = append(captured, row)
	}
	if err = errors.Join(rows.Err(), rows.Close()); err != nil {
		return report, err
	}
	groups := map[string]*protocol.UsageGroup{}
	resources := map[string]protocol.UsageKey{}
	requests := []protocol.UsageRequest{}
	prices := modelPrices{}
	for _, row := range captured {
		created, e := time.Parse(time.RFC3339Nano, row.time)
		if e != nil {
			return report, e
		}
		ms := created.UnixMilli()
		if ms < query.StartMs || ms >= query.EndMs {
			continue
		}
		provider, e := protocol.ParseProviderID(row.provider)
		if e != nil {
			return report, e
		}
		worktree, e := protocol.ParseWorktreeID(row.worktree)
		if e != nil {
			return report, e
		}
		session, e := protocol.ParseSessionID(row.session)
		if e != nil {
			return report, e
		}
		turn, e := protocol.ParseTurnID(row.turn)
		if e != nil {
			return report, e
		}
		var project *protocol.ProjectID
		if row.project.Valid {
			id, e := protocol.ParseProjectID(row.project.String)
			if e != nil {
				return report, e
			}
			project = &id
		}
		owner := protocol.UsageKey{Kind: "session", Data: session}
		location := owner
		if project != nil {
			owner = protocol.UsageKey{Kind: "project", Data: *project}
			location = protocol.UsageKey{Kind: "worktree", Data: protocol.UsageKeyWorktreePayload{Project: *project, Worktree: worktree}}
		} else if row.resource.Valid {
			var resource protocol.ConnectionResource
			if e = json.Unmarshal([]byte(row.resource.String), &resource); e != nil {
				return report, e
			}
			owner = protocol.UsageKey{Kind: "connection", Data: resource}
			location = owner
		} else if row.assistant.Valid {
			var assistant protocol.PluginConversationBinding
			if e = json.Unmarshal([]byte(row.assistant.String), &assistant); e != nil {
				return report, e
			}
			owner = protocol.UsageKey{Kind: "assistant", Data: protocol.UsageKeyAssistantPayload{Package: assistant.Package.Name, ID: assistant.ID}}
			location = owner
		}
		keys := []protocol.UsageKey{{Kind: "provider", Data: provider}, {Kind: "model", Data: protocol.UsageKeyModelPayload{Provider: provider, Model: row.model}}, owner, location}
		index := 0
		switch query.Dimension {
		case protocol.UsageDimensionModel:
			index = 1
		case protocol.UsageDimensionProject:
			index = 2
		case protocol.UsageDimensionWorktree:
			index = 3
		}
		key := keys[index]
		keyText := usageKeyText(key)
		group := groups[keyText]
		if group == nil {
			g := protocol.DefaultUsageGroup()
			g.Key = key
			group = &g
			groups[keyText] = group
		}
		name := row.provider
		if row.providerName.Valid {
			name = row.providerName.String
		}
		if index == 2 {
			name = row.scope
		}
		if index < 3 && name != "" {
			v := name
			group.Name = &v
		}
		for _, key := range keys {
			resources[usageKeyText(key)] = key
		}
		if len(groups) > 4096 || len(resources) > 4096*4 {
			return report, invalid("usage report exceeds the group or resource limit; narrow the filters")
		}
		var usage canonical.Usage
		if e = json.Unmarshal([]byte(row.usage), &usage); e != nil {
			return report, e
		}
		var cost *protocol.UsageCost
		if !row.media.Valid || row.media.String != "image" && row.media.String != "video" {
			cost, e = estimateAgentCost(ctx, db, prices, row.api.String, row.model, usage)
			if e != nil {
				return report, e
			}
		}
		var elapsed *uint64
		if row.elapsed.Valid && row.elapsed.Int64 >= 0 {
			v := uint64(row.elapsed.Int64)
			elapsed = &v
		}
		if e = addUsageMetrics(&report.Totals, row.tokens, cost, elapsed); e != nil {
			return report, e
		}
		if e = addUsageMetrics(&group.Metrics, row.tokens, cost, elapsed); e != nil {
			return report, e
		}
		day := int((usageFloor(ms, 86400000, offset) - report.Days[0].StartMs) / 86400000)
		if e = addUsageMetrics(&report.Days[day].Metrics, row.tokens, cost, elapsed); e != nil {
			return report, e
		}
		addUsageModel(&report.Days[day], row.model, row.tokens)
		if ms >= recentStart {
			bucket := int((usageFloor(ms, 1800000, offset) - report.Recent[0].StartMs) / 1800000)
			if e = addUsageMetrics(&report.Recent[bucket].Metrics, row.tokens, cost, elapsed); e != nil {
				return report, e
			}
			addUsageModel(&report.Recent[bucket], row.model, row.tokens)
		}
		position := protocol.UsagePosition{TimestampMs: ms, Node: node, Sequence: row.sequence}
		if query.Before != nil && (ms > query.Before.TimestampMs || ms == query.Before.TimestampMs && row.sequence >= query.Before.Sequence) {
			continue
		}
		request := protocol.DefaultUsageRequest()
		request.Position = position
		request.Session = session
		request.Turn = turn
		request.Project = project
		request.ScopeName = row.scope
		request.Worktree = worktree
		request.Provider = provider
		request.ProviderName = name
		request.Model = row.model
		request.Tokens = row.tokens
		request.ElapsedUs = elapsed
		if row.first.Valid && row.first.Int64 >= 0 && elapsed != nil && uint64(row.first.Int64) <= *elapsed {
			v := uint64(row.first.Int64)
			request.FirstTokenUs = &v
		}
		if cost != nil {
			request.UsdMicros = &cost.UsdMicros
		}
		requests = append(requests, request)
	}
	sort.Slice(requests, func(i, j int) bool {
		if requests[i].Position.TimestampMs != requests[j].Position.TimestampMs {
			return requests[i].Position.TimestampMs > requests[j].Position.TimestampMs
		}
		return requests[i].Position.Sequence > requests[j].Position.Sequence
	})
	if len(requests) > 20 {
		report.Requests.HasMore = true
		requests = requests[:20]
	}
	titles := map[protocol.SessionID]string{}
	for i := range requests {
		title, known := titles[requests[i].Session]
		if !known {
			full, e := activityTitle(ctx, db, requests[i].Session)
			if e != nil {
				return report, e
			}
			title = shortTitle(full)
			titles[requests[i].Session] = title
		}
		if strings.TrimSpace(title) != "" {
			requests[i].SessionTitle = &title
		}
	}
	report.Requests.Items = requests
	groupKeys := []string{}
	for k := range groups {
		groupKeys = append(groupKeys, k)
	}
	slices.Sort(groupKeys)
	for _, k := range groupKeys {
		report.Groups = append(report.Groups, *groups[k])
	}
	resourceKeys := []string{}
	for k := range resources {
		resourceKeys = append(resourceKeys, k)
	}
	slices.Sort(resourceKeys)
	for _, k := range resourceKeys {
		report.Resources = append(report.Resources, resources[k])
	}
	var updated sql.NullInt64
	err = db.QueryRowContext(ctx, "SELECT json_extract(body,'$.updated_at_ms') FROM model_catalog_status WHERE id=1").Scan(&updated)
	if err != nil && !errors.Is(err, sql.ErrNoRows) {
		return report, err
	}
	if updated.Valid && updated.Int64 >= 0 {
		v := uint64(updated.Int64)
		report.PricingUpdatedAtMs = &v
	}
	encoded, e := encode(report)
	if e != nil {
		return report, e
	}
	if len(encoded) > protocol.MAX_FRAME_BYTES-64*1024 {
		return report, invalid("usage report exceeds the response limit; narrow the filters")
	}
	return report, nil
}
func usageKeyText(key protocol.UsageKey) string {
	order := slices.Index([]string{"provider", "model", "project", "session", "connection", "assistant", "worktree"}, key.Kind)
	body, _ := encode(key.Data)
	return string(rune('0'+order)) + string(body)
}
func usageFloor(ms, width, offset int64) int64 {
	v := ms + offset
	q := v / width
	if v < 0 && v%width != 0 {
		q--
	}
	return q*width - offset
}
func usageBuckets(start, end, width, offset int64) []protocol.UsagePoint {
	out := []protocol.UsagePoint{}
	for t := usageFloor(start, width, offset); t < end; t += width {
		p := protocol.DefaultUsagePoint()
		p.StartMs = t
		out = append(out, p)
	}
	return out
}
func addUsageModel(point *protocol.UsagePoint, model string, value protocol.ConversationUsage) {
	if point.Models == nil {
		point.Models = map[string]protocol.ConversationUsage{}
	}
	v := point.Models[model]
	v.Input += value.Input
	v.Output += value.Output
	v.CachedInput += value.CachedInput
	v.Reasoning += value.Reasoning
	point.Models[model] = v
}
func addUsageMetrics(target *protocol.UsageMetrics, tokens protocol.ConversationUsage, cost *protocol.UsageCost, elapsed *uint64) error {
	target.Responses++
	if target.Tokens == nil {
		target.Tokens = &protocol.ConversationUsage{}
	}
	target.Tokens.Input += tokens.Input
	target.Tokens.Output += tokens.Output
	target.Tokens.CachedInput += tokens.CachedInput
	target.Tokens.Reasoning += tokens.Reasoning
	if elapsed != nil {
		if target.Generation == nil {
			target.Generation = &protocol.UsageGeneration{}
		}
		if math.MaxUint64-target.Generation.ElapsedUs < *elapsed {
			return invalid("usage duration exceeds the counter limit")
		}
		target.Generation.ElapsedUs += *elapsed
		target.Generation.OutputTokens += tokens.Output
		target.Generation.Responses++
	}
	if cost != nil {
		if target.Cost == nil {
			copy := *cost
			if cost.Breakdown != nil {
				b := *cost.Breakdown
				copy.Breakdown = &b
			}
			target.Cost = &copy
		} else {
			if math.MaxUint64-target.Cost.UsdMicros < cost.UsdMicros {
				return invalid("usage cost exceeds the counter limit")
			}
			target.Cost.UsdMicros += cost.UsdMicros
			target.Cost.Responses += cost.Responses
			if target.Cost.Breakdown != nil && cost.Breakdown != nil {
				a, b := target.Cost.Breakdown, cost.Breakdown
				a.Input += b.Input
				a.Output += b.Output
				a.CacheRead += b.CacheRead
				a.CacheWrite += b.CacheWrite
			} else {
				target.Cost.Breakdown = nil
			}
		}
	}
	return nil
}

// modelPrices holds the catalog entries one report has read, keyed by supplier
// and model. An entry the catalog lacks is kept as nil.
type modelPrices map[[2]string][]byte

func estimateAgentCost(ctx context.Context, db *sql.Conn, prices modelPrices, api, model string, usage canonical.Usage) (*protocol.UsageCost, error) {
	supplier := ""
	switch api {
	case "responses", "chat_completions":
		supplier = "openai"
	case "anthropic":
		supplier = "anthropic"
	case "gemini":
		supplier = "google"
	}
	if supplier == "" || usage.Input == nil || usage.Output == nil {
		return nil, nil
	}
	body, known := prices[[2]string{supplier, model}]
	if !known {
		err := db.QueryRowContext(ctx, "SELECT body FROM model_catalog WHERE provider=?1 AND id=?2", supplier, model).Scan(&body)
		if err != nil && !errors.Is(err, sql.ErrNoRows) {
			return nil, err
		}
		prices[[2]string{supplier, model}] = body
	}
	if body == nil {
		return nil, nil
	}
	var object map[string]json.RawMessage
	if json.Unmarshal(body, &object) != nil {
		return nil, nil
	}
	var cost map[string]json.RawMessage
	if json.Unmarshal(object["cost"], &cost) != nil {
		return nil, nil
	}
	if raw, ok := cost["tiers"]; ok {
		var tiers []map[string]json.RawMessage
		if json.Unmarshal(raw, &tiers) != nil {
			return nil, nil
		}
		seen := map[uint64]bool{}
		threshold := uint64(0)
		for _, tier := range tiers {
			var criteria struct {
				Type string `json:"type"`
				Size uint64 `json:"size"`
			}
			if json.Unmarshal(tier["tier"], &criteria) != nil || criteria.Type != "context" || seen[criteria.Size] {
				return nil, nil
			}
			seen[criteria.Size] = true
			if *usage.Input > criteria.Size && criteria.Size >= threshold {
				cost = tier
				threshold = criteria.Size
			}
		}
	} else if *usage.Input > 200000 {
		if raw, ok := cost["context_over_200k"]; ok {
			if json.Unmarshal(raw, &cost) != nil {
				return nil, nil
			}
		}
	}
	if _, ok := cost["input_audio"]; ok {
		return nil, nil
	}
	if _, ok := cost["output_audio"]; ok {
		return nil, nil
	}
	read, write, reasoning := optionalUint(usage.CacheRead), optionalUint(usage.CacheWrite), optionalUint(usage.Reasoning)
	if *usage.Input < read || *usage.Input-read < write || *usage.Output < reasoning {
		return nil, nil
	}
	rate := func(field string) (uint64, bool) {
		var v float64
		if json.Unmarshal(cost[field], &v) != nil || math.IsNaN(v) || math.IsInf(v, 0) || v < 0 || v*1000000 >= float64(math.MaxUint64) {
			return 0, false
		}
		return uint64(math.Round(v * 1000000)), true
	}
	if _, ok := rate("input"); !ok {
		return nil, nil
	}
	if _, ok := rate("output"); !ok {
		return nil, nil
	}
	categories := [4]*big.Int{new(big.Int), new(big.Int), new(big.Int), new(big.Int)}
	for _, item := range []struct {
		count           uint64
		field, fallback string
		category        int
	}{{*usage.Input - read - write, "input", "", 0}, {*usage.Output - reasoning, "output", "", 1}, {read, "cache_read", "", 2}, {write, "cache_write", "", 3}, {reasoning, "reasoning", "output", 1}} {
		if item.count == 0 {
			continue
		}
		r, ok := rate(item.field)
		if !ok && item.fallback != "" {
			r, ok = rate(item.fallback)
		}
		if !ok {
			return nil, nil
		}
		product := new(big.Int).Mul(new(big.Int).SetUint64(r), new(big.Int).SetUint64(item.count))
		categories[item.category].Add(categories[item.category], product)
	}
	cumulative := new(big.Int)
	total := uint64(0)
	amounts := [4]uint64{}
	for i, category := range categories {
		cumulative.Add(cumulative, category)
		rounded := new(big.Int).Add(cumulative, big.NewInt(500000))
		rounded.Div(rounded, big.NewInt(1000000))
		if !rounded.IsUint64() {
			return nil, nil
		}
		next := rounded.Uint64()
		amounts[i] = next - total
		total = next
	}
	return &protocol.UsageCost{UsdMicros: total, Responses: 1, Breakdown: &protocol.UsageCostBreakdown{Input: amounts[0], Output: amounts[1], CacheRead: amounts[2], CacheWrite: amounts[3]}}, nil
}
