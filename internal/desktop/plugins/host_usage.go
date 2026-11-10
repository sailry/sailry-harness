package plugins

import (
	"context"
	"encoding/hex"
	"encoding/json"
	"strconv"
	"time"

	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func (owner *Host) checkUsage() error {
	scope := owner.scope()
	if scope.Surface.Kind != "workspace" && scope.Surface.Kind != "settings" || scope.Turn != nil || scope.Invocation != nil {
		return hostInvalid("usage requires a page scope")
	}
	return nil
}

func (owner *Host) usageSources() []map[string]string {
	values := []map[string]string{}
	if owner.binding.UsageSources == nil {
		return values
	}
	for _, source := range owner.binding.UsageSources() {
		if source.Client == nil {
			continue
		}
		target := source.Client.Target()
		values = append(values, map[string]string{"node": hex.EncodeToString(target[:]), "label": source.Label})
	}
	return values
}

func (owner *Host) observeUsageSources() {
	for {
		select {
		case <-owner.ctx.Done():
			return
		case _, open := <-owner.binding.UsageChanges:
			owner.mu.Lock()
			owner.usageSourcesClosed = !open
			close(owner.usageChanged)
			owner.usageChanged = make(chan struct{})
			owner.mu.Unlock()
			if !open {
				return
			}
		}
	}
}

func (owner *Host) selectUsageClient(ctx context.Context, arguments []json.RawMessage, index int) (*client.Client, context.Context, error) {
	if err := owner.checkUsage(); err != nil {
		return nil, ctx, err
	}
	selected := owner.target()
	if index < len(arguments) && string(arguments[index]) != "null" {
		text, err := hostValue[string](arguments, index)
		if err != nil {
			return nil, ctx, err
		}
		bytes, err := hex.DecodeString(text)
		if err != nil || len(bytes) != len(selected) {
			return nil, ctx, hostInvalid("invalid Node ID")
		}
		copy(selected[:], bytes)
	}
	if selected == owner.target() {
		if owner.binding.Client == nil {
			return nil, ctx, hostUnavailable("plugin Client is unavailable")
		}
		return owner.binding.Client, client.WithUsageScope(ctx, selected, owner.scope()), nil
	}
	if owner.binding.UsageSources != nil {
		for _, source := range owner.binding.UsageSources() {
			if source.Client != nil && source.Client.Target() == selected {
				return source.Client, client.WithMatchingUsageScope(ctx, selected, owner.scope()), nil
			}
		}
	}
	return nil, ctx, hostUnavailable("usage source is unavailable")
}

func (owner *Host) addUsageSDK(add hostSDKAdd) {
	add("usageSources", false, func(context.Context, []json.RawMessage) (json.RawMessage, error) {
		if err := owner.checkUsage(); err != nil {
			return nil, err
		}
		return hostJSON(owner.usageSources())
	})
	add("nextUsageSources", true, func(ctx context.Context, arguments []json.RawMessage) (json.RawMessage, error) {
		if err := owner.checkUsage(); err != nil {
			return nil, err
		}
		seen, err := firstArgument(arguments)
		if err != nil {
			return nil, err
		}
		encoded, _ := hostJSON(seen)
		for {
			owner.mu.Lock()
			changed, closed := owner.usageChanged, owner.usageSourcesClosed
			owner.mu.Unlock()
			current, _ := hostJSON(owner.usageSources())
			if string(encoded) != string(current) {
				return current, nil
			}
			if closed {
				return nil, hostUnavailable("usage sources are closed")
			}
			select {
			case <-ctx.Done():
				return nil, ctx.Err()
			case <-owner.ctx.Done():
				return nil, owner.ctx.Err()
			case <-changed:
			}
		}
	})
	add("readUsageChoices", true, func(ctx context.Context, arguments []json.RawMessage) (json.RawMessage, error) {
		selected, scoped, err := owner.selectUsageClient(ctx, arguments, 1)
		if err != nil {
			return nil, err
		}
		query, err := hostUsageQuery(arguments)
		if err != nil {
			return nil, err
		}
		report, err := selected.ReadUsage(scoped, query)
		if err != nil {
			return nil, err
		}
		value, err := hostJSON(report)
		if err != nil {
			return nil, err
		}
		decoded, err := decodeHostJSON(value)
		if err != nil {
			return nil, err
		}
		return hostJSON(exactHostUsage(decoded))
	})
	add("readUsageInventory", true, func(ctx context.Context, arguments []json.RawMessage) (json.RawMessage, error) {
		selected, scoped, err := owner.selectUsageClient(ctx, arguments, 0)
		if err != nil {
			return nil, err
		}
		query := protocol.UsageQuery{StartMs: 0, EndMs: 1, Dimension: protocol.UsageDimensionModel}
		if _, err := selected.ReadUsage(scoped, query); err != nil {
			return nil, err
		}
		output, err := selected.Execute(ctx, selected.Prepare(protocol.Command{Kind: "snapshot"}))
		if err != nil {
			return nil, err
		}
		if output.Kind != "snapshot" {
			return nil, hostInvalid("Node snapshot expected")
		}
		body, _ := hostJSON(output.Data)
		var snapshot protocol.Snapshot
		if json.Unmarshal(body, &snapshot) != nil {
			return nil, hostInvalid("Node snapshot expected")
		}
		projects := []map[string]any{}
		for _, project := range snapshot.Projects {
			projects = append(projects, map[string]any{"id": project.ID, "name": project.Name})
		}
		providers := []map[string]any{}
		for _, provider := range snapshot.Providers {
			models := []string{}
			for _, model := range provider.Models {
				models = append(models, model.ID)
			}
			providers = append(providers, map[string]any{"id": provider.ID, "name": provider.Name, "icon": nil, "models": models})
		}
		target := snapshot.Node
		return hostJSON(map[string]any{"node": hex.EncodeToString(target[:]), "projects": projects, "providers": providers})
	})
	add("watchUsage", false, func(ctx context.Context, arguments []json.RawMessage) (json.RawMessage, error) {
		selected, scope, err := owner.selectUsageClient(owner.ctx, arguments, 2)
		if err != nil {
			return nil, err
		}
		query, err := hostUsageQuery(arguments)
		if err != nil {
			return nil, err
		}
		all, err := hostOptional(arguments, 1, false)
		if err != nil {
			return nil, err
		}
		if all && (len(query.Projects) != 0 || len(query.Worktrees) != 0 || len(query.Providers) != 0 || len(query.Models) != 0) {
			return nil, hostInvalid("cross-Node usage requires an unfiltered resource scope")
		}
		body, _ := hostJSON(query)
		owner.mu.Lock()
		previous := owner.observers["usage"]
		delete(owner.observers, "usage")
		delete(owner.feeds, "usage")
		owner.mu.Unlock()
		if previous != nil {
			_ = previous.Close()
		}
		clients := []*client.Client{selected}
		if all {
			clients = []*client.Client{owner.binding.Client}
			scope = client.WithUsageScope(owner.ctx, owner.target(), owner.scope())
			if owner.binding.UsageSources != nil {
				for _, source := range owner.binding.UsageSources() {
					if source.Client != nil && source.Client.Target() != owner.target() {
						clients = append(clients, source.Client)
						scope = client.WithMatchingUsageScope(scope, source.Client.Target(), owner.scope())
					}
				}
			}
		}
		observer, err := client.WatchOverview(scope, clients, body)
		if err != nil {
			return nil, err
		}
		feed := &hostFeed{changed: make(chan struct{}), observer: observer, query: &query, all: all}
		owner.mu.Lock()
		owner.feeds["usage"], owner.observers["usage"] = feed, observer
		owner.mu.Unlock()
		go func() {
			for {
				value, err := observer.Next(owner.ctx)
				feed.mu.Lock()
				feed.latest, feed.err = value, err
				feed.revision++
				close(feed.changed)
				feed.changed = make(chan struct{})
				feed.mu.Unlock()
				if err != nil {
					return
				}
			}
		}()
		return json.RawMessage("null"), nil
	})
	add("nextUsageChange", true, func(ctx context.Context, arguments []json.RawMessage) (json.RawMessage, error) {
		if err := owner.checkUsage(); err != nil {
			return nil, err
		}
		seen, err := hostValue[string](arguments, 0)
		if err != nil {
			return nil, err
		}
		owner.mu.Lock()
		feed := owner.feeds["usage"]
		owner.mu.Unlock()
		if feed == nil {
			return nil, hostUnavailable("usage observer is unavailable")
		}
		for {
			feed.mu.Lock()
			body, failure, changed, revision, all := append(json.RawMessage{}, feed.latest...), feed.err, feed.changed, feed.revision, feed.all
			feed.mu.Unlock()
			if failure != nil {
				return nil, failure
			}
			cursor := strconv.FormatUint(revision, 10)
			if cursor != seen {
				var value any
				if len(body) != 0 {
					value, err = decodeHostJSON(body)
					if err != nil {
						return nil, err
					}
					if !all {
						rows, _ := hostAt(value, "sources").([]any)
						if len(rows) > 0 {
							value = hostAt(rows[0], "view")
						} else {
							value = nil
						}
					}
				}
				return hostJSON(map[string]any{"cursor": cursor, "all": all, "view": exactHostUsage(value)})
			}
			select {
			case <-ctx.Done():
				return nil, ctx.Err()
			case <-owner.ctx.Done():
				return nil, owner.ctx.Err()
			case <-changed:
			}
		}
	})
	add("refreshUsage", false, func(ctx context.Context, _ []json.RawMessage) (json.RawMessage, error) {
		if err := owner.checkUsage(); err != nil {
			return nil, err
		}
		owner.mu.Lock()
		observer := owner.observers["usage"]
		owner.mu.Unlock()
		if observer != nil {
			if err := observer.Control(ctx, "refresh", json.RawMessage("null")); err != nil {
				return nil, err
			}
		}
		return json.RawMessage("null"), nil
	})
}

func hostUsageQuery(arguments []json.RawMessage) (protocol.UsageQuery, error) {
	value, err := firstArgument(arguments)
	if err != nil {
		return protocol.UsageQuery{}, err
	}
	for _, path := range [][]string{{"start_ms"}, {"end_ms"}, {"utc_offset_minutes"}, {"before", "timestamp_ms"}, {"before", "sequence"}} {
		if text, ok := hostAt(value, path...).(string); ok {
			if _, err := strconv.ParseInt(text, 10, 64); err != nil && path[len(path)-1] != "sequence" {
				return protocol.UsageQuery{}, hostInvalid("invalid usage timestamp")
			}
			if path[len(path)-1] == "sequence" {
				if _, err := strconv.ParseUint(text, 10, 64); err != nil {
					return protocol.UsageQuery{}, hostInvalid("invalid usage cursor")
				}
			}
			hostSet(value, json.Number(text), path...)
		}
	}
	if node, ok := hostAt(value, "before", "node").(string); ok {
		bytes, err := hex.DecodeString(node)
		if err != nil || len(bytes) != 32 {
			return protocol.UsageQuery{}, hostInvalid("invalid Node ID")
		}
		var id protocol.NodeID
		copy(id[:], bytes)
		hostSet(value, id, "before", "node")
	}
	object, ok := value.(map[string]any)
	if !ok {
		return protocol.UsageQuery{}, hostInvalid("invalid usage query")
	}
	if _, exists := object["utc_offset_minutes"]; !exists {
		_, offset := time.Now().Zone()
		object["utc_offset_minutes"] = offset / 60
	}
	body, err := hostJSON(value)
	var query protocol.UsageQuery
	if err != nil || json.Unmarshal(body, &query) != nil {
		return query, hostInvalid("invalid usage query")
	}
	return query, nil
}

func exactHostUsage(value any) any {
	switch typed := value.(type) {
	case json.Number:
		if _, err := strconv.ParseInt(typed.String(), 10, 64); err == nil {
			return typed.String()
		}
		if _, err := strconv.ParseUint(typed.String(), 10, 64); err == nil {
			return typed.String()
		}
	case []any:
		for index, nested := range typed {
			typed[index] = exactHostUsage(nested)
		}
	case map[string]any:
		for key, nested := range typed {
			if key == "node" {
				body, _ := hostJSON(nested)
				var node protocol.NodeID
				if json.Unmarshal(body, &node) == nil {
					typed[key] = hex.EncodeToString(node[:])
					continue
				}
			}
			typed[key] = exactHostUsage(nested)
		}
	}
	return value
}
