package plugins

import (
	"context"
	"encoding/hex"
	"encoding/json"
	"errors"
	"runtime"
	"strings"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// A usage sample answers every usage call in place of the Nodes, as one
// source, with a Node's shapes; setting another or none moves an open watch
// over under a later cursor, and without one the calls go to the Node.
func testHostUsageSample(t *testing.T) {
	ctx, cancel := context.WithTimeout(t.Context(), 10*time.Second)
	defer cancel()
	scope := protocol.DefaultPluginContext()
	scope.Package = protocol.PluginReference{Name: "usage-fixture", Digest: strings.Repeat("a", 64), SettingsRevision: 1}
	scope.Surface.Kind = "settings"
	project, provider := protocol.NewProjectID(), protocol.NewProviderID()
	// sample reports responses, and one response for a query that names a
	// model, so a call shows that its query arrived.
	sample := func(responses uint64) *UsageSample {
		return &UsageSample{
			Report: func(node protocol.NodeID, query protocol.UsageQuery) (protocol.UsageReport, error) {
				report := protocol.DefaultUsageReport()
				report.Node, report.Query, report.Cursor = node, query, 1<<62
				report.Totals.Responses = responses
				if len(query.Models) != 0 {
					report.Totals.Responses = 1
				}
				report.Totals.Tokens = &protocol.ConversationUsage{Input: 9_007_199_254_740_993}
				group := protocol.DefaultUsageGroup()
				group.Key, group.Metrics = protocol.UsageKey{Kind: "provider", Data: provider}, report.Totals
				report.Groups = append(report.Groups, group)
				report.Resources = append(report.Resources, group.Key)
				return report, nil
			},
			Inventory: func() ([]protocol.Project, []protocol.ConversationProvider) {
				listed, offered, model := protocol.DefaultProject(), protocol.DefaultConversationProvider(), protocol.DefaultConversationModel()
				listed.ID, listed.Name = project, "harbor"
				model.ID = "fixture-model"
				offered.ID, offered.Name, offered.Models = provider, "Fixture", []protocol.ConversationModel{model}
				return []protocol.Project{listed}, []protocol.ConversationProvider{offered}
			},
		}
	}
	type answer struct {
		value any
		err   error
	}
	call := func(owner *Host, name string, arguments ...string) (any, error) {
		t.Helper()
		for _, export := range owner.sdkModule().Exports {
			if export.Name != name {
				continue
			}
			operation := export.Sync
			if operation == nil {
				operation = export.Async
			}
			raw := make([]json.RawMessage, len(arguments))
			for index, argument := range arguments {
				raw[index] = json.RawMessage(argument)
			}
			body, err := operation(ctx, raw)
			if err != nil {
				return nil, err
			}
			return decodeHostJSON(body)
		}
		t.Fatalf("the SDK has no %s", name)
		return nil, nil
	}
	later := func(owner *Host, name string, arguments ...string) <-chan answer {
		done := make(chan answer, 1)
		go func() {
			value, err := call(owner, name, arguments...)
			done <- answer{value, err}
		}()
		return done
	}
	wait := func(done <-chan answer) answer {
		t.Helper()
		select {
		case got := <-done:
			return got
		case <-ctx.Done():
			t.Fatal("a usage call that waits was not woken")
			return answer{}
		}
	}
	unavailable := func(err error) bool {
		var fault *protocol.Fault
		return errors.As(err, &fault) && fault.Code == protocol.ErrorCodeUnavailable
	}
	quote := func(text string) string {
		body, _ := json.Marshal(text)
		return string(body)
	}
	const query = `{"start_ms":"1000","end_ms":"2000","utc_offset_minutes":480,"dimension":"model","projects":[],"worktrees":[],"providers":[],"models":[],"before":null}`
	const filtered = `{"start_ms":"1000","end_ms":"2000","utc_offset_minutes":480,"dimension":"model","projects":[],"worktrees":[],"providers":[],"models":["fixture-model"],"before":null}`

	t.Run("answers", func(t *testing.T) {
		target := protocol.NodeID{2}
		node := hex.EncodeToString(target[:])
		owner := NewHost(HostBinding{Context: ctx, Target: target, Scope: scope, UsageSample: sample(7)})
		defer owner.Close()

		sources, err := call(owner, "usageSources")
		listed, _ := sources.([]any)
		if err != nil || len(listed) != 1 || hostAt(listed[0], "node") != node || hostAt(listed[0], "label") != target.String() {
			t.Fatalf("the sample's sources are %#v, %v", sources, err)
		}
		before, _ := hostJSON(sources)

		report, err := call(owner, "readUsageChoices", query, quote(node))
		if err != nil || hostAt(report, "node") != node || hostAt(report, "cursor") != "4611686018427387904" ||
			hostAt(report, "totals", "responses") != "7" || hostAt(report, "totals", "tokens", "input") != "9007199254740993" ||
			hostAt(report, "query", "start_ms") != "1000" || hostAt(report, "query", "utc_offset_minutes") != "480" {
			t.Fatalf("the sample's report is %#v, %v", report, err)
		}
		if groups, _ := hostAt(report, "groups").([]any); len(groups) != 1 || hostAt(groups[0], "key", "kind") != "provider" || hostAt(groups[0], "key", "data") != provider.String() {
			t.Fatalf("the sample's groups are %#v", hostAt(report, "groups"))
		}
		if report, err := call(owner, "readUsageChoices", filtered); err != nil || hostAt(report, "totals", "responses") != "1" {
			t.Fatalf("the query did not reach the sample: %#v, %v", report, err)
		}
		other := protocol.NodeID{3}
		if _, err := call(owner, "readUsageChoices", query, quote(hex.EncodeToString(other[:]))); !unavailable(err) {
			t.Fatalf("the sample answered for another Node: %v", err)
		}

		inventory, err := call(owner, "readUsageInventory", quote(node))
		projects, _ := hostAt(inventory, "projects").([]any)
		providers, _ := hostAt(inventory, "providers").([]any)
		if err != nil || hostAt(inventory, "node") != node || len(projects) != 1 || hostAt(projects[0], "id") != project.String() || hostAt(projects[0], "name") != "harbor" ||
			len(providers) != 1 || hostAt(providers[0], "id") != provider.String() || hostAt(providers[0], "name") != "Fixture" {
			t.Fatalf("the sample's inventory is %#v, %v", inventory, err)
		}
		if models, _ := hostAt(providers[0], "models").([]any); len(models) != 1 || models[0] != "fixture-model" {
			t.Fatalf("the sample's models are %#v", hostAt(providers[0], "models"))
		}

		// A watch observes the sample once; a refresh reads it anew.
		if _, err := call(owner, "watchUsage", query, "false"); err != nil {
			t.Fatal(err)
		}
		event, err := call(owner, "nextUsageChange", `""`)
		if err != nil || hostAt(event, "cursor") != "0" || hostAt(event, "all") != false || hostAt(event, "view", "connected") != true ||
			hostAt(event, "view", "refreshing") != false || hostAt(event, "view", "error") != nil ||
			hostAt(event, "view", "report", "totals", "responses") != "7" || hostAt(event, "view", "report", "node") != node {
			t.Fatalf("the watch observes %#v, %v", event, err)
		}
		if _, err := call(owner, "refreshUsage"); err != nil {
			t.Fatal(err)
		}
		if event, err := call(owner, "nextUsageChange", `"0"`); err != nil || hostAt(event, "cursor") != "1" || hostAt(event, "view", "report", "totals", "responses") != "7" {
			t.Fatalf("the refreshed watch observes %#v, %v", event, err)
		}

		// Another sample: the call that waits goes on with it.
		waiting, listing := later(owner, "nextUsageChange", `"1"`), later(owner, "nextUsageSources", string(before))
		owner.SetUsageSample(sample(9))
		if got := wait(waiting); got.err != nil || hostAt(got.value, "cursor") != "2" || hostAt(got.value, "view", "report", "totals", "responses") != "9" {
			t.Fatalf("the watch moved to %#v, %v", got.value, got.err)
		}

		// No sample: the sources are the Nodes', which this fixture has none
		// of, and the calls ask for the Node's Client.
		waiting = later(owner, "nextUsageChange", `"2"`)
		owner.SetUsageSample(nil)
		if got := wait(listing); got.err != nil || len(got.value.([]any)) != 0 {
			t.Fatalf("the sources without a sample are %#v, %v", got.value, got.err)
		}
		if got := wait(waiting); !unavailable(got.err) {
			t.Fatalf("the watch without a sample or a Client observes %#v, %v", got.value, got.err)
		}
		if _, err := call(owner, "readUsageChoices", query); !unavailable(err) {
			t.Fatalf("the report without a sample or a Client is %v", err)
		}
		if _, err := call(owner, "readUsageInventory"); !unavailable(err) {
			t.Fatalf("the inventory without a sample or a Client is %v", err)
		}

		// The sample again: the page's watch is still the one to move.
		owner.SetUsageSample(sample(7))
		for {
			event, err = call(owner, "nextUsageChange", `"2"`)
			if err == nil || ctx.Err() != nil {
				break
			}
			runtime.Gosched()
		}
		if err != nil || hostAt(event, "cursor") != "4" || hostAt(event, "view", "report", "totals", "responses") != "7" {
			t.Fatalf("the watch moved back to %#v, %v", event, err)
		}
	})

	// With a Client the sample keeps every call from the Node, and without
	// the sample the same call is the Node's to answer.
	t.Run("node", func(t *testing.T) {
		transport := &hostTestTransport{target: protocol.NodeID{1}}
		controller := client.New(transport)
		defer controller.Close()
		owner := NewHost(HostBinding{Context: ctx, Client: controller, Scope: scope, UsageSample: sample(7)})
		defer owner.Close()
		asked := func(kind string) int {
			count := 0
			for _, request := range transport.snapshot() {
				if request.Command.Kind == kind {
					count++
				}
			}
			return count
		}
		if report, err := call(owner, "readUsageChoices", query); err != nil || hostAt(report, "totals", "responses") != "7" {
			t.Fatalf("the sample's report is %#v, %v", report, err)
		}
		if _, err := call(owner, "readUsageInventory"); err != nil {
			t.Fatal(err)
		}
		if _, err := call(owner, "watchUsage", query, "false"); err != nil {
			t.Fatal(err)
		}
		if _, err := call(owner, "refreshUsage"); err != nil {
			t.Fatal(err)
		}
		if asked("read_usage") != 0 || asked("snapshot") != 0 {
			t.Fatalf("the sample asked the Node: %#v", transport.snapshot())
		}
		// The fixture answers every command with a stored value, which is no
		// usage report: the call fails, having asked.
		fresh := NewHost(HostBinding{Context: ctx, Client: controller, Scope: scope, UsageSample: sample(7)})
		defer fresh.Close()
		fresh.SetUsageSample(nil)
		if report, err := call(fresh, "readUsageChoices", query); err == nil {
			t.Fatalf("the fixture's Node answered a usage report: %#v", report)
		}
		if asked("read_usage") == 0 {
			t.Fatalf("the call without a sample did not ask the Node: %#v", transport.snapshot())
		}
	})
}
