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

// sampledUsage is the sample the usage calls answer from, nil while the
// Nodes answer.
func (owner *Host) sampledUsage() *UsageSample {
	owner.mu.Lock()
	defer owner.mu.Unlock()
	return owner.usageSample
}

// SetUsageSample has the usage calls answer from sample, or from the Nodes
// again when it is nil. It wakes the calls that wait for the sources and
// moves the page's watch over, so an open page draws the other figures.
func (owner *Host) SetUsageSample(sample *UsageSample) {
	owner.mu.Lock()
	owner.usageSample = sample
	watched := owner.usageWatch != nil
	close(owner.usageChanged)
	owner.usageChanged = make(chan struct{})
	owner.mu.Unlock()
	if !watched {
		return
	}
	// A Node's watch opens off the caller's frame.
	go func() {
		owner.mu.Lock()
		arguments := owner.usageWatch
		owner.mu.Unlock()
		_ = owner.watchUsage(arguments, true)
	}()
}

func (owner *Host) usageSources() []map[string]string {
	values := []map[string]string{}
	if owner.sampledUsage() != nil {
		target := owner.target()
		return append(values, map[string]string{"node": hex.EncodeToString(target[:]), "label": target.String()})
	}
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

// usageNode is the Node a usage call names at index, this device's by
// default.
func (owner *Host) usageNode(arguments []json.RawMessage, index int) (protocol.NodeID, error) {
	selected := owner.target()
	if err := owner.checkUsage(); err != nil {
		return selected, err
	}
	if index < len(arguments) && string(arguments[index]) != "null" {
		text, err := hostValue[string](arguments, index)
		if err != nil {
			return selected, err
		}
		bytes, err := hex.DecodeString(text)
		if err != nil || len(bytes) != len(selected) {
			return selected, hostInvalid("invalid Node ID")
		}
		copy(selected[:], bytes)
	}
	return selected, nil
}

// sampleNode is the Node a usage call names while a sample answers: this
// device, its only source.
func (owner *Host) sampleNode(arguments []json.RawMessage, index int) (protocol.NodeID, error) {
	selected, err := owner.usageNode(arguments, index)
	if err == nil && selected != owner.target() {
		err = hostUnavailable("usage source is unavailable")
	}
	return selected, err
}

func (owner *Host) selectUsageClient(ctx context.Context, arguments []json.RawMessage, index int) (*client.Client, context.Context, error) {
	selected, err := owner.usageNode(arguments, index)
	if err != nil {
		return nil, ctx, err
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

// exactUsage is a usage value as the plugin reads it: its integers exact.
func exactUsage(value any) (json.RawMessage, error) {
	body, err := hostJSON(value)
	if err != nil {
		return nil, err
	}
	decoded, err := decodeHostJSON(body)
	if err != nil {
		return nil, err
	}
	return hostJSON(exactHostUsage(decoded))
}

// usageInventory is what a usage page's filters choose between on node.
func usageInventory(node protocol.NodeID, listed []protocol.Project, offered []protocol.ConversationProvider) map[string]any {
	projects := []map[string]any{}
	for _, project := range listed {
		projects = append(projects, map[string]any{"id": project.ID, "name": project.Name})
	}
	providers := []map[string]any{}
	for _, provider := range offered {
		models := []string{}
		for _, model := range provider.Models {
			models = append(models, model.ID)
		}
		providers = append(providers, map[string]any{"id": provider.ID, "name": provider.Name, "icon": nil, "models": models})
	}
	return map[string]any{"node": hex.EncodeToString(node[:]), "projects": projects, "providers": providers}
}

// sampleUsage is what a watch of the sample observes: the overview a Node's
// watch yields, of the one source, read once.
func sampleUsage(sample *UsageSample, node protocol.NodeID, query protocol.UsageQuery) json.RawMessage {
	view := map[string]any{"report": nil, "connected": true, "refreshing": false, "error": nil}
	overview := map[string]any{"sources": []any{map[string]any{"node": node, "view": view}}, "summary": nil, "complete": true, "error": nil}
	if report, err := sample.Report(node, query); err != nil {
		fault := hostFault(err)
		view["error"], overview["error"], overview["complete"] = fault, fault, false
	} else {
		groups := []any{}
		for _, group := range report.Groups {
			groups = append(groups, map[string]any{"node": node, "group": group})
		}
		view["report"] = report
		overview["summary"] = map[string]any{"totals": report.Totals, "days": report.Days, "recent": report.Recent, "groups": groups, "requests": report.Requests}
	}
	body, _ := hostJSON(overview)
	return body
}

// watchUsage opens the page's usage watch: on the sample while one answers,
// else on the Nodes. moved runs the page's last watch again after the sample
// changed or was read anew: the new feed takes the place of the one before
// under a later revision, so a call that waits goes on with it and never
// sees the old one end.
func (owner *Host) watchUsage(arguments []json.RawMessage, moved bool) error {
	owner.usageWatching.Lock()
	defer owner.usageWatching.Unlock()
	if err := owner.checkUsage(); err != nil {
		return err
	}
	query, err := hostUsageQuery(arguments)
	if err != nil {
		return err
	}
	all, err := hostOptional(arguments, 1, false)
	if err != nil {
		return err
	}
	if all && (len(query.Projects) != 0 || len(query.Worktrees) != 0 || len(query.Providers) != 0 || len(query.Models) != 0) {
		return hostInvalid("cross-Node usage requires an unfiltered resource scope")
	}
	sample := owner.sampledUsage()
	var selected *client.Client
	var node protocol.NodeID
	scope := owner.ctx
	if sample != nil {
		node, err = owner.sampleNode(arguments, 2)
	} else {
		selected, scope, err = owner.selectUsageClient(owner.ctx, arguments, 2)
	}
	if err != nil && !moved {
		return err
	}
	if !moved {
		kept := make([]json.RawMessage, len(arguments))
		for index, argument := range arguments {
			kept[index] = append(json.RawMessage{}, argument...)
		}
		owner.mu.Lock()
		previous := owner.observers["usage"]
		delete(owner.observers, "usage")
		delete(owner.feeds, "usage")
		owner.usageWatch = kept
		owner.mu.Unlock()
		if previous != nil {
			_ = previous.Close()
		}
	}
	feed := &hostFeed{changed: make(chan struct{}), query: &query, all: all}
	switch {
	case err != nil:
	case sample != nil:
		feed.latest = sampleUsage(sample, node, query)
	default:
		body, _ := hostJSON(query)
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
		feed.observer, err = client.WatchOverview(scope, clients, body)
	}
	if err != nil {
		if !moved {
			return err
		}
		// The page waits for the other figures: it learns why none come.
		feed.err = err
	}
	observer := feed.observer
	owner.mu.Lock()
	replaced, previous := owner.feeds["usage"], owner.observers["usage"]
	if replaced != nil {
		replaced.mu.Lock()
		feed.revision = replaced.revision + 1
		replaced.replaced = true
		close(replaced.changed)
		replaced.changed = make(chan struct{})
		replaced.mu.Unlock()
	}
	owner.feeds["usage"] = feed
	delete(owner.observers, "usage")
	if observer != nil {
		owner.observers["usage"] = observer
	}
	owner.mu.Unlock()
	if previous != nil {
		_ = previous.Close()
	}
	if observer != nil {
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
	}
	return nil
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
		if sample := owner.sampledUsage(); sample != nil {
			node, err := owner.sampleNode(arguments, 1)
			if err != nil {
				return nil, err
			}
			query, err := hostUsageQuery(arguments)
			if err != nil {
				return nil, err
			}
			report, err := sample.Report(node, query)
			if err != nil {
				return nil, err
			}
			return exactUsage(report)
		}
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
		return exactUsage(report)
	})
	add("readUsageInventory", true, func(ctx context.Context, arguments []json.RawMessage) (json.RawMessage, error) {
		if sample := owner.sampledUsage(); sample != nil {
			node, err := owner.sampleNode(arguments, 0)
			if err != nil {
				return nil, err
			}
			projects, providers := sample.Inventory()
			return hostJSON(usageInventory(node, projects, providers))
		}
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
		return hostJSON(usageInventory(snapshot.Node, snapshot.Projects, snapshot.Providers))
	})
	add("watchUsage", false, func(_ context.Context, arguments []json.RawMessage) (json.RawMessage, error) {
		if err := owner.watchUsage(arguments, false); err != nil {
			return nil, err
		}
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
		for {
			if feed == nil {
				return nil, hostUnavailable("usage observer is unavailable")
			}
			feed.mu.Lock()
			body, failure, changed, revision, all, replaced := append(json.RawMessage{}, feed.latest...), feed.err, feed.changed, feed.revision, feed.all, feed.replaced
			feed.mu.Unlock()
			if replaced {
				// Its successor took its place when it was marked.
				owner.mu.Lock()
				feed = owner.feeds["usage"]
				owner.mu.Unlock()
				continue
			}
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
		observer, sample, arguments := owner.observers["usage"], owner.usageSample, owner.usageWatch
		owner.mu.Unlock()
		if sample != nil {
			// The sample is read anew, which shows what its clock has reached.
			if arguments != nil {
				return json.RawMessage("null"), owner.watchUsage(arguments, true)
			}
			return json.RawMessage("null"), nil
		}
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
