package client

import (
	"bytes"
	"cmp"
	"context"
	"encoding/json"
	"math"
	"reflect"
	"slices"

	"github.com/sailry/sailry-harness/internal/protocol"
)

type usageView struct {
	Report     *protocol.UsageReport `json:"report"`
	Connected  bool                  `json:"connected"`
	Refreshing bool                  `json:"refreshing"`
	Error      *protocol.Fault       `json:"error"`
}

func (client *Client) watchUsageView(ctx context.Context, query protocol.UsageQuery) *Observer {
	observer := newObserver(ctx)
	_, scoped := usageScopeFor(ctx, client.Target())
	requested := make(chan bool, 1)
	state := usageView{}
	forcedRefresh := false
	wake := func() {
		select {
		case requested <- false:
		default:
		}
	}
	observer.release = client.OnChange(wake)
	observer.read = func() (any, error) {
		observer.mu.Lock()
		defer observer.mu.Unlock()
		return copyValue(state), nil
	}
	observer.control = func(_ context.Context, method string, _ json.RawMessage) error {
		if method != "refresh" {
			return fault(protocol.ErrorCodeInvalidRequest, "unknown usage control")
		}
		observer.mu.Lock()
		forcedRefresh = true
		observer.mu.Unlock()
		select {
		case requested <- true:
		default:
		}
		return nil
	}
	observer.run(func() {
		err := client.startObserver(observer.ctx, false)
		if err != nil {
			observer.mu.Lock()
			state.Error = observerFault(err)
			observer.mu.Unlock()
			observer.notify()
		}
		wake()
	})
	observer.run(func() {
		for {
			var forced bool
			select {
			case <-observer.ctx.Done():
				return
			case forced = <-requested:
			}
			connected := client.Connected()
			snapshot := client.Snapshot()
			observer.mu.Lock()
			forced = forced || forcedRefresh
			forcedRefresh = false
			wasConnected := state.Connected
			state.Connected = connected
			if !connected {
				state.Error = observerFault(client.Err())
				observer.mu.Unlock()
				observer.notify()
				continue
			}
			if !forced && wasConnected && state.Report != nil && state.Report.Cursor >= snapshot.Cursor {
				observer.mu.Unlock()
				observer.notify()
				continue
			}
			state.Refreshing = true
			observer.mu.Unlock()
			observer.notify()
			report, err := client.ReadUsage(observer.ctx, query)
			observer.mu.Lock()
			state.Connected = client.Connected()
			state.Refreshing = false
			if err == nil && state.Report != nil && report.Cursor < state.Report.Cursor {
				err = fault(protocol.ErrorCodeInvalidRequest, "usage report cursor moved backwards")
			}
			if err == nil {
				state.Report = &report
				state.Error = observerFault(client.Err())
			} else {
				if scoped && slices.Contains([]protocol.ErrorCode{
					protocol.ErrorCodeNotConfigured, protocol.ErrorCodeNotFound,
					protocol.ErrorCodePermissionDenied, protocol.ErrorCodeRevisionConflict,
				}, faultCode(err)) {
					state.Report = nil
				}
				state.Error = observerFault(err)
			}
			observer.mu.Unlock()
			observer.notify()
		}
	})
	return observer
}

func (client *Client) ReadUsage(ctx context.Context, query protocol.UsageQuery) (protocol.UsageReport, error) {
	request := client.Prepare(protocol.Command{Kind: "read_usage", Data: query})
	if selected, scoped := usageScopeFor(ctx, client.Target()); scoped {
		scope := copyValue(selected.context)
		if selected.resolve {
			output, err := client.Do(ctx, protocol.Command{Kind: "list_plugins"})
			if err != nil {
				return protocol.UsageReport{}, err
			}
			packages, err := payload[[]protocol.PluginSummary](output.Data)
			if err != nil || output.Kind != "plugins" {
				return protocol.UsageReport{}, fault(protocol.ErrorCodeInternal, "plugin inventory response expected")
			}
			index := slices.IndexFunc(packages, func(summary protocol.PluginSummary) bool {
				return summary.Name == scope.Package.Name && summary.Digest == scope.Package.Digest
			})
			if index < 0 {
				return protocol.UsageReport{}, fault(protocol.ErrorCodeNotConfigured, "matching usage package is unavailable")
			}
			packageInfo := packages[index]
			scope.Package = protocol.PluginReference{Name: packageInfo.Name, Digest: packageInfo.Digest, SettingsRevision: packageInfo.SettingsRevision}
		}
		request.Plugin = &scope
	}
	output, err := client.Execute(ctx, request)
	if err != nil {
		return protocol.UsageReport{}, err
	}
	report, err := payload[protocol.UsageReport](output.Data)
	if err != nil || output.Kind != "usage" {
		return report, fault(protocol.ErrorCodeInternal, "usage report response expected")
	}
	if report.Node != client.Target() || !reflect.DeepEqual(report.Query, query) || len(report.Requests.Items) > 20 {
		return report, fault(protocol.ErrorCodeInvalidRequest, "usage report belongs to another Node or query")
	}
	for index, request := range report.Requests.Items {
		position := request.Position
		if position.Node != report.Node || position.TimestampMs < query.StartMs || position.TimestampMs >= query.EndMs ||
			query.Before != nil && compareUsagePosition(position, *query.Before) >= 0 ||
			index > 0 && compareUsagePosition(report.Requests.Items[index-1].Position, position) <= 0 {
			return report, fault(protocol.ErrorCodeInvalidRequest, "usage request page is invalid")
		}
	}
	return report, nil
}

type usageSource struct {
	Node protocol.NodeID `json:"node"`
	View usageView       `json:"view"`
}

type usageScopedGroup struct {
	Node  protocol.NodeID     `json:"node"`
	Group protocol.UsageGroup `json:"group"`
}

type usageSummary struct {
	Totals   protocol.UsageMetrics `json:"totals"`
	Days     []protocol.UsagePoint `json:"days"`
	Recent   []protocol.UsagePoint `json:"recent"`
	Groups   []usageScopedGroup    `json:"groups"`
	Requests protocol.UsagePage    `json:"requests"`
}

type usageOverview struct {
	Sources  []usageSource   `json:"sources"`
	Summary  *usageSummary   `json:"summary"`
	Complete bool            `json:"complete"`
	Error    *protocol.Fault `json:"error"`
}

func WatchOverview(ctx context.Context, clients []*Client, argument json.RawMessage) (*Observer, error) {
	var query protocol.UsageQuery
	if json.Unmarshal(argument, &query) != nil {
		return nil, fault(protocol.ErrorCodeInvalidRequest, "invalid usage query")
	}
	unique := map[protocol.NodeID]*Client{}
	for _, client := range clients {
		unique[client.Target()] = client
	}
	if len(unique) == 0 {
		return nil, fault(protocol.ErrorCodeInvalidRequest, "usage overview requires an execution Node")
	}
	observer := newObserver(ctx)
	sources := map[protocol.NodeID]usageView{}
	children := make([]*Observer, 0, len(unique))
	for node := range unique {
		sources[node] = usageView{}
	}
	for node, client := range unique {
		child := client.watchUsageView(observer.ctx, query)
		children = append(children, child)
		observer.run(func() {
			for {
				data, err := child.Next(observer.ctx)
				if err != nil {
					return
				}
				var value usageView
				if json.Unmarshal(data, &value) != nil {
					return
				}
				observer.mu.Lock()
				sources[node] = value
				observer.mu.Unlock()
				observer.notify()
			}
		})
	}
	observer.release = func() {
		for _, child := range children {
			_ = child.Close()
		}
	}
	observer.read = func() (any, error) {
		observer.mu.Lock()
		defer observer.mu.Unlock()
		value := usageOverview{Sources: make([]usageSource, 0, len(sources)), Complete: true}
		for node, source := range sources {
			value.Sources = append(value.Sources, usageSource{Node: node, View: copyValue(source)})
			value.Complete = value.Complete && source.Connected && !source.Refreshing && source.Error == nil && source.Report != nil
		}
		slices.SortFunc(value.Sources, func(left, right usageSource) int { return bytes.Compare(left.Node[:], right.Node[:]) })
		summary, err := summarizeUsage(value.Sources)
		value.Summary, value.Error = summary, observerFault(err)
		value.Complete = value.Complete && err == nil
		return value, nil
	}
	observer.control = func(ctx context.Context, method string, data json.RawMessage) error {
		if method != "refresh" {
			return fault(protocol.ErrorCodeInvalidRequest, "unknown usage overview control")
		}
		for _, child := range children {
			if err := child.Control(ctx, method, data); err != nil {
				return err
			}
		}
		return nil
	}
	observer.notify()
	return observer, nil
}

func summarizeUsage(sources []usageSource) (*usageSummary, error) {
	value := &usageSummary{Groups: []usageScopedGroup{}, Requests: protocol.DefaultUsagePage()}
	days, recent := map[int64]protocol.UsagePoint{}, map[int64]protocol.UsagePoint{}
	requests := map[protocol.UsagePosition]protocol.UsageRequest{}
	available := false
	for _, source := range sources {
		report := source.View.Report
		if report == nil {
			continue
		}
		available = true
		value.Requests.HasMore = value.Requests.HasMore || report.Requests.HasMore
		if err := addUsageMetrics(&value.Totals, report.Totals); err != nil {
			return nil, err
		}
		for _, request := range report.Requests.Items {
			requests[request.Position] = request
		}
		for index, points := range [][]protocol.UsagePoint{report.Days, report.Recent} {
			target := days
			if index == 1 {
				target = recent
			}
			for _, point := range points {
				total, exists := target[point.StartMs]
				if !exists {
					total = protocol.DefaultUsagePoint()
					total.StartMs = point.StartMs
				}
				if err := addUsageMetrics(&total.Metrics, point.Metrics); err != nil {
					return nil, err
				}
				for model, tokens := range point.Models {
					combined := total.Models[model]
					if err := addUsageTokens(&combined, tokens); err != nil {
						return nil, err
					}
					total.Models[model] = combined
				}
				target[point.StartMs] = total
			}
		}
		for _, group := range report.Groups {
			value.Groups = append(value.Groups, usageScopedGroup{Node: source.Node, Group: group})
		}
	}
	if !available {
		return nil, nil
	}
	value.Days, value.Recent = make([]protocol.UsagePoint, 0, len(days)), make([]protocol.UsagePoint, 0, len(recent))
	for _, point := range days {
		value.Days = append(value.Days, point)
	}
	for _, point := range recent {
		value.Recent = append(value.Recent, point)
	}
	slices.SortFunc(value.Days, func(left, right protocol.UsagePoint) int { return cmp.Compare(left.StartMs, right.StartMs) })
	slices.SortFunc(value.Recent, func(left, right protocol.UsagePoint) int { return cmp.Compare(left.StartMs, right.StartMs) })
	for _, request := range requests {
		value.Requests.Items = append(value.Requests.Items, request)
	}
	slices.SortFunc(value.Requests.Items, func(left, right protocol.UsageRequest) int {
		return -compareUsagePosition(left.Position, right.Position)
	})
	if len(value.Requests.Items) > 20 {
		value.Requests.Items = value.Requests.Items[:20]
		value.Requests.HasMore = true
	}
	return value, nil
}

func compareUsagePosition(left, right protocol.UsagePosition) int {
	if result := cmp.Compare(left.TimestampMs, right.TimestampMs); result != 0 {
		return result
	}
	if result := bytes.Compare(left.Node[:], right.Node[:]); result != 0 {
		return result
	}
	return cmp.Compare(left.Sequence, right.Sequence)
}

func addUsageCounter(target *uint64, value uint64) error {
	if value > math.MaxUint64-*target {
		return fault(protocol.ErrorCodeInvalidRequest, "usage overview exceeds the counter limit")
	}
	*target += value
	return nil
}

func addUsageTokens(target *protocol.ConversationUsage, value protocol.ConversationUsage) error {
	for _, field := range []struct {
		target *uint64
		value  uint64
	}{
		{&target.Input, value.Input}, {&target.Output, value.Output}, {&target.CachedInput, value.CachedInput}, {&target.Reasoning, value.Reasoning},
	} {
		if err := addUsageCounter(field.target, field.value); err != nil {
			return err
		}
	}
	return nil
}

func addUsageMetrics(target *protocol.UsageMetrics, value protocol.UsageMetrics) error {
	if err := addUsageCounter(&target.Responses, value.Responses); err != nil {
		return err
	}
	if value.Tokens != nil {
		if target.Tokens == nil {
			target.Tokens = &protocol.ConversationUsage{}
		}
		if err := addUsageTokens(target.Tokens, *value.Tokens); err != nil {
			return err
		}
	}
	if value.Generation != nil {
		if target.Generation == nil {
			target.Generation = &protocol.UsageGeneration{}
		}
		for _, field := range []struct {
			target *uint64
			value  uint64
		}{
			{&target.Generation.ElapsedUs, value.Generation.ElapsedUs}, {&target.Generation.OutputTokens, value.Generation.OutputTokens}, {&target.Generation.Responses, value.Generation.Responses},
		} {
			if err := addUsageCounter(field.target, field.value); err != nil {
				return err
			}
		}
	}
	if value.Cost != nil {
		if target.Cost == nil {
			value := copyValue(*value.Cost)
			target.Cost = &value
			return nil
		}
		if err := addUsageCounter(&target.Cost.UsdMicros, value.Cost.UsdMicros); err != nil {
			return err
		}
		if err := addUsageCounter(&target.Cost.Responses, value.Cost.Responses); err != nil {
			return err
		}
		if target.Cost.Breakdown == nil || value.Cost.Breakdown == nil {
			target.Cost.Breakdown = nil
		} else {
			for _, field := range []struct {
				target *uint64
				value  uint64
			}{
				{&target.Cost.Breakdown.Input, value.Cost.Breakdown.Input}, {&target.Cost.Breakdown.Output, value.Cost.Breakdown.Output},
				{&target.Cost.Breakdown.CacheRead, value.Cost.Breakdown.CacheRead}, {&target.Cost.Breakdown.CacheWrite, value.Cost.Breakdown.CacheWrite},
			} {
				if err := addUsageCounter(field.target, field.value); err != nil {
					return err
				}
			}
		}
	}
	return nil
}
