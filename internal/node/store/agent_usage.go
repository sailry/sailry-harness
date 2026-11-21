package store

import (
	"bytes"
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"maps"
	"math"
	"math/bits"
	"slices"
	"sort"
	"strings"
	"sync"
	"time"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/providers"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// usageRequestPage is the protocol's bound on the requests of one report page.
const usageRequestPage = 20

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
	user, err := CurrentUser(ctx, db)
	if err != nil {
		return report, err
	}
	index := 0
	switch query.Dimension {
	case protocol.UsageDimensionModel:
		index = 1
	case protocol.UsageDimensionProject:
		index = 2
	case protocol.UsageDimensionWorktree:
		index = 3
	}
	rows, err := db.QueryContext(ctx, agentUsageRows, time.UnixMilli(query.StartMs).UTC().Format("2006-01-02T15:04:05"), time.UnixMilli(query.EndMs).UTC().Format("2006-01-02T15:04:05"), string(projects), string(worktrees), string(providers), string(models), 100001, user.ID.String())
	if err != nil {
		return report, err
	}
	// Rows are aggregated as they are read. SQLite runs the price lookups
	// beside the open statement on the same connection.
	defer rows.Close()
	var (
		row     usageRow
		stored  canonical.Usage
		usage   sql.RawBytes
		scanned int
	)
	groups := map[string]*protocol.UsageGroup{}
	resources := map[string]protocol.UsageKey{}
	scopes := map[usageScopeKey]*usageScope{}
	latest := usageLatest{items: []protocol.UsageRequest{}}
	prices := &modelPrices{}
	for rows.Next() {
		if scanned == 100000 {
			return report, invalid("usage report exceeds the row limit; narrow the time range or filters")
		}
		scanned++
		if err = rows.Scan(&row.time, &row.scope.provider, &row.model, &row.scope.project, &row.scope.worktree, &row.api, &row.authentication, &row.endpoint, &usage, &row.elapsed, &row.media, &row.sequence, &row.scope.session, &row.turn, &row.first, &row.reasoningStreamed, &row.providerName, &row.scopeName, &row.scope.resource, &row.scope.assistant, &row.preset); err != nil {
			return report, err
		}
		created, e := time.Parse(time.RFC3339Nano, row.time)
		if e != nil {
			return report, e
		}
		ms := created.UnixMilli()
		if ms < query.StartMs || ms >= query.EndMs {
			continue
		}
		row.scope.model = row.model
		scope := scopes[row.scope]
		if scope == nil {
			if scope, e = newUsageScope(row.scope); e != nil {
				return report, e
			}
			scopes[row.scope] = scope
			for i, key := range scope.keys {
				resources[scope.texts[i]] = key
			}
		}
		turn, e := protocol.ParseTurnID(row.turn)
		if e != nil {
			return report, e
		}
		keyText := scope.texts[index]
		group := groups[keyText]
		if group == nil {
			g := protocol.DefaultUsageGroup()
			g.Key = scope.keys[index]
			group = &g
			groups[keyText] = group
		}
		providerName := row.scope.provider
		if row.providerName.Valid {
			providerName = row.providerName.String
		}
		name := providerName
		if index == 2 {
			name = row.scopeName
		}
		// The group is named after its latest response.
		if index < 3 && name != "" && (group.Name == nil || *group.Name != name) {
			v := name
			group.Name = &v
		}
		if len(groups) > 4096 || len(resources) > 4096*4 {
			return report, invalid("usage report exceeds the group or resource limit; narrow the filters")
		}
		// The scanned bytes are decoded without a copy; only the provider body
		// is retained.
		stored = canonical.Usage{}
		if e = stored.UnmarshalJSON(usage); e != nil {
			return report, e
		}
		cost, e := readAgentCost(ctx, db, prices, row.media.String, row.preset.String, row.api.String, row.model, stored)
		if e != nil {
			return report, e
		}
		tokens := agentUsageTokens(stored)
		var complete *protocol.ConversationUsage
		if stored.Input != nil && stored.Output != nil {
			complete = &tokens
		}
		elapsed, elapsedKnown := usageMicros(row.elapsed)
		first, firstKnown := usageMicros(row.first)
		var generation *protocol.UsageGeneration
		if stored.Output != nil && elapsedKnown && firstKnown {
			generation = tokens.DecodeGeneration(&elapsed, &first, row.reasoningStreamed)
		}
		if e = addUsageMetrics(&report.Totals, complete, cost, generation); e != nil {
			return report, e
		}
		if e = addUsageMetrics(&group.Metrics, complete, cost, generation); e != nil {
			return report, e
		}
		day := int((usageFloor(ms, 86400000, offset) - report.Days[0].StartMs) / 86400000)
		if e = addUsageMetrics(&report.Days[day].Metrics, complete, cost, generation); e != nil {
			return report, e
		}
		if complete != nil {
			addUsageModel(&report.Days[day], row.model, tokens)
		}
		if ms >= recentStart {
			bucket := int((usageFloor(ms, 1800000, offset) - report.Recent[0].StartMs) / 1800000)
			if e = addUsageMetrics(&report.Recent[bucket].Metrics, complete, cost, generation); e != nil {
				return report, e
			}
			if complete != nil {
				addUsageModel(&report.Recent[bucket], row.model, tokens)
			}
		}
		position := protocol.UsagePosition{TimestampMs: ms, Node: node, Sequence: row.sequence}
		if before := query.Before; before != nil && (ms > before.TimestampMs || ms == before.TimestampMs &&
			(bytes.Compare(node[:], before.Node[:]) > 0 || node == before.Node && row.sequence >= before.Sequence)) {
			continue
		}
		if !latest.admits(position) {
			continue
		}
		request := protocol.DefaultUsageRequest()
		request.Position = position
		request.Session = scope.session
		request.Turn = turn
		request.Project = scope.project
		request.ScopeName = row.scopeName
		request.Worktree = scope.worktree
		request.Provider = scope.provider
		request.ProviderName = providerName
		request.Model = row.model
		request.Tokens = tokens
		request.MissingInputTokens, request.MissingOutputTokens = stored.Input == nil, stored.Output == nil
		// Copies escape only for the requests a page keeps.
		if elapsedKnown {
			v := elapsed
			request.ElapsedUs = &v
		}
		if firstKnown && (!elapsedKnown || first <= elapsed) {
			v := first
			request.FirstTokenUs = &v
		}
		if cost != nil {
			request.UsdMicros = &cost.UsdMicros
		}
		latest.add(request)
	}
	if err = errors.Join(rows.Err(), rows.Close()); err != nil {
		return report, err
	}
	requests := latest.items
	sort.Slice(requests, func(i, j int) bool { return usageNewer(requests[i].Position, requests[j].Position) })
	report.Requests.HasMore = latest.offered > usageRequestPage
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

// usageRow holds the columns of the response being aggregated.
type usageRow struct {
	time                          string
	model                         string
	scope                         usageScopeKey
	api, authentication, endpoint sql.NullString
	elapsed                       sql.NullInt64
	media                         sql.NullString
	sequence                      uint64
	turn                          string
	first                         sql.NullInt64
	reasoningStreamed             bool
	providerName                  sql.NullString
	scopeName                     string
	preset                        sql.NullString
}

// usageScopeKey is the stored identity from which a response's report keys
// are derived.
type usageScopeKey struct {
	provider, model, worktree, session string
	project, resource, assistant       sql.NullString
}

// usageScope is the parsed identity and report keys of a usageScopeKey: its
// provider, model, owner and location keys and their sort texts.
type usageScope struct {
	provider protocol.ProviderID
	worktree protocol.WorktreeID
	session  protocol.SessionID
	project  *protocol.ProjectID
	keys     [4]protocol.UsageKey
	texts    [4]string
}

func newUsageScope(key usageScopeKey) (*usageScope, error) {
	scope := &usageScope{}
	var err error
	if scope.provider, err = protocol.ParseProviderID(key.provider); err != nil {
		return nil, err
	}
	if scope.worktree, err = protocol.ParseWorktreeID(key.worktree); err != nil {
		return nil, err
	}
	if scope.session, err = protocol.ParseSessionID(key.session); err != nil {
		return nil, err
	}
	if key.project.Valid {
		id, err := protocol.ParseProjectID(key.project.String)
		if err != nil {
			return nil, err
		}
		scope.project = &id
	}
	owner := protocol.UsageKey{Kind: "session", Data: scope.session}
	location := owner
	if scope.project != nil {
		owner = protocol.UsageKey{Kind: "project", Data: *scope.project}
		location = protocol.UsageKey{Kind: "worktree", Data: protocol.UsageKeyWorktreePayload{Project: *scope.project, Worktree: scope.worktree}}
	} else if key.resource.Valid {
		var resource protocol.ConnectionResource
		if err = json.Unmarshal([]byte(key.resource.String), &resource); err != nil {
			return nil, err
		}
		owner = protocol.UsageKey{Kind: "connection", Data: resource}
		location = owner
	} else if key.assistant.Valid {
		var assistant protocol.PluginConversationBinding
		if err = json.Unmarshal([]byte(key.assistant.String), &assistant); err != nil {
			return nil, err
		}
		owner = protocol.UsageKey{Kind: "assistant", Data: protocol.UsageKeyAssistantPayload{Package: assistant.Package.Name, ID: assistant.ID}}
		location = owner
	}
	scope.keys = [4]protocol.UsageKey{{Kind: "provider", Data: scope.provider}, {Kind: "model", Data: protocol.UsageKeyModelPayload{Provider: scope.provider, Model: key.model}}, owner, location}
	for i, key := range scope.keys {
		scope.texts[i] = usageKeyText(key)
	}
	return scope, nil
}

// usageLatest keeps the newest requests before a page boundary; the protocol
// returns at most one page of them.
type usageLatest struct {
	items   []protocol.UsageRequest
	oldest  int
	offered int
}

// admits counts a request and reports whether it belongs to the page so far.
func (latest *usageLatest) admits(position protocol.UsagePosition) bool {
	latest.offered++
	return len(latest.items) < usageRequestPage || usageNewer(position, latest.items[latest.oldest].Position)
}

func (latest *usageLatest) add(request protocol.UsageRequest) {
	if len(latest.items) < usageRequestPage {
		latest.items = append(latest.items, request)
	} else {
		latest.items[latest.oldest] = request
	}
	for i := range latest.items {
		if usageNewer(latest.items[latest.oldest].Position, latest.items[i].Position) {
			latest.oldest = i
		}
	}
}

func usageNewer(a, b protocol.UsagePosition) bool {
	if a.TimestampMs != b.TimestampMs {
		return a.TimestampMs > b.TimestampMs
	}
	return a.Sequence > b.Sequence
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
func usageTime(value sql.NullInt64) *uint64 {
	if !value.Valid || value.Int64 < 0 {
		return nil
	}
	us := uint64(value.Int64)
	return &us
}

func agentUsageTokens(usage canonical.Usage) protocol.ConversationUsage {
	return protocol.ConversationUsage{Input: optionalUint(usage.Input), Output: optionalUint(usage.Output), CachedInput: optionalUint(usage.CacheRead), Reasoning: optionalUint(usage.Reasoning)}
}

func addUsageMetrics(target *protocol.UsageMetrics, tokens *protocol.ConversationUsage, cost *protocol.UsageCost, generation *protocol.UsageGeneration) error {
	target.Responses++
	if tokens == nil {
		target.IncompleteTokenResponses++
	} else {
		if target.Tokens == nil {
			target.Tokens = &protocol.ConversationUsage{}
		}
		target.Tokens.Input += tokens.Input
		target.Tokens.Output += tokens.Output
		target.Tokens.CachedInput += tokens.CachedInput
		target.Tokens.Reasoning += tokens.Reasoning
	}
	if generation != nil {
		if target.Generation == nil {
			target.Generation = &protocol.UsageGeneration{}
		}
		if math.MaxUint64-target.Generation.ElapsedUs < generation.ElapsedUs {
			return invalid("usage duration exceeds the counter limit")
		}
		target.Generation.ElapsedUs += generation.ElapsedUs
		target.Generation.OutputTokens += generation.OutputTokens
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

func usageMicros(value sql.NullInt64) (uint64, bool) {
	if !value.Valid || value.Int64 < 0 {
		return 0, false
	}
	return uint64(value.Int64), true
}

// modelPrices holds the catalog prices one report has read. Entries are keyed
// by supplier and model; the empty supplier keys the price every supplier
// agrees on, and a price the catalog lacks is kept as nil. Channels memoize
// which entry and supplier price a preset, wire protocol and model.
type modelPrices struct {
	entries  map[[2]string]*catalogPrice
	channels map[[3]string]pricedChannel
}

type pricedChannel struct {
	price    *catalogPrice
	supplier string
}

// catalogSuppliers names the models.dev supplier of a preset or brand that
// models.dev lists under another identifier.
var catalogSuppliers = map[string]string{"gemini": "google", "qwen": "alibaba", "moonshot": "moonshotai", "fireworks": "fireworks-ai", "together": "togetherai", "zhipu": "zhipuai", "doubao": "volcengine"}

var catalogModelNamespaces = map[string]string{
	"anthropic": "anthropic", "claude": "anthropic", "deepseek": "deepseek",
	"google": "google", "openai": "openai", "xai": "xai",
}

var presetBrands = sync.OnceValue(func() map[string]string {
	brands := map[string]string{}
	for _, preset := range providers.Bundled().Builtin {
		if preset.Brand != nil {
			brands[preset.ID] = *preset.Brand
		}
	}
	return brands
})

// priceSuppliers lists, in order, the suppliers whose price applies to a
// response: the vendor of its channel (the preset, then the preset's brand),
// then the vendor its wire protocol belongs to.
func priceSuppliers(preset, api string) []string {
	var suppliers []string
	for _, name := range []string{preset, presetBrands()[preset], map[string]string{"responses": "openai", "chat_completions": "openai", "anthropic": "anthropic", "gemini": "google"}[api]} {
		if alias, ok := catalogSuppliers[name]; ok {
			name = alias
		}
		if name != "" && !slices.Contains(suppliers, name) {
			suppliers = append(suppliers, name)
		}
	}
	return suppliers
}

// channel returns the price of a model on a channel and its supplier.
func (prices *modelPrices) channel(ctx context.Context, db *sql.Conn, preset, api, model string) (*catalogPrice, string, error) {
	key := [3]string{preset, api, model}
	if priced, known := prices.channels[key]; known {
		return priced.price, priced.supplier, nil
	}
	var priced pricedChannel
	for _, supplier := range append(priceSuppliers(preset, api), "") {
		price, err := prices.entry(ctx, db, supplier, model)
		if err != nil {
			return nil, "", err
		}
		if price != nil {
			priced = pricedChannel{price, supplier}
			break
		}
	}
	if priced.price == nil {
		namespace, bare, _ := strings.Cut(model, "/")
		if supplier := catalogModelNamespaces[namespace]; supplier != "" && bare != "" && !strings.Contains(bare, "/") {
			price, err := prices.entry(ctx, db, supplier, bare)
			if err != nil {
				return nil, "", err
			}
			priced = pricedChannel{price, supplier}
		}
	}
	if prices.channels == nil {
		prices.channels = map[[3]string]pricedChannel{}
	}
	prices.channels[key] = priced
	return priced.price, priced.supplier, nil
}

// entry distinguishes a missing identifier from an exact match without a
// usable price. Only a missing identifier permits namespace fallback.
func (prices *modelPrices) entry(ctx context.Context, db *sql.Conn, supplier, model string) (*catalogPrice, error) {
	key := [2]string{supplier, model}
	if price, known := prices.entries[key]; known {
		return price, nil
	}
	var price *catalogPrice
	if supplier == "" {
		var err error
		price, err = agreedPrice(ctx, db, model)
		if err != nil {
			return nil, err
		}
	} else {
		var body []byte
		err := db.QueryRowContext(ctx, "SELECT body FROM model_catalog WHERE provider=?1 AND id=?2", supplier, model).Scan(&body)
		if err == nil {
			price = parseCatalogPrice(body)
		} else if !errors.Is(err, sql.ErrNoRows) {
			return nil, err
		}
	}
	if prices.entries == nil {
		prices.entries = map[[2]string]*catalogPrice{}
	}
	prices.entries[key] = price
	return price, nil
}

func agreedPrice(ctx context.Context, db *sql.Conn, model string) (*catalogPrice, error) {
	rows, err := db.QueryContext(ctx, "SELECT body FROM model_catalog WHERE id=?1 ORDER BY provider", model)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	var agreed []byte
	price := ""
	matched := false
	for rows.Next() {
		matched = true
		var body []byte
		if err = rows.Scan(&body); err != nil {
			return nil, err
		}
		var entry struct {
			Cost map[string]any `json:"cost"`
		}
		if json.Unmarshal(body, &entry) != nil || entry.Cost == nil {
			continue
		}
		// Encoding a decoded object orders its keys, so equal prices compare equal.
		cost, _ := json.Marshal(entry.Cost)
		if agreed != nil && string(cost) != price {
			return &catalogPrice{}, rows.Err()
		}
		agreed, price = body, string(cost)
	}
	if err := rows.Err(); err != nil {
		return nil, err
	}
	if agreed != nil {
		return parseCatalogPrice(agreed), nil
	}
	if matched {
		return &catalogPrice{}, nil
	}
	return nil, nil
}

// The rates a catalog price states, by the field that names them.
const (
	rateInput = iota
	rateOutput
	rateCacheRead
	rateCacheWrite
	rateReasoning
	rateCount
)

var rateFields = [rateCount]string{"input", "output", "cache_read", "cache_write", "reasoning"}

// priceRates is one set of catalog rates in micro-USD per million tokens. A
// rate that is absent or not a usable number is unknown.
type priceRates struct {
	rate  [rateCount]uint64
	known [rateCount]bool
	// audio marks a price with audio rates, which usage does not separate.
	audio bool
}

func parseRates(cost map[string]json.RawMessage) priceRates {
	var rates priceRates
	_, input := cost["input_audio"]
	_, output := cost["output_audio"]
	rates.audio = input || output
	for i, field := range rateFields {
		var v *float64
		if json.Unmarshal(cost[field], &v) != nil || v == nil || math.IsNaN(*v) || math.IsInf(*v, 0) || *v < 0 || *v*1000000 >= float64(math.MaxUint64) {
			continue
		}
		rates.rate[i], rates.known[i] = uint64(math.Round(*v*1000000)), true
	}
	return rates
}

// catalogPrice is a catalog entry's cost, parsed once per report. An entry
// whose cost cannot be read is listed but prices nothing.
type catalogPrice struct {
	usable bool
	base   priceRates
	// tiered prices select the context tier with the largest size the input
	// exceeds, else the base rates.
	tiered bool
	tiers  []priceTier
	// over200k, when set, replaces the base rates above 200,000 input tokens.
	over200k *priceRates
}

type priceTier struct {
	size  uint64
	rates priceRates
}

func parseCatalogPrice(body []byte) *catalogPrice {
	price := &catalogPrice{}
	var object map[string]json.RawMessage
	if json.Unmarshal(body, &object) != nil {
		return price
	}
	var cost map[string]json.RawMessage
	if json.Unmarshal(object["cost"], &cost) != nil {
		return price
	}
	if raw, ok := cost["tiers"]; ok {
		var tiers []map[string]json.RawMessage
		if json.Unmarshal(raw, &tiers) != nil {
			return price
		}
		seen := map[uint64]bool{}
		for _, tier := range tiers {
			var criteria struct {
				Type string `json:"type"`
				Size uint64 `json:"size"`
			}
			if json.Unmarshal(tier["tier"], &criteria) != nil || criteria.Type != "context" || seen[criteria.Size] {
				return price
			}
			seen[criteria.Size] = true
			price.tiers = append(price.tiers, priceTier{criteria.Size, parseRates(tier)})
		}
		price.tiered = true
	} else if raw, ok := cost["context_over_200k"]; ok {
		// The override is decoded over the base cost, keeping members it omits.
		merged := maps.Clone(cost)
		var over priceRates
		if json.Unmarshal(raw, &merged) == nil {
			over = parseRates(merged)
		}
		price.over200k = &over
	}
	price.base = parseRates(cost)
	price.usable = true
	return price
}

func (price *catalogPrice) rates(input uint64) *priceRates {
	if price.tiered {
		selected, threshold := &price.base, uint64(0)
		for i := range price.tiers {
			if tier := &price.tiers[i]; input > tier.size && tier.size >= threshold {
				selected, threshold = &tier.rates, tier.size
			}
		}
		return selected
	}
	if input > 200000 && price.over200k != nil {
		return price.over200k
	}
	return &price.base
}

// uint128 is an exact sum of rate and token products.
type uint128 struct{ hi, lo uint64 }

// add reports whether the sum still fits.
func (value *uint128) add(other uint128) bool {
	var carry uint64
	value.lo, carry = bits.Add64(value.lo, other.lo, 0)
	value.hi, carry = bits.Add64(value.hi, other.hi, carry)
	return carry == 0
}

// micros applies the channel's multiplier before rounding the cumulative
// amount to micro-USD. Overflow cannot yield a representable rounded price.
func (value uint128) micros(billingRateBps uint32) (uint64, bool) {
	divisor := uint64(1_000_000)
	if billingRateBps != protocol.DefaultBillingRateBps {
		divisor *= uint64(protocol.DefaultBillingRateBps)
		rate := uint64(billingRateBps)
		hi, lo := bits.Mul64(value.lo, rate)
		overflow, upper := bits.Mul64(value.hi, rate)
		hi, carry := bits.Add64(hi, upper, 0)
		if overflow != 0 || carry != 0 {
			return 0, false
		}
		value = uint128{hi: hi, lo: lo}
	}
	if !value.add(uint128{lo: divisor / 2}) || value.hi >= divisor {
		return 0, false
	}
	quotient, _ := bits.Div64(value.hi, value.lo, divisor)
	return quotient, true
}

func tokenPricedMedia(media string) bool {
	return media != string(protocol.MediaKindImage) && media != string(protocol.MediaKindVideo)
}

func readAgentCost(ctx context.Context, db *sql.Conn, prices *modelPrices, media, preset, api, model string, usage canonical.Usage) (*protocol.UsageCost, error) {
	if usage.Pricing != nil {
		return usage.Pricing.Estimate, nil
	}
	if !tokenPricedMedia(media) {
		return nil, nil
	}
	return estimateAgentCost(ctx, db, prices, preset, api, model, usage, protocol.DefaultBillingRateBps)
}

func estimateAgentCost(ctx context.Context, db *sql.Conn, prices *modelPrices, preset, api, model string, usage canonical.Usage, billingRateBps uint32) (*protocol.UsageCost, error) {
	if usage.Input == nil || usage.Output == nil {
		return nil, nil
	}
	price, supplier, err := prices.channel(ctx, db, preset, api, model)
	if err != nil || price == nil || !price.usable {
		return nil, err
	}
	rates := price.rates(*usage.Input)
	if rates.audio {
		return nil, nil
	}
	read, write, reasoning := optionalUint(usage.CacheRead), optionalUint(usage.CacheWrite), optionalUint(usage.Reasoning)
	if *usage.Input < read || *usage.Input-read < write || *usage.Output < reasoning {
		return nil, nil
	}
	hourlyWrite := uint64(0)
	if api == "anthropic" && write > 0 && len(usage.Provider) > 0 {
		var reported struct {
			Creation *struct {
				FiveMinutes *uint64 `json:"ephemeral_5m_input_tokens"`
				OneHour     *uint64 `json:"ephemeral_1h_input_tokens"`
			} `json:"cache_creation"`
		}
		if json.Unmarshal(usage.Provider, &reported) != nil {
			return nil, nil
		}
		if creation := reported.Creation; creation != nil {
			if creation.FiveMinutes == nil || creation.OneHour == nil || *creation.FiveMinutes > write || *creation.OneHour != write-*creation.FiveMinutes {
				return nil, nil
			}
			hourlyWrite = *creation.OneHour
			if hourlyWrite > 0 && supplier != "anthropic" {
				// A compatible API does not imply Anthropic's cache pricing.
				return nil, nil
			}
		}
	}
	if !rates.known[rateInput] || !rates.known[rateOutput] {
		return nil, nil
	}
	var categories [4]uint128
	for _, item := range []struct {
		count           uint64
		field, fallback int
		category        int
		doubled         bool
	}{
		{*usage.Input - read - write, rateInput, -1, 0, false},
		{*usage.Output - reasoning, rateOutput, -1, 1, false},
		{read, rateCacheRead, -1, 2, false},
		{write - hourlyWrite, rateCacheWrite, -1, 3, false},
		// Anthropic's catalog cache_write rate is for five minutes. It bills
		// one-hour writes at twice the base input rate, including context tiers.
		{hourlyWrite, rateInput, -1, 3, true},
		{reasoning, rateReasoning, rateOutput, 1, false},
	} {
		if item.count == 0 {
			continue
		}
		field := item.field
		if !rates.known[field] && item.fallback >= 0 {
			field = item.fallback
		}
		if !rates.known[field] {
			return nil, nil
		}
		if billingRateBps == 0 {
			continue
		}
		var product uint128
		product.hi, product.lo = bits.Mul64(rates.rate[field], item.count)
		if item.doubled && !product.add(product) {
			return nil, nil
		}
		if !categories[item.category].add(product) {
			return nil, nil
		}
	}
	// A product or sum beyond 128 bits exceeds every representable amount.
	var cumulative uint128
	total := uint64(0)
	result := &struct {
		cost      protocol.UsageCost
		breakdown protocol.UsageCostBreakdown
	}{}
	amounts := [4]*uint64{&result.breakdown.Input, &result.breakdown.Output, &result.breakdown.CacheRead, &result.breakdown.CacheWrite}
	for i, category := range categories {
		if !cumulative.add(category) {
			return nil, nil
		}
		next, ok := cumulative.micros(billingRateBps)
		if !ok {
			return nil, nil
		}
		*amounts[i] = next - total
		total = next
	}
	result.cost = protocol.UsageCost{UsdMicros: total, Responses: 1, Breakdown: &result.breakdown}
	return &result.cost, nil
}
