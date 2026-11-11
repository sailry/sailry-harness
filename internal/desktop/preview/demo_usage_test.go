package preview

import (
	"encoding/json"
	"errors"
	"math"
	"reflect"
	"slices"
	"strings"
	"testing"
	"time"
	"unicode"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// usageSum adds metrics up the way a report's totals are.
type usageSum struct {
	responses, input, output, cached, reasoning, cost, elapsed uint64
}

func (s *usageSum) add(m protocol.UsageMetrics) {
	s.responses += m.Responses
	if m.Tokens != nil {
		s.input += m.Tokens.Input
		s.output += m.Tokens.Output
		s.cached += m.Tokens.CachedInput
		s.reasoning += m.Tokens.Reasoning
	}
	if m.Cost != nil {
		s.cost += m.Cost.UsdMicros
	}
	if m.Generation != nil {
		s.elapsed += m.Generation.ElapsedUs
	}
}

func usageOf(m protocol.UsageMetrics) usageSum {
	var s usageSum
	s.add(m)
	return s
}

// The demo's usage (Demo mode) answers the Usage page's queries the way a
// Node answers them: every range has figures whose parts add up, filters
// narrow them, the records page through once each, and the demo's own
// conversations count what their composers show.
func testDemoUsage(t *testing.T) {
	zone := time.FixedZone("", 8*3600)
	// A Thursday afternoon.
	clock := time.Date(2026, 11, 12, 15, 20, 0, 0, zone)
	d := Demo("en", "local")
	u := DemoUsage(d, "en", func() time.Time { return clock })
	var node protocol.NodeID
	node[0] = 7
	end := time.Date(2026, 11, 13, 0, 0, 0, 0, zone).UnixMilli()
	// window is the page's query for a range of days, which ends with the
	// local day.
	window := func(days int) protocol.UsageQuery {
		query := protocol.DefaultUsageQuery()
		query.StartMs, query.EndMs, query.UtcOffsetMinutes, query.Dimension = end-int64(days)*usageDay, end, 480, protocol.UsageDimensionModel
		return query
	}
	read := func(t *testing.T, query protocol.UsageQuery) protocol.UsageReport {
		t.Helper()
		report, err := u.Report(node, query)
		if err != nil {
			t.Fatalf("the report of %+v failed: %v", query, err)
		}
		return report
	}

	t.Run("ranges", func(t *testing.T) {
		previous := uint64(0)
		for _, days := range []int{1, 7, 30, 90, 365} {
			report := read(t, window(days))
			total := usageOf(report.Totals)
			if total.responses == 0 || total.input == 0 || total.output == 0 || total.cost == 0 || total.elapsed == 0 {
				t.Fatalf("%d days total %+v", days, total)
			}
			if total.responses <= previous {
				t.Errorf("%d days hold %d responses, a shorter range %d", days, total.responses, previous)
			}
			previous = total.responses
			if report.Node != node || !reflect.DeepEqual(report.Query, window(days)) || report.Cursor < 1<<62 {
				t.Errorf("%d days answer node %x, query %+v, cursor %d", days, report.Node[:2], report.Query, report.Cursor)
			}
			if total.cached > total.input || total.reasoning > total.output || total.cached*2 < total.input {
				t.Errorf("%d days read %d of %d input tokens from the cache and reason %d of %d", days, total.cached, total.input, total.reasoning, total.output)
			}
			cost, generation := report.Totals.Cost, report.Totals.Generation
			if parts := cost.Breakdown; parts == nil || parts.Input+parts.Output+parts.CacheRead+parts.CacheWrite != cost.UsdMicros || cost.Responses != total.responses {
				t.Errorf("%d days cost %d in parts %+v over %d responses", days, cost.UsdMicros, parts, cost.Responses)
			}
			if generation.OutputTokens != total.output || generation.Responses != total.responses {
				t.Errorf("%d days generated %+v of %+v", days, generation, total)
			}
			if speed := float64(generation.OutputTokens) * 1e6 / float64(generation.ElapsedUs); speed < 40 || speed > 160 {
				t.Errorf("%d days write %.1f tokens a second", days, speed)
			}
			if len(report.Days) != days {
				t.Fatalf("%d days have %d daily points", days, len(report.Days))
			}
			var trend, recent, groups, models usageSum
			for i, point := range report.Days {
				if point.StartMs != report.Query.StartMs+int64(i)*usageDay {
					t.Fatalf("day %d of %d starts at %d", i, days, point.StartMs)
				}
				trend.add(point.Metrics)
				for _, tokens := range point.Models {
					models.input += tokens.Input
					models.output += tokens.Output
				}
			}
			for _, point := range report.Recent {
				recent.add(point.Metrics)
			}
			for _, group := range report.Groups {
				groups.add(group.Metrics)
			}
			if trend != total || groups != total || models.input != total.input || models.output != total.output {
				t.Errorf("%d days total %+v, their days %+v, their groups %+v, their models %d in %d out", days, total, trend, groups, models.input, models.output)
			}
			if want := min(days, 2) * 48; len(report.Recent) != want {
				t.Errorf("%d days have %d half hours, want %d", days, len(report.Recent), want)
			}
			if days == 1 && recent != total {
				t.Errorf("a day's half hours hold %+v of %+v", recent, total)
			}
			if days > 2 && (recent.responses == 0 || recent.responses >= total.responses) {
				t.Errorf("the last two of %d days hold %d of %d responses", days, recent.responses, total.responses)
			}
			if len(report.Requests.Items) == 0 || report.Requests.Items[0].Position.TimestampMs > clock.UnixMilli() {
				t.Errorf("%d days list %d records, the newest after now", days, len(report.Requests.Items))
			}
			// The report is a Node's on the wire.
			body, err := json.Marshal(report)
			var decoded protocol.UsageReport
			if err == nil {
				err = json.Unmarshal(body, &decoded)
			}
			if err != nil || decoded.Totals.Responses != total.responses || len(decoded.Groups) != len(report.Groups) {
				t.Errorf("%d days do not read back as a usage report: %v", days, err)
			}
		}
	})

	// Working days carry the year, which the weekends rest from, and a few
	// days are busy.
	t.Run("shape", func(t *testing.T) {
		report := read(t, window(365))
		var week, rest, busiest uint64
		var weekdays, weekends uint64
		for _, point := range report.Days {
			day := time.UnixMilli(point.StartMs).In(zone).Weekday()
			if day == time.Saturday || day == time.Sunday {
				rest += point.Metrics.Responses
				weekends++
			} else {
				week += point.Metrics.Responses
				weekdays++
			}
			busiest = max(busiest, point.Metrics.Responses)
		}
		if week*weekends < 3*rest*weekdays {
			t.Errorf("working days hold %d responses over %d days, weekends %d over %d", week, weekdays, rest, weekends)
		}
		if mean := week / weekdays; busiest < mean*2 || mean < 20 || mean > 200 {
			t.Errorf("a working day holds %d responses and the busiest day %d", mean, busiest)
		}
		total := usageOf(report.Totals)
		if dollars := float64(total.cost) / 1e6; dollars < 20 || dollars > 2000 {
			t.Errorf("the year costs $%.2f", dollars)
		}
		if size := total.input / total.responses; size < 5000 || size > 200_000 {
			t.Errorf("a response reads %d tokens", size)
		}
	})

	t.Run("filters", func(t *testing.T) {
		projects, providers := u.Inventory()
		var names, models []string
		for _, p := range projects {
			names = append(names, p.Name)
		}
		for _, m := range providers[0].Models {
			models = append(models, m.ID)
		}
		if !slices.Equal(names, []string{"harbor-api", "lumen-web", "growth-notes"}) || len(providers) != 1 || providers[0].Name != DemoProvider ||
			!slices.Equal(models, []string{"deepseek-v4.1-pro", "deepseek-v4.1", "deepseek-v4.1-flash"}) {
			t.Fatalf("the ledger offers projects %q and models %q of %d providers", names, models, len(providers))
		}
		all := usageOf(read(t, window(30)).Totals)

		// One project: its records alone, in one group under its name.
		query := window(30)
		query.Dimension, query.Projects = protocol.UsageDimensionProject, []protocol.ProjectID{projects[0].ID}
		report := read(t, query)
		if got := usageOf(report.Totals); got.responses == 0 || got.responses >= all.responses || got.cost >= all.cost {
			t.Errorf("one project holds %+v of %+v", got, all)
		}
		if len(report.Groups) != 1 || report.Groups[0].Key.Kind != "project" || report.Groups[0].Name == nil || *report.Groups[0].Name != "harbor-api" {
			t.Errorf("one project groups as %+v", report.Groups)
		}
		for _, item := range report.Requests.Items {
			if item.Project == nil || *item.Project != projects[0].ID || item.ScopeName != "harbor-api" || item.ProviderName != DemoProvider {
				t.Errorf("a record of harbor-api is of %v, %q, %q", item.Project, item.ScopeName, item.ProviderName)
			}
		}
		query.Projects = []protocol.ProjectID{protocol.NewProjectID()}
		if report := read(t, query); report.Totals.Responses != 0 || len(report.Groups) != 0 || len(report.Requests.Items) != 0 || len(report.Days) != 30 {
			t.Errorf("a project the ledger lacks holds %d responses", report.Totals.Responses)
		}

		// Each model alone; together they are everything.
		var sum usageSum
		for _, model := range models {
			query := window(30)
			query.Models = []string{model}
			report := read(t, query)
			got := usageOf(report.Totals)
			if got.responses == 0 || got.responses >= all.responses || len(report.Groups) != 1 {
				t.Errorf("%s holds %+v of %+v in %d groups", model, got, all, len(report.Groups))
			}
			for _, item := range report.Requests.Items {
				if item.Model != model {
					t.Errorf("a record of %s is of %s", model, item.Model)
				}
			}
			sum.add(report.Totals)
		}
		if sum != all {
			t.Errorf("the models hold %+v between them, the ledger %+v", sum, all)
		}

		// The provider is everything; another has nothing.
		query = window(30)
		query.Providers = []protocol.ProviderID{providers[0].ID}
		if got := usageOf(read(t, query).Totals); got != all {
			t.Errorf("the provider holds %+v of %+v", got, all)
		}
		query.Providers = []protocol.ProviderID{protocol.NewProviderID()}
		if got := read(t, query).Totals.Responses; got != 0 {
			t.Errorf("another provider holds %d responses", got)
		}

		// Every dimension groups the same records.
		for dimension, want := range map[protocol.UsageDimension][]string{protocol.UsageDimensionProvider: {"provider"}, protocol.UsageDimensionModel: {"model"},
			protocol.UsageDimensionProject: {"project", "session"}, protocol.UsageDimensionWorktree: {"session", "worktree"}} {
			query := window(30)
			query.Dimension = dimension
			report := read(t, query)
			var groups usageSum
			var kinds []string
			named := 0
			for _, group := range report.Groups {
				groups.add(group.Metrics)
				if !slices.Contains(kinds, group.Key.Kind) {
					kinds = append(kinds, group.Key.Kind)
				}
				if group.Name != nil {
					named++
				}
			}
			slices.Sort(kinds)
			if groups != all || !slices.Equal(kinds, want) {
				t.Errorf("by %s the groups are %q and hold %+v of %+v", dimension, kinds, groups, all)
			}
			switch dimension {
			case protocol.UsageDimensionProvider:
				if len(report.Groups) != 1 || named != 1 {
					t.Errorf("by provider there are %d groups, %d named", len(report.Groups), named)
				}
			case protocol.UsageDimensionModel:
				if len(report.Groups) != 3 || named != 3 || *report.Groups[0].Name != DemoProvider {
					t.Errorf("by model there are %d groups, %d named", len(report.Groups), named)
				}
			case protocol.UsageDimensionProject:
				if named != 3 || len(report.Groups) <= 3 {
					t.Errorf("by project there are %d groups, %d named", len(report.Groups), named)
				}
			case protocol.UsageDimensionWorktree:
				if named != 0 {
					t.Errorf("by worktree %d groups are named", named)
				}
			}
			// The filters choose between every key the records carry.
			var resources []string
			for _, key := range report.Resources {
				if !slices.Contains(resources, key.Kind) {
					resources = append(resources, key.Kind)
				}
			}
			if !slices.Equal(resources, []string{"provider", "model", "project", "session", "worktree"}) {
				t.Errorf("by %s the resources are %q", dimension, resources)
			}
		}
	})

	// The records page from the newest back, every one once, under totals
	// that stay the range's.
	t.Run("paging", func(t *testing.T) {
		query := window(7)
		first := read(t, query)
		seen := map[uint64]bool{}
		var last *protocol.UsagePosition
		pages := 0
		for {
			report := read(t, query)
			pages++
			if usageOf(report.Totals) != usageOf(first.Totals) || len(report.Requests.Items) > 20 || len(report.Requests.Items) == 0 {
				t.Fatalf("page %d holds %d records under totals %+v", pages, len(report.Requests.Items), usageOf(report.Totals))
			}
			for _, item := range report.Requests.Items {
				at := item.Position
				if seen[at.Sequence] || at.Node != node || at.TimestampMs < query.StartMs || at.TimestampMs > clock.UnixMilli() {
					t.Fatalf("page %d lists %+v again or out of range", pages, at)
				}
				if last != nil && (at.TimestampMs > last.TimestampMs || at.TimestampMs == last.TimestampMs && at.Sequence >= last.Sequence) {
					t.Fatalf("page %d lists %+v after %+v", pages, at, *last)
				}
				seen[at.Sequence], last = true, &at
				if item.SessionTitle == nil || *item.SessionTitle == "" || item.UsdMicros == nil || item.ElapsedUs == nil || item.FirstTokenUs == nil ||
					*item.FirstTokenUs > *item.ElapsedUs || item.Tokens.CachedInput > item.Tokens.Input || item.Tokens.Reasoning > item.Tokens.Output {
					t.Fatalf("record %+v is incomplete", item)
				}
			}
			if !report.Requests.HasMore {
				break
			}
			if len(report.Requests.Items) != 20 {
				t.Fatalf("page %d holds %d records and more follow", pages, len(report.Requests.Items))
			}
			query.Before = last
		}
		if uint64(len(seen)) != first.Totals.Responses || pages != (len(seen)+19)/20 {
			t.Errorf("%d pages list %d records of %d", pages, len(seen), first.Totals.Responses)
		}
	})

	// A demo conversation's records add up to the usage its composer shows.
	t.Run("conversations", func(t *testing.T) {
		count := 0
		each := func(list []*Conversation, project string) {
			for _, c := range list {
				usage := c.Session.Usage
				if usage == nil {
					t.Errorf("%s counts no usage", c.ID)
					continue
				}
				count++
				id := protocol.SessionID(usageID("session", c.ID))
				var tokens, cached, cost, output, elapsed uint64
				records := 0
				for _, r := range u.records {
					s := u.sessions[r.session]
					if s.id != id {
						continue
					}
					records++
					tokens += r.tokens.Input + r.tokens.Output
					cached += r.tokens.CachedInput
					cost += r.cost.Input + r.cost.Output + r.cost.CacheRead + r.cost.CacheWrite
					output += r.tokens.Output
					elapsed += r.elapsed
					if r.tokens.CachedInput > r.tokens.Input || r.first > r.elapsed || u.scope(&s) != project || s.title != usageTitle(c.Title) {
						t.Errorf("%s has record %+v in %q titled %q", c.ID, r, u.scope(&s), s.title)
					}
					if ago := clock.UnixMilli() - r.at; ago < Now.Sub(c.Updated).Milliseconds() || ago > Now.Sub(c.Updated).Milliseconds()+time.Hour.Milliseconds() {
						t.Errorf("%s has a record %d ms ago, updated %s ago", c.ID, ago, Now.Sub(c.Updated))
					}
				}
				speed := float64(output) * 1e6 / float64(elapsed)
				if records < usage.Turns || tokens != uint64(usage.Tokens) || cached != uint64(usage.Cached) || cost != uint64(math.Round(usage.Cost*1e6)) || math.Abs(speed-usage.Speed) > 0.05 {
					t.Errorf("%s shows %+v; its %d records hold %d tokens, %d cached, %d micro-dollars at %.2f a second", c.ID, *usage, records, tokens, cached, cost, speed)
				}
			}
		}
		for _, p := range d.Projects {
			each(p.Conversations, p.Name)
		}
		each(d.Unassigned, "")
		if count < 15 {
			t.Errorf("the demo has %d conversations", count)
		}
	})

	// The same clock writes the same ledger; a later one shows what the
	// earlier kept for later, and the titles follow the language.
	t.Run("clock", func(t *testing.T) {
		again := DemoUsage(Demo("en", "local"), "en", func() time.Time { return clock })
		same, err := again.Report(node, window(90))
		if err != nil || !reflect.DeepEqual(same, read(t, window(90))) {
			t.Errorf("the ledger written again answers another report: %v", err)
		}
		ahead := window(7)
		ahead.StartMs, ahead.EndMs = end, end+7*usageDay
		before := read(t, ahead)
		if before.Totals.Responses != 0 || len(before.Requests.Items) != 0 || len(before.Days) != 7 {
			t.Errorf("the week ahead holds %d responses", before.Totals.Responses)
		}
		start := clock
		clock = clock.AddDate(0, 0, 8)
		after := read(t, ahead)
		clock = start
		if after.Totals.Responses == 0 || after.Cursor <= before.Cursor {
			t.Errorf("the week holds %d responses once it has passed, cursor %d after %d", after.Totals.Responses, after.Cursor, before.Cursor)
		}
		zh := DemoUsage(Demo("zh-CN", "local"), "zh-CN", func() time.Time { return clock })
		report, err := zh.Report(node, window(30))
		if err != nil || usageOf(report.Totals) != usageOf(read(t, window(30)).Totals) {
			t.Fatalf("the Chinese ledger holds other figures: %v", err)
		}
		for _, item := range report.Requests.Items {
			if !strings.ContainsFunc(*item.SessionTitle, func(r rune) bool { return unicode.Is(unicode.Han, r) }) {
				t.Errorf("the Chinese ledger titles a record %q", *item.SessionTitle)
			}
		}
	})

	// The Node's limits are the ledger's.
	t.Run("limits", func(t *testing.T) {
		wide, reversed, shifted, paged, blank := window(367), window(1), window(1), window(1), window(1)
		reversed.EndMs = reversed.StartMs
		shifted.UtcOffsetMinutes = 900
		paged.Before = &protocol.UsagePosition{TimestampMs: paged.EndMs}
		blank.Models = []string{" "}
		for name, query := range map[string]protocol.UsageQuery{"wide": wide, "reversed": reversed, "shifted": shifted, "paged": paged, "blank": blank} {
			_, err := u.Report(node, query)
			var fault *protocol.Fault
			if !errors.As(err, &fault) || fault.Code != protocol.ErrorCodeInvalidRequest {
				t.Errorf("the %s query answers %v", name, err)
			}
		}
	})
}
