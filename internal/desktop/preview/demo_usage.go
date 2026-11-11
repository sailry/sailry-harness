package preview

import (
	"encoding/json"
	"fmt"
	"maps"
	"math"
	"math/bits"
	"math/rand/v2"
	"slices"
	"sort"
	"strconv"
	"strings"
	"time"
	"unicode"

	"github.com/google/uuid"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// The demo's usage is a year of model responses over the demo's projects,
// provider and models, which the Usage page reads in Demo mode in place of
// the Node's. It is made up like the rest of the demo and held in memory:
// a day's responses come from a generator seeded with that day's number and
// thinned by the day's distance from the ledger's own, so the same clock
// writes the same ledger, and the demo's conversations add the responses
// their own usage counts. A report is computed from the same
// responses the way the Node computes its own, so its totals, trend,
// distribution and records agree.

// Usage is the demo's usage ledger. It does not change once written; what
// lies after the clock's now is not reported yet.
type Usage struct {
	now      func() time.Time
	provider protocol.ProviderID
	name     string
	models   []usageModel
	projects []usageProject
	sessions []usageSession
	// records are in time order, each one's sequence its place.
	records []usageRecord
}

// usageModel is a demo model with its prices in micro-dollars a thousand
// tokens and its output speed in tokens a second.
type usageModel struct {
	id                    string
	reasoning             bool
	input, cached, output uint64
	slow, fast            float64
	key                   protocol.UsageKey
	text                  string
}

type usageProject struct {
	id       protocol.ProjectID
	worktree protocol.WorktreeID
	name     string
}

// usageSession is a conversation of the ledger: a project's, or one of no
// project, which the Node keys by its session.
type usageSession struct {
	id       protocol.SessionID
	worktree protocol.WorktreeID
	title    string
	// project is the session's place in the ledger's projects, -1 for none.
	project                 int
	owner, location         protocol.UsageKey
	ownerText, locationText string
}

// usageRecord is one model response.
type usageRecord struct {
	at             int64
	session, model int
	tokens         protocol.ConversationUsage
	cost           protocol.UsageCostBreakdown
	elapsed, first uint64
}

// usageRates are the prices and speeds of the demo's models by their place
// in DemoModels; a model past them reads the last.
var usageRates = []usageModel{
	{input: 1400, cached: 140, output: 4200, slow: 56, fast: 84},
	{input: 560, cached: 70, output: 1680, slow: 82, fast: 112},
	{input: 140, cached: 28, output: 420, slow: 150, fast: 210},
}

// usageShare is the percentage of a day's conversations each project takes,
// by its place in the demo; the rest belong to no project.
var usageShare = []int{46, 34, 8}

// usageMix is the percentage of conversations each model answers: in a
// project, and outside one, where the quick model does more.
var usageMix = [2][]int{{62, 14, 24}, {35, 15, 50}}

const (
	usageSeed = 0x5a11_0c0d_e5ee_d001
	// usageBefore and usageAfter are the days the ledger is written for
	// around the day it is written on. The days after wait for the clock, so
	// a window left open keeps filling.
	usageBefore = 366
	usageAfter  = 31
	usageDay    = int64(86400000)
)

func usageID(kind, name string) uuid.UUID {
	return uuid.NewSHA1(uuid.NameSpaceOID, []byte("sailry/usage/"+kind+"/"+name))
}

func usageKeyText(key protocol.UsageKey) string {
	order := slices.Index([]string{"provider", "model", "project", "session", "connection", "assistant", "worktree"}, key.Kind)
	body, _ := json.Marshal(key.Data)
	return string(rune('0'+order)) + string(body)
}

// DemoUsage writes the usage of demo d in locale, around now's day.
func DemoUsage(d *Data, locale string, now func() time.Time) *Usage {
	u := &Usage{now: now, provider: protocol.ProviderID(usageID("provider", demoFamily)), name: DemoProvider}
	if len(d.Providers) > 0 {
		u.provider, u.name = protocol.ProviderID(usageID("provider", d.Providers[0].ID)), d.Providers[0].Name
	}
	for i, m := range d.Models {
		model := usageRates[min(i, len(usageRates)-1)]
		model.id, model.reasoning = m.ID, m.Reasoning
		model.key = protocol.UsageKey{Kind: "model", Data: protocol.UsageKeyModelPayload{Provider: u.provider, Model: m.ID}}
		model.text = usageKeyText(model.key)
		u.models = append(u.models, model)
	}
	if len(u.models) == 0 {
		return u
	}
	b := &demo{locale: locale}
	titles := func(id string) []string {
		var out []string
		for i := 1; ; i++ {
			key := fmt.Sprintf("usage.%s.%d", id, i)
			if _, ok := demoCatalogs()[DemoLocales[0]][key]; !ok {
				return out
			}
			out = append(out, b.t(key))
		}
	}
	// pools are the titles of the conversations of each project, then of
	// the ones of no project, which a project without its own reads too.
	chats := titles("chat")
	var pools [][]string
	for _, p := range d.Projects {
		id := protocol.ProjectID(usageID("project", p.ID))
		u.projects = append(u.projects, usageProject{id: id, worktree: protocol.WorktreeID(usageID("worktree", p.ID)), name: p.Name})
		pool := titles(p.ID)
		if len(pool) == 0 {
			pool = chats
		}
		pools = append(pools, pool)
	}
	pools = append(pools, chats)

	start := now()
	y, m, day := start.Date()
	for age := usageBefore; age >= -usageAfter; age-- {
		u.day(time.Date(y, m, day-age, 0, 0, 0, 0, start.Location()), age, pools)
	}
	for i, p := range d.Projects {
		for _, c := range p.Conversations {
			u.conversation(c, i, start)
		}
	}
	for _, c := range d.Unassigned {
		u.conversation(c, -1, start)
	}
	sort.SliceStable(u.records, func(i, j int) bool { return u.records[i].at < u.records[j].at })
	return u
}

// open adds a conversation of project (-1 for none) and returns its place.
func (u *Usage) open(name, title string, project int) int {
	s := usageSession{id: protocol.SessionID(usageID("session", name)), title: usageTitle(title), project: project}
	if project >= 0 {
		p := u.projects[project]
		s.worktree = p.worktree
		s.owner = protocol.UsageKey{Kind: "project", Data: p.id}
		s.location = protocol.UsageKey{Kind: "worktree", Data: protocol.UsageKeyWorktreePayload{Project: p.id, Worktree: p.worktree}}
	} else {
		s.worktree = protocol.WorktreeID(usageID("worktree", name))
		s.owner = protocol.UsageKey{Kind: "session", Data: s.id}
		s.location = s.owner
	}
	s.ownerText, s.locationText = usageKeyText(s.owner), usageKeyText(s.location)
	u.sessions = append(u.sessions, s)
	return len(u.sessions) - 1
}

// usageTitle shortens a conversation's title as the Node's report does.
func usageTitle(title string) string {
	text := []rune(strings.Join(strings.Fields(title), " "))
	if len(text) <= 48 {
		return string(text)
	}
	return strings.TrimRightFunc(string(text[:47]), unicode.IsSpace) + "…"
}

// pick draws a place by its percentage; past the last it is len(shares).
func pick(r *rand.Rand, shares []int) int {
	roll := r.IntN(100)
	for i, share := range shares {
		if roll < share {
			return i
		}
		roll -= share
	}
	return len(shares)
}

// day writes the conversations of the day that starts at start, age days
// before the ledger's own: working days carry most, a few of them are busy
// and a few are off, and the year grows toward its end.
func (u *Usage) day(start time.Time, age int, pools [][]string) {
	y, m, d := start.Date()
	number := uint64(time.Date(y, m, d, 0, 0, 0, 0, time.UTC).Unix() / 86400)
	r := rand.New(rand.NewPCG(usageSeed, number))
	weekend := start.Weekday() == time.Saturday || start.Weekday() == time.Sunday
	sessions := 6.5
	switch start.Weekday() {
	case time.Saturday:
		sessions = 1.3
	case time.Sunday:
		sessions = 0.8
	default:
		switch roll := r.Float64(); {
		case roll < 0.04:
			sessions = 0
		case roll < 0.11:
			sessions *= 2.2
		}
	}
	growth := 1.0
	if age > 0 {
		growth -= 0.45 * float64(min(age, 365)) / 365
	}
	count := int(sessions*growth*(0.6+0.8*r.Float64()) + r.Float64())
	for i := range count {
		shares := usageShare[:min(len(u.projects), len(usageShare))]
		project, mix := pick(r, shares), usageMix[0]
		if project == len(shares) {
			project, mix = -1, usageMix[1]
		}
		model := min(pick(r, mix), len(u.models)-1)
		minute := 600 + r.IntN(720)
		if !weekend {
			switch roll := r.IntN(100); {
			case roll < 40:
				minute = 540 + r.IntN(180)
			case roll < 85:
				minute = 780 + r.IntN(300)
			default:
				minute = 1140 + r.IntN(240)
			}
		}
		at := start.Add(time.Duration(minute)*time.Minute + time.Duration(r.IntN(60000))*time.Millisecond).UnixMilli()
		pool := pools[len(pools)-1]
		if project >= 0 {
			pool = pools[project]
		}
		title := ""
		if len(pool) > 0 {
			title = pool[r.IntN(len(pool))]
		}
		u.steps(r, u.open(fmt.Sprintf("%d/%d", number, i), title, project), model, at)
	}
}

// steps writes the responses of one conversation from at: the context grows
// with every step and is read from the cache while the steps follow closely,
// and is compacted before it fills the window.
func (u *Usage) steps(r *rand.Rand, session, model int, at int64) {
	rate := u.models[model]
	context := 5000 + r.Uint64N(9000)
	cached := uint64(0)
	if r.IntN(100) < 60 {
		// The instructions and tools an earlier conversation left cached.
		cached = context * 7 / 10
	}
	count := min(2+int(r.ExpFloat64()*9), 48)
	for step := range count {
		output := 120 + uint64(r.ExpFloat64()*420)
		if step == count-1 {
			output += 300 + r.Uint64N(1200)
		}
		output = min(output, 6000)
		reasoning := uint64(0)
		if rate.reasoning {
			reasoning = output * (10 + r.Uint64N(36)) / 100
		}
		speed := rate.slow + (rate.fast-rate.slow)*r.Float64()
		first := 300_000 + r.Uint64N(900_000) + context*4
		elapsed := first + uint64(float64(output)/speed*1e6)
		u.records = append(u.records, usageRecord{at: at, session: session, model: model,
			tokens: protocol.ConversationUsage{Input: context, Output: output, CachedInput: cached, Reasoning: reasoning},
			cost:   rate.cost(context, cached, output), elapsed: elapsed, first: first})
		at += int64(elapsed/1000) + 4000 + r.Int64N(86_000)
		cached = context
		context += output + 500 + r.Uint64N(4500)
		if r.IntN(100) < 15 {
			pause := 1 + r.Int64N(12)
			at += pause * 60_000
			if pause > 9 {
				cached = 0
			}
		}
		if context > 170_000 {
			context, cached = 28_000+r.Uint64N(14_000), 0
		}
	}
}

// cost prices a response. The parts round one after another, as the Node's
// estimate does, so they add up to the rounded total.
func (m usageModel) cost(input, cached, output uint64) protocol.UsageCostBreakdown {
	var parts [3]uint64
	sum, total := uint64(0), uint64(0)
	for i, part := range [3]uint64{(input - cached) * m.input, output * m.output, cached * m.cached} {
		sum += part
		next := (sum + 500) / 1000
		parts[i], total = next-total, next
	}
	return protocol.UsageCostBreakdown{Input: parts[0], Output: parts[1], CacheRead: parts[2]}
}

// split shares total between the weights in whole numbers that add up to
// it. A share is at most one over its exact part.
func split(total uint64, weights []uint64) []uint64 {
	out := make([]uint64, len(weights))
	sum := uint64(0)
	for _, w := range weights {
		sum += w
	}
	if sum == 0 {
		if len(out) > 0 {
			out[len(out)-1] = total
		}
		return out
	}
	left := total
	for i, w := range weights {
		high, low := bits.Mul64(total, w)
		out[i], _ = bits.Div64(high, low, sum)
		left -= out[i]
	}
	for i := 0; left > 0; i = (i + 1) % len(out) {
		if weights[i] > 0 {
			out[i]++
			left--
		}
	}
	return out
}

// conversation writes the responses of demo conversation c, the last one
// when c was last updated before end, so that they add up to the usage the
// conversation's composer shows: its tokens, cached tokens, cost and speed.
func (u *Usage) conversation(c *Conversation, project int, end time.Time) {
	usage := c.Session.Usage
	if usage == nil || usage.Tokens <= 0 {
		return
	}
	tokens := uint64(usage.Tokens)
	price := uint64(math.Round(max(usage.Cost, 0) * 1e6))
	// The quick model answered a conversation that cost a fraction of what
	// the first model's would.
	model := 0
	if price*4 < tokens {
		model = len(u.models) - 1
	}
	rate := u.models[model]
	count := min(max(usage.Tokens/9000, usage.Turns, 1), 12)
	cached := min(uint64(max(usage.Cached, 0)), tokens)
	output := min(max(tokens*6/100, uint64(count)), tokens-cached)
	grow, even := make([]uint64, count), make([]uint64, count)
	for i := range count {
		grow[i], even[i] = uint64(2+i), uint64(2+i*7%5)
	}
	inputs, outputs := split(tokens-output, grow), split(output, even)
	// A response's cached share is its input's share of the whole, which
	// never exceeds the input.
	reads := split(cached, inputs)
	sizes := make([]uint64, count)
	for i := range count {
		sizes[i] = inputs[i] + outputs[i]
	}
	costs := split(price, sizes)
	spent := uint64(0)
	if usage.Speed > 0 {
		spent = uint64(math.Round(float64(output) * 1e6 / usage.Speed))
	}
	times := split(spent, outputs)
	session := u.open(c.ID, c.Title, project)
	last := end.Add(c.Updated.Sub(Now))
	for i := range count {
		parts := split(costs[i], []uint64{(inputs[i] - reads[i]) * rate.input, outputs[i] * rate.output, reads[i] * rate.cached})
		reasoning := uint64(0)
		if rate.reasoning {
			reasoning = outputs[i] * 3 / 10
		}
		u.records = append(u.records, usageRecord{at: last.Add(-time.Duration(count-1-i) * 3 * time.Minute).UnixMilli(), session: session, model: model,
			tokens:  protocol.ConversationUsage{Input: inputs[i], Output: outputs[i], CachedInput: reads[i], Reasoning: reasoning},
			cost:    protocol.UsageCostBreakdown{Input: parts[0], Output: parts[1], CacheRead: parts[2]},
			elapsed: times[i], first: min(times[i]/3, 380_000+uint64(i%4)*90_000)})
	}
}

// Inventory is what the ledger's filters choose between: the demo's
// projects, and its provider with its models.
func (u *Usage) Inventory() ([]protocol.Project, []protocol.ConversationProvider) {
	projects := []protocol.Project{}
	for _, p := range u.projects {
		project := protocol.DefaultProject()
		project.ID, project.Name = p.id, p.name
		projects = append(projects, project)
	}
	provider := protocol.DefaultConversationProvider()
	provider.ID, provider.Name, provider.Enabled = u.provider, u.name, true
	for _, m := range u.models {
		model := protocol.DefaultConversationModel()
		model.ID = m.id
		provider.Models = append(provider.Models, model)
	}
	return projects, []protocol.ConversationProvider{provider}
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

func (r *usageRecord) add(target *protocol.UsageMetrics) {
	target.Responses++
	if target.Tokens == nil {
		target.Tokens = &protocol.ConversationUsage{}
	}
	target.Tokens.Input += r.tokens.Input
	target.Tokens.Output += r.tokens.Output
	target.Tokens.CachedInput += r.tokens.CachedInput
	target.Tokens.Reasoning += r.tokens.Reasoning
	if target.Generation == nil {
		target.Generation = &protocol.UsageGeneration{}
	}
	target.Generation.ElapsedUs += r.elapsed
	target.Generation.OutputTokens += r.tokens.Output
	target.Generation.Responses++
	if target.Cost == nil {
		target.Cost = &protocol.UsageCost{Breakdown: &protocol.UsageCostBreakdown{}}
	}
	target.Cost.UsdMicros += r.cost.Input + r.cost.Output + r.cost.CacheRead + r.cost.CacheWrite
	target.Cost.Responses++
	target.Cost.Breakdown.Input += r.cost.Input
	target.Cost.Breakdown.Output += r.cost.Output
	target.Cost.Breakdown.CacheRead += r.cost.CacheRead
	target.Cost.Breakdown.CacheWrite += r.cost.CacheWrite
}

func (r *usageRecord) addModel(point *protocol.UsagePoint, model string) {
	if point.Models == nil {
		point.Models = map[string]protocol.ConversationUsage{}
	}
	v := point.Models[model]
	v.Input += r.tokens.Input
	v.Output += r.tokens.Output
	v.CachedInput += r.tokens.CachedInput
	v.Reasoning += r.tokens.Reasoning
	point.Models[model] = v
}

// Report answers query as node's usage report, with the Node's limits,
// buckets, grouping, order and paging, over the responses up to now.
func (u *Usage) Report(node protocol.NodeID, query protocol.UsageQuery) (protocol.UsageReport, error) {
	report := protocol.DefaultUsageReport()
	report.Node, report.Query = node, query
	limits := &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "usage time range or filters exceed the query limits"}
	if query.StartMs < 0 || query.EndMs <= query.StartMs || query.EndMs-query.StartMs > 366*usageDay || query.UtcOffsetMinutes > 840 || query.UtcOffsetMinutes < -840 || query.Before != nil && (query.Before.TimestampMs < query.StartMs || query.Before.TimestampMs >= query.EndMs) || len(query.Projects) > 128 || len(query.Worktrees) > 128 || len(query.Providers) > 128 || len(query.Models) > 128 {
		return report, limits
	}
	for _, model := range query.Models {
		if strings.TrimSpace(model) == "" || len(model) > 256 {
			return report, limits
		}
	}
	now := u.now().UnixMilli()
	visible := sort.Search(len(u.records), func(i int) bool { return u.records[i].at > now })
	// The cursor moves when a response comes into sight, and is far above
	// any a Node counts, so a page tells the two ledgers apart.
	report.Cursor = 1<<62 | uint64(visible)
	offset := int64(query.UtcOffsetMinutes) * 60000
	report.Days = usageBuckets(query.StartMs, query.EndMs, usageDay, offset)
	recentStart := max(query.StartMs, query.EndMs-2*usageDay)
	report.Recent = usageBuckets(recentStart, query.EndMs, 1800000, offset)
	first := sort.Search(visible, func(i int) bool { return u.records[i].at >= query.StartMs })
	last := sort.Search(visible, func(i int) bool { return u.records[i].at >= query.EndMs })

	provider := len(query.Providers) == 0 || slices.Contains(query.Providers, u.provider)
	models := make([]bool, len(u.models))
	for i, m := range u.models {
		models[i] = len(query.Models) == 0 || slices.Contains(query.Models, m.id)
	}
	projects := make([]bool, len(u.projects))
	for i, p := range u.projects {
		projects[i] = slices.Contains(query.Projects, p.id)
	}
	matches := func(r *usageRecord) bool {
		s := &u.sessions[r.session]
		return provider && models[r.model] &&
			(len(query.Projects) == 0 || s.project >= 0 && projects[s.project]) &&
			(len(query.Worktrees) == 0 || slices.Contains(query.Worktrees, s.worktree))
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
	providerKey := protocol.UsageKey{Kind: "provider", Data: u.provider}
	providerText := usageKeyText(providerKey)
	groups := map[string]*protocol.UsageGroup{}
	resources := map[string]protocol.UsageKey{}
	for i := first; i < last; i++ {
		r := &u.records[i]
		if !matches(r) {
			continue
		}
		s, m := &u.sessions[r.session], &u.models[r.model]
		keys := [4]protocol.UsageKey{providerKey, m.key, s.owner, s.location}
		texts := [4]string{providerText, m.text, s.ownerText, s.locationText}
		group := groups[texts[index]]
		if group == nil {
			g := protocol.DefaultUsageGroup()
			g.Key = keys[index]
			name := u.name
			if index == 2 {
				name = u.scope(s)
			}
			if index < 3 && name != "" {
				g.Name = &name
			}
			group = &g
			groups[texts[index]] = group
		}
		for k, text := range texts {
			resources[text] = keys[k]
		}
		r.add(&report.Totals)
		r.add(&group.Metrics)
		day := &report.Days[(usageFloor(r.at, usageDay, offset)-report.Days[0].StartMs)/usageDay]
		r.add(&day.Metrics)
		r.addModel(day, m.id)
		if r.at >= recentStart {
			bucket := &report.Recent[(usageFloor(r.at, 1800000, offset)-report.Recent[0].StartMs)/1800000]
			r.add(&bucket.Metrics)
			r.addModel(bucket, m.id)
		}
	}
	// The page of records is the newest twenty before the query's position;
	// a response's sequence is its place in the ledger.
	for i := last - 1; i >= first; i-- {
		r := &u.records[i]
		sequence := uint64(i + 1)
		if !matches(r) || query.Before != nil && (r.at > query.Before.TimestampMs || r.at == query.Before.TimestampMs && sequence >= query.Before.Sequence) {
			continue
		}
		if len(report.Requests.Items) == 20 {
			report.Requests.HasMore = true
			break
		}
		s, m := &u.sessions[r.session], &u.models[r.model]
		request := protocol.DefaultUsageRequest()
		request.Position = protocol.UsagePosition{TimestampMs: r.at, Node: node, Sequence: sequence}
		request.Session = s.id
		if s.title != "" {
			request.SessionTitle = &s.title
		}
		request.Turn = protocol.TurnID(usageID("turn", strconv.Itoa(i)))
		if s.project >= 0 {
			request.Project = &u.projects[s.project].id
		}
		request.ScopeName = u.scope(s)
		request.Worktree = s.worktree
		request.Provider, request.ProviderName, request.Model = u.provider, u.name, m.id
		request.Tokens = r.tokens
		cost, elapsed := r.cost.Input+r.cost.Output+r.cost.CacheRead+r.cost.CacheWrite, r.elapsed
		request.UsdMicros, request.ElapsedUs = &cost, &elapsed
		if r.first <= r.elapsed {
			started := r.first
			request.FirstTokenUs = &started
		}
		report.Requests.Items = append(report.Requests.Items, request)
	}
	for _, text := range slices.Sorted(maps.Keys(groups)) {
		report.Groups = append(report.Groups, *groups[text])
	}
	for _, text := range slices.Sorted(maps.Keys(resources)) {
		report.Resources = append(report.Resources, resources[text])
	}
	return report, nil
}

// scope is the name of the project of s, empty for none.
func (u *Usage) scope(s *usageSession) string {
	if s.project < 0 {
		return ""
	}
	return u.projects[s.project].name
}
