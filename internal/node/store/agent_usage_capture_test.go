package store

import (
	"database/sql"
	"fmt"
	"reflect"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestCanonicalUsageFreezesPricesFromTheExecutingProvider(t *testing.T) {
	t.Parallel()
	storage, node, session, turn, now := agentHistoryFixture(t, protocol.WorkModeCode)
	ctx := t.Context()
	builtin := func(id string) *protocol.ConversationPresetsPresetRef {
		return &protocol.ConversationPresetsPresetRef{Source: protocol.ConversationPresetsSource{Kind: "builtin"}, ID: id}
	}
	main := protocol.ConversationProvider{ID: session.Config.Provider, Name: "Fixture", Api: protocol.ConversationModelApiChatCompletions,
		Preset: builtin("deepseek"), Authentication: protocol.AuthenticationApiKey, BillingRateBps: ptr(uint32(5_000))}
	helper := protocol.ConversationProvider{ID: protocol.NewProviderID(), Name: "Helper", Api: protocol.ConversationModelApiResponses,
		Preset: builtin("xai"), Authentication: protocol.AuthenticationApiKey, BillingRateBps: ptr(uint32(20_000))}
	free, account := helper, helper
	free.BillingRateBps, account.BillingRateBps = ptr(uint32(0)), ptr(uint32(0))
	account.Authentication = protocol.AuthenticationChatGptOAuth
	requests := []struct {
		provider *protocol.ConversationProvider
		model    string
		cost     *uint64
	}{
		{&main, "fixture", ptr(uint64(12))},
		{&helper, "fixture", ptr(uint64(80))},
		{&free, "fixture", ptr(uint64(0))},
		{&account, "fixture", ptr(uint64(40))},
		{&helper, "unavailable", nil},
		{nil, "fixture", ptr(uint64(32))},
	}
	var records []canonical.Record
	if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		for supplier, body := range map[string]string{
			"openai":   `{"cost":{"input":2,"output":4}}`,
			"deepseek": `{"cost":{"input":3,"output":6}}`,
			"xai":      `{"cost":{"input":5,"output":10}}`,
		} {
			if _, err := db.ExecContext(ctx, "INSERT INTO model_catalog(provider,id,body) VALUES(?1,'fixture',?2)", supplier, []byte(body)); err != nil {
				return err
			}
		}
		for i, request := range requests {
			message := canonical.NewMessage(canonical.Assistant)
			message.Usage = &canonical.Usage{Input: ptr(uint64(4)), Output: ptr(uint64(2))}
			record := canonical.NewRecord(turn, "message", &message, now.Add(time.Duration(i)*time.Second))
			if provider := request.provider; provider != nil {
				source := canonical.ModelSource{ID: provider.ID, Name: provider.Name, API: provider.Api, Authentication: provider.Authentication, Model: request.model}
				purpose := canonical.PurposeSearch
				if i == 0 {
					purpose = canonical.PurposeMain
				}
				record.Metadata.Execution = &canonical.Execution{Purpose: &purpose, Source: &source}
			}
			if _, _, err := AppendCanonicalWithProvider(ctx, db, session.ID, record, request.provider); err != nil {
				return err
			}
			if message.Usage.Pricing != nil {
				return fmt.Errorf("request %d: append changed the caller's usage", i)
			}
			records = append(records, record)
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
	if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		if _, err := db.ExecContext(ctx, "UPDATE model_catalog SET body=?1", []byte(`{"cost":{"input":4,"output":8}}`)); err != nil {
			return err
		}
		if _, err := db.ExecContext(ctx, "INSERT INTO model_catalog(provider,id,body) VALUES('xai','unavailable',?1)", []byte(`{"cost":{"input":5,"output":10}}`)); err != nil {
			return err
		}
		saved, err := currentProvider(ctx, db, main.ID, nil)
		if err != nil {
			return err
		}
		saved.Provider.BillingRateBps = ptr(uint32(0))
		if err := saveProviderConfiguration(ctx, db, *saved); err != nil {
			return err
		}
		updated := main
		updated.BillingRateBps = ptr(uint32(20_000))
		_, events, err := AppendCanonicalWithProvider(ctx, db, session.ID, records[0], &updated)
		if len(events) != 0 {
			return fmt.Errorf("reappending the same response emitted %d events", len(events))
		}
		return err
	}); err != nil {
		t.Fatal(err)
	}
	if err := storage.Node.Read(ctx, func(db *sql.Conn) error {
		statistics, err := ReadAgentStatistics(ctx, db, session.ID)
		if err != nil {
			return err
		}
		query := protocol.DefaultUsageQuery()
		query.StartMs, query.EndMs, query.Dimension = now.UnixMilli(), now.Add(time.Hour).UnixMilli(), protocol.UsageDimensionProvider
		report, err := readAgentUsage(ctx, db, node, query)
		if err != nil {
			return err
		}
		if statistics.Responses != uint64(len(requests)) || report.Totals.Responses != statistics.Responses || len(report.Days) != 1 || len(report.Requests.Items) != len(requests) {
			return fmt.Errorf("session counted %d responses, report counted %d; %d days and %d requests", statistics.Responses, report.Totals.Responses, len(report.Days), len(report.Requests.Items))
		}
		want := &protocol.UsageCost{UsdMicros: 164, Responses: 5, Breakdown: &protocol.UsageCostBreakdown{Input: 82, Output: 82}}
		for surface, cost := range map[string]*protocol.UsageCost{"session": statistics.Cost, "report": report.Totals.Cost, "day": report.Days[0].Metrics.Cost} {
			if !reflect.DeepEqual(cost, want) {
				t.Errorf("%s estimate = %+v, want %+v", surface, cost, want)
			}
		}
		groupCosts := map[protocol.ProviderID]uint64{}
		for _, group := range report.Groups {
			provider, ok := group.Key.Data.(protocol.ProviderID)
			if !ok || group.Metrics.Cost == nil {
				return fmt.Errorf("unexpected provider group: %+v", group)
			}
			groupCosts[provider] = group.Metrics.Cost.UsdMicros
		}
		wantGroups := map[protocol.ProviderID]uint64{main.ID: 44, helper.ID: 120}
		if !reflect.DeepEqual(groupCosts, wantGroups) {
			t.Errorf("provider groups = %v, want %v", groupCosts, wantGroups)
		}
		for _, item := range report.Requests.Items {
			offset := time.UnixMilli(item.Position.TimestampMs).Sub(now)
			i := int(offset / time.Second)
			if i < 0 || i >= len(requests) {
				return fmt.Errorf("unexpected request timestamp: %d", item.Position.TimestampMs)
			}
			request := requests[i]
			provider := request.provider
			if provider == nil {
				provider = &main
			}
			if item.Provider != provider.ID || item.ProviderName != provider.Name || item.Model != request.model || !reflect.DeepEqual(item.UsdMicros, request.cost) {
				t.Errorf("request %d used the wrong execution source or estimate: %+v, want %s / %s with cost %v", i, item, provider.Name, request.model, request.cost)
			}
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}
}

func TestCanonicalUsageReportsIncompleteHelperResponses(t *testing.T) {
	t.Parallel()
	storage, node, session, turn, now := agentHistoryFixture(t, protocol.WorkModeCode)
	ctx := t.Context()
	media := protocol.ConversationProvider{ID: protocol.NewProviderID(), Name: "Media", Api: protocol.ConversationModelApiResponses,
		Authentication: protocol.AuthenticationApiKey}
	search := protocol.ConversationProvider{ID: protocol.NewProviderID(), Name: "Search", Api: protocol.ConversationModelApiResponses,
		Authentication: protocol.AuthenticationApiKey, BillingRateBps: ptr(uint32(5_000))}
	requests := []struct {
		provider *protocol.ConversationProvider
		model    string
		usage    canonical.Usage
		present  protocol.ConversationUsage
		missing  [2]bool
		cost     *uint64
	}{
		{&media, "image", canonical.Usage{}, protocol.ConversationUsage{}, [2]bool{true, true}, nil},
		{&search, "search", canonical.Usage{Input: ptr(uint64(13)), CacheRead: ptr(uint64(2))}, protocol.ConversationUsage{Input: 13, CachedInput: 2}, [2]bool{false, true}, nil},
		{&search, "search", canonical.Usage{Output: ptr(uint64(0))}, protocol.ConversationUsage{}, [2]bool{true, false}, nil},
		{&search, "search", canonical.Usage{Input: ptr(uint64(4)), Output: ptr(uint64(2))}, protocol.ConversationUsage{Input: 4, Output: 2}, [2]bool{}, ptr(uint64(8))},
	}
	appendRequest := func(db *sql.Conn, i int) error {
		request := requests[i]
		message := canonical.NewMessage(canonical.Assistant)
		message.Usage = &request.usage
		record := canonical.NewRecord(turn, "message", &message, now.Add(time.Duration(i)*time.Second))
		record.Provenance.Author = "system"
		record.Metadata.Runtime = &canonical.Runtime{}
		record.Metadata.Execution = &canonical.Execution{
			Source: &canonical.ModelSource{ID: request.provider.ID, Name: request.provider.Name, API: request.provider.Api,
				Authentication: request.provider.Authentication, Model: request.model},
			Timing: &canonical.Timing{ElapsedUs: ptr(uint64(10)), FirstTokenUs: ptr(uint64(4))},
		}
		if request.provider == &media {
			record.Metadata.Execution.Media = ptr(protocol.MediaKindImage)
		} else {
			record.Metadata.Execution.Purpose = ptr(canonical.PurposeSearch)
		}
		entry, _, err := AppendCanonicalWithProvider(ctx, db, session.ID, record, request.provider)
		if err != nil {
			return err
		}
		replay, events, err := AppendCanonicalWithProvider(ctx, db, session.ID, record, request.provider)
		if err == nil && (replay.Sequence != entry.Sequence || len(events) != 0) {
			return fmt.Errorf("request %d: retry added to history: before=%d after=%d events=%d", i, entry.Sequence, replay.Sequence, len(events))
		}
		return err
	}
	if err := storage.Node.Write(ctx, func(db *sql.Conn) error {
		if _, err := db.ExecContext(ctx, "INSERT INTO model_catalog(provider,id,body) VALUES('openai','search',?1)", []byte(`{"cost":{"input":2,"output":4}}`)); err != nil {
			return err
		}
		for _, test := range []struct {
			purpose canonical.Purpose
			usage   *canonical.Usage
		}{{canonical.PurposeSearch, nil}, {canonical.PurposeMain, &canonical.Usage{}}, {canonical.PurposeMain, &canonical.Usage{Input: ptr(uint64(99))}}} {
			message := canonical.NewMessage(canonical.Assistant)
			message.Usage = test.usage
			record := canonical.NewRecord(turn, "message", &message, now)
			record.Metadata.Execution = &canonical.Execution{Purpose: &test.purpose}
			if _, _, err := AppendCanonical(ctx, db, session.ID, record); err != nil {
				return err
			}
		}
		for i := range 3 {
			if err := appendRequest(db, i); err != nil {
				return err
			}
		}
		return nil
	}); err != nil {
		t.Fatal(err)
	}

	var history *AgentStatisticsHistory
	check := func(want protocol.UsageMetrics, wantModels map[string]protocol.ConversationUsage, wantDelta int) {
		t.Helper()
		if err := storage.Node.Read(ctx, func(db *sql.Conn) error {
			statistics, next, err := ReadAgentStatisticsReusing(ctx, db, session.ID, history)
			if err != nil {
				return err
			}
			history = next
			if next.work.usageRows != wantDelta {
				t.Errorf("statistics read %d usage rows, want %d", next.work.usageRows, wantDelta)
			}
			sessionMetrics := protocol.UsageMetrics{Responses: statistics.Responses, Tokens: statistics.Usage,
				Cost: statistics.Cost, Generation: statistics.Generation, IncompleteTokenResponses: statistics.IncompleteTokenResponses}
			query := protocol.DefaultUsageQuery()
			query.StartMs, query.EndMs, query.Dimension = now.UnixMilli(), now.Add(time.Minute).UnixMilli(), protocol.UsageDimensionModel
			output, err := ReadAgentCommand(ctx, db, node, protocol.Command{Kind: "read_usage", Data: &query})
			if err != nil {
				return err
			}
			report := output.Data.(protocol.UsageReport)
			if len(report.Requests.Items) != int(want.Responses) || report.Requests.HasMore || len(report.Days) != 1 || len(report.Recent) != 1 {
				return fmt.Errorf("report has %d requests, %d day and %d recent buckets", len(report.Requests.Items), len(report.Days), len(report.Recent))
			}
			for surface, value := range map[string]protocol.UsageMetrics{"session": sessionMetrics, "report": report.Totals, "day": report.Days[0].Metrics, "recent": report.Recent[0].Metrics} {
				if !reflect.DeepEqual(value, want) {
					t.Errorf("%s metrics = %+v, want %+v", surface, value, want)
				}
			}
			if !reflect.DeepEqual(report.Days[0].Models, wantModels) || !reflect.DeepEqual(report.Recent[0].Models, wantModels) {
				t.Errorf("bucket token subtotals = %v and %v, want %v", report.Days[0].Models, report.Recent[0].Models, wantModels)
			}
			searchMetrics := want
			searchMetrics.Responses--
			searchMetrics.IncompleteTokenResponses--
			wantGroups := map[protocol.UsageKeyModelPayload]protocol.UsageMetrics{
				{Provider: media.ID, Model: "image"}:   {Responses: 1, IncompleteTokenResponses: 1},
				{Provider: search.ID, Model: "search"}: searchMetrics,
			}
			gotGroups := map[protocol.UsageKeyModelPayload]protocol.UsageMetrics{}
			for _, group := range report.Groups {
				gotGroups[group.Key.Data.(protocol.UsageKeyModelPayload)] = group.Metrics
			}
			if !reflect.DeepEqual(gotGroups, wantGroups) {
				t.Errorf("model groups = %+v, want %+v", gotGroups, wantGroups)
			}
			for i, item := range report.Requests.Items {
				request := requests[len(report.Requests.Items)-1-i]
				if item.Session != session.ID || item.Turn != turn || item.Provider != request.provider.ID || item.ProviderName != request.provider.Name || item.Model != request.model ||
					item.Tokens != request.present || [2]bool{item.MissingInputTokens, item.MissingOutputTokens} != request.missing || !reflect.DeepEqual(item.UsdMicros, request.cost) {
					t.Errorf("request %d lost its binding, reported token fields, or cost: %+v", i, item)
				}
			}
			facts, err := ReadTurnFacts(ctx, db, session.ID, turn)
			if err != nil {
				return err
			}
			var wantFacts TurnTokens
			if want.Tokens != nil {
				wantFacts = TurnTokens(*want.Tokens)
			}
			if facts.Usage != wantFacts || facts.SessionUsage != wantFacts {
				t.Errorf("turn usage = %+v, session usage = %+v, want %+v", facts.Usage, facts.SessionUsage, wantFacts)
			}
			return nil
		}); err != nil {
			t.Fatal(err)
		}
	}
	check(protocol.UsageMetrics{Responses: 3, IncompleteTokenResponses: 3, Generation: &protocol.UsageGeneration{ElapsedUs: 6, Responses: 1}},
		map[string]protocol.ConversationUsage{}, 3)
	if err := storage.Node.Write(ctx, func(db *sql.Conn) error { return appendRequest(db, 3) }); err != nil {
		t.Fatal(err)
	}
	check(protocol.UsageMetrics{Responses: 4, IncompleteTokenResponses: 3, Tokens: &requests[3].present,
		Generation: &protocol.UsageGeneration{ElapsedUs: 12, OutputTokens: 2, Responses: 2},
		Cost:       &protocol.UsageCost{UsdMicros: 8, Responses: 1, Breakdown: &protocol.UsageCostBreakdown{Input: 4, Output: 4}}},
		map[string]protocol.ConversationUsage{"search": requests[3].present}, 1)
}
