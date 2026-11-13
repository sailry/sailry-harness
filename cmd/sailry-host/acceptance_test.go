package main

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"io"
	"os"
	"path/filepath"
	"runtime"
	"slices"
	"strings"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/acceptance"
	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/hostctl"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestParseAcceptanceRequiresExplicitAbsoluteProfile(t *testing.T) {
	for _, args := range [][]string{nil, {"--data-dir", ""}, {"--data-dir", "."}, {"--data-dir", "relative/profile"}, {"--start"}} {
		if _, help, err := parseAcceptance(args); err == nil || help || !strings.Contains(err.Error(), "absolute test profile") {
			t.Fatalf("parse %v = help %v, error %v", args, help, err)
		}
	}
	profile := filepath.Join(t.TempDir(), "not-opened")
	options, help, err := parseAcceptance([]string{"--data-dir", profile})
	if err != nil || help || options.profile != profile || options.suite != "quick" || options.start || options.effort != nil || options.timeout != 2*time.Minute || options.reportDirectory != filepath.Join(profile, "acceptance") {
		t.Fatalf("default options = %#v, help %v, error %v", options, help, err)
	}
	if _, err := os.Stat(profile); !errors.Is(err, os.ErrNotExist) {
		t.Fatalf("parse opened its profile: %v", err)
	}
	if _, help, err := parseAcceptance([]string{"--help"}); err != nil || !help {
		t.Fatalf("help requires a profile: help %v, error %v", help, err)
	}
	if options, help, err := parseAcceptance([]string{"--list"}); err != nil || help || !options.list || options.profile != "" || options.reportDirectory != "" {
		t.Fatalf("offline list options = %#v, help %v, error %v", options, help, err)
	}
}

func TestParseAcceptanceValidatesSelectionAndTimeout(t *testing.T) {
	profile := t.TempDir()
	provider := (protocol.ProviderID{1}).String()
	for _, test := range []struct {
		name string
		args []string
		want string
	}{
		{"suite", []string{"--suite", "automatic"}, "--suite"},
		{"provider_only", []string{"--provider", provider}, "both --provider and --model"},
		{"model_only", []string{"--model", "fixture"}, "both --provider and --model"},
		{"effort_only", []string{"--effort", "medium"}, "--effort requires both --provider and --model"},
		{"invalid_effort", []string{"--provider", provider, "--model", "fixture", "--effort", "automatic"}, "protocol effort level"},
		{"empty_effort", []string{"--provider", provider, "--model", "fixture", "--effort="}, "protocol effort level"},
		{"budget_without_value", []string{"--provider", provider, "--model", "fixture", "--effort", "budget"}, "protocol effort level"},
		{"numeric_effort", []string{"--provider", provider, "--model", "fixture", "--effort", "2000"}, "protocol effort level"},
		{"nil_provider", []string{"--provider", "00000000-0000-0000-0000-000000000000", "--model", "fixture"}, "nonzero provider ID"},
		{"zero_timeout", []string{"--timeout", "0"}, "positive duration"},
		{"negative_timeout", []string{"--timeout", "-1s"}, "positive duration"},
		{"invalid_timeout", []string{"--timeout", "soon"}, "invalid value"},
		{"empty_case", []string{"--case", " "}, "case ID"},
		{"positional", []string{"unexpected"}, "unexpected acceptance argument"},
	} {
		t.Run(test.name, func(t *testing.T) {
			args := append([]string{"--data-dir", profile}, test.args...)
			if _, help, err := parseAcceptance(args); err == nil || help || !strings.Contains(err.Error(), test.want) {
				t.Fatalf("parse %v = help %v, error %v", args, help, err)
			}
		})
	}
	options, help, err := parseAcceptance([]string{"--data-dir", profile, "--start", "--suite", "backend", "--provider", provider, "--model", "gpt-6-luna", "--effort", "medium", "--timeout", "30s", "--case", "A01", "--case", "P02"})
	if err != nil || help || !options.start || options.suite != "backend" || options.providerID != (protocol.ProviderID{1}) || options.model != "gpt-6-luna" || options.effort == nil || options.effort.Kind != "medium" || options.timeout != 30*time.Second || !slices.Equal(options.cases, []string{"A01", "P02"}) {
		t.Fatalf("explicit options = %#v, help %v, error %v", options, help, err)
	}
	for _, level := range []string{"default", "none", "minimal", "low", "medium", "high", "xhigh", "max"} {
		t.Run("effort_"+level, func(t *testing.T) {
			options, help, err := parseAcceptance([]string{"--data-dir", profile, "--provider", provider, "--model", "fixture", "--effort", level})
			if err != nil || help || options.effort == nil || options.effort.Kind != level || options.effort.Data != nil {
				t.Fatalf("effort %q = %#v, help %v, error %v", level, options.effort, help, err)
			}
		})
	}
	for _, suite := range []string{"ui", "e2e", "full", "replay"} {
		options, _, err := parseAcceptance([]string{"--data-dir", profile, "--suite", suite})
		if err != nil || options.suite != suite {
			t.Fatalf("suite %s: %v", suite, err)
		}
	}
}

func TestAcceptanceChannelSelection(t *testing.T) {
	p := protocol.DefaultConversationProvider()
	p.ID, p.Name, p.Enabled, p.DefaultModel = protocol.ProviderID{1}, "Test channel", true, "fixture"
	snapshot := protocol.DefaultSnapshot()
	snapshot.Providers = []protocol.ConversationProvider{p}
	for _, selector := range []string{p.ID.String(), p.Name} {
		config, err := acceptanceConfig(snapshot, acceptanceOptions{provider: selector, model: "fixture", effort: &protocol.Effort{Kind: "medium"}})
		if err != nil || config.Provider != p.ID || config.Model != "fixture" || config.Effort.Kind != "medium" {
			t.Fatalf("selection %s: %+v, %v", selector, config, err)
		}
	}
	other := p
	other.ID = protocol.ProviderID{2}
	snapshot.Providers = append(snapshot.Providers, other)
	if _, err := acceptanceConfig(snapshot, acceptanceOptions{provider: p.Name, model: "fixture"}); err == nil {
		t.Fatal("an ambiguous channel name selected a provider")
	}
	if _, err := acceptanceConfig(snapshot, acceptanceOptions{provider: "missing", model: "fixture"}); err == nil {
		t.Fatal("an unknown channel selected a provider")
	}
}

func TestAcceptanceListNeverOpensProfile(t *testing.T) {
	directory := t.TempDir()
	profile := filepath.Join(directory, "profile-is-a-file")
	reportDirectory := filepath.Join(directory, "not-created")
	const sentinel = "LIST-MUST-NOT-OPEN-THIS-PROFILE"
	if err := os.WriteFile(profile, []byte(sentinel), 0o600); err != nil {
		t.Fatal(err)
	}
	var output bytes.Buffer
	code, err := runAcceptance(t.Context(), []string{"--list", "--start", "--data-dir", profile, "--report-dir", reportDirectory}, &output)
	if err != nil || code != 0 {
		t.Fatalf("offline list = exit %d, %v", code, err)
	}
	listed := decodeAcceptanceEvents(t, output.Bytes())
	expected, err := acceptance.ListBackendCases("quick")
	if err != nil || len(listed) != len(expected) {
		t.Fatalf("list has %d events, expected %d: %v", len(listed), len(expected), err)
	}
	for index, event := range listed {
		if event.Type != "case" || event.Case == nil || event.Case.ID != expected[index].ID || event.Case.Group != "backend" || event.Case.Status != "" || event.Report != nil || event.Files != nil {
			t.Fatalf("offline list performed or reported work: %#v", event)
		}
	}
	content, err := os.ReadFile(profile)
	if err != nil || string(content) != sentinel {
		t.Fatalf("offline list modified the profile: %q, %v", content, err)
	}
	if _, err := os.Stat(reportDirectory); !errors.Is(err, os.ErrNotExist) {
		t.Fatalf("offline list created a report directory: %v", err)
	}
	output.Reset()
	if code, err := runAcceptance(t.Context(), []string{"--list"}, &output); err != nil || code != 0 || output.Len() == 0 {
		t.Fatalf("offline list without any profile = exit %d, %v", code, err)
	}
}

func TestAcceptanceListSelectsCasesOutsideQuick(t *testing.T) {
	quick, err := acceptance.ListBackendCases("quick")
	if err != nil {
		t.Fatal(err)
	}
	full, err := acceptance.ListBackendCases("backend")
	if err != nil {
		t.Fatal(err)
	}
	index := slices.IndexFunc(full, func(value acceptance.CaseResult) bool {
		return !slices.ContainsFunc(quick, func(quick acceptance.CaseResult) bool { return quick.ID == value.ID })
	})
	if index < 0 {
		t.Fatal("backend suite has no extended case")
	}
	var output bytes.Buffer
	code, err := runAcceptance(t.Context(), []string{"--list", "--suite", "quick", "--case", full[index].ID}, &output)
	if err != nil || code != 0 {
		t.Fatalf("explicit extended selection = exit %d, %v", code, err)
	}
	events := decodeAcceptanceEvents(t, output.Bytes())
	if len(events) != 1 || events[0].Case == nil || events[0].Case.ID != full[index].ID {
		t.Fatalf("quick filtered an explicitly selected extended case: %#v", events)
	}
	output.Reset()
	if code, err := runAcceptance(t.Context(), []string{"--list", "--case", "unknown-case"}, &output); err == nil || code != 1 || output.Len() != 0 {
		t.Fatalf("unknown case = exit %d, output %q, error %v", code, output.String(), err)
	}
}

func TestAcceptanceMissingHostProducesBlockedReport(t *testing.T) {
	profile := filepath.Join(t.TempDir(), "no-host")
	directory := filepath.Join(profile, "acceptance")
	previous, err := acceptance.WriteReport(directory, acceptance.Report{Version: 1, Suite: "backend", Node: protocol.NodeID{1}, Cases: []acceptance.CaseResult{{ID: "A01", Group: "backend", Session: (protocol.SessionID{1}).String(), Status: "failed"}}})
	if err != nil {
		t.Fatal(err)
	}
	original := map[string][]byte{}
	for _, path := range []string{previous.JSON, previous.HTML} {
		original[path], err = os.ReadFile(path)
		if err != nil {
			t.Fatal(err)
		}
	}
	var output bytes.Buffer
	provider := (protocol.ProviderID{1}).String()
	code, err := runAcceptance(t.Context(), []string{"--data-dir", profile, "--provider", provider, "--model", "gpt-6-luna", "--effort", "medium", "--case", "A01"}, &output)
	if err != nil || code != 1 {
		t.Fatalf("missing Host = exit %d, %v", code, err)
	}
	events := decodeAcceptanceEvents(t, output.Bytes())
	if len(events) != 3 || events[0].Type != "started" || events[1].Type != "case" || events[1].Case == nil || events[1].Case.ID != "runner" || events[1].Case.Status != "blocked" || events[2].Type != "report" || events[2].Report == nil || events[2].Files == nil {
		t.Fatalf("missing Host events = %#v", events)
	}
	final := events[2]
	if final.Files.JSON != filepath.Join(directory, "setup", "latest.json") {
		t.Fatalf("setup failure did not get a separate diagnostic report: %#v", final.Files)
	}
	if final.Report.OK() || len(final.Report.Cases) != 1 || final.Report.Cases[0].Status != "blocked" || final.Report.Provider != provider || final.Report.Model != "gpt-6-luna" || final.Report.Effort == nil || final.Report.Effort.Kind != "medium" || final.Report.Started.IsZero() || final.Report.Finished.Before(final.Report.Started) || !strings.Contains(final.Error, "running Host") {
		t.Fatalf("missing Host was reported as complete: %#v", final)
	}
	data, err := os.ReadFile(final.Files.JSON)
	if err != nil {
		t.Fatal(err)
	}
	var stored acceptance.Report
	if err := json.Unmarshal(data, &stored); err != nil || stored.OK() || len(stored.Cases) != 1 || stored.Cases[0].Status != "blocked" || stored.Effort == nil || stored.Effort.Kind != "medium" {
		t.Fatalf("persisted setup report = %#v, %v", stored, err)
	}
	for path, before := range original {
		after, err := os.ReadFile(path)
		if err != nil || !bytes.Equal(before, after) {
			t.Fatalf("failed Host connection replaced previous ownership evidence at %s: %v", path, err)
		}
	}
	if _, err := os.Stat(filepath.Join(directory, "ownership.json")); !errors.Is(err, os.ErrNotExist) {
		t.Fatalf("a failed connection created an ownership ledger: %v", err)
	}
	entries, err := os.ReadDir(profile)
	if err != nil || len(entries) != 1 || entries[0].Name() != "acceptance" || !entries[0].IsDir() {
		t.Fatalf("attach opened a Node profile: %#v, %v", entries, err)
	}
}

func TestAcceptanceImportFailureKeepsEvidence(t *testing.T) {
	if runtime.GOOS == "windows" {
		t.Skip("Host control requires a Unix socket")
	}
	if runtime.GOOS == "darwin" {
		// Keep the test-owned socket below Darwin's Unix socket path limit.
		t.Setenv("TMPDIR", "/tmp")
	}
	for _, variant := range []string{"other_node", "invalid_ledger"} {
		t.Run(variant, func(t *testing.T) {
			profile := filepath.Join(t.TempDir(), "profile")
			if err := core.PrivateDirectory(profile); err != nil {
				t.Fatal(err)
			}
			directory := filepath.Join(profile, "acceptance")
			owner := protocol.NodeID{1}
			if variant == "other_node" {
				owner = protocol.NodeID{2}
			}
			previous, err := acceptance.WriteReport(directory, acceptance.Report{Version: 1, Suite: "backend", Node: owner, Cases: []acceptance.CaseResult{{ID: "A01", Group: "backend", Session: (protocol.SessionID{1}).String(), Status: "failed"}}})
			if err != nil {
				t.Fatal(err)
			}
			if err := acceptance.Own(directory, owner, protocol.SessionID{1}); err != nil {
				t.Fatal(err)
			}
			ledgerPath := filepath.Join(directory, "ownership.json")
			if variant == "invalid_ledger" {
				if err := os.WriteFile(ledgerPath, []byte("{incomplete ownership evidence"), 0o600); err != nil {
					t.Fatal(err)
				}
			}
			original := map[string][]byte{}
			for _, path := range []string{previous.JSON, previous.HTML, ledgerPath} {
				original[path], err = os.ReadFile(path)
				if err != nil {
					t.Fatal(err)
				}
			}
			transport := &acceptanceSetupTransport{acceptanceCleanupTransport{dispatch: func(context.Context, protocol.Request) (protocol.Output, error) {
				t.Error("a failed ownership import dispatched a Node command")
				return protocol.Output{}, errors.New("ownership import must stop execution")
			}}}
			server, err := hostctl.Listen(profile, nil, transport)
			if err != nil {
				t.Fatal(err)
			}
			ctx, cancel := context.WithTimeout(t.Context(), 5*time.Second)
			done := make(chan error, 1)
			go func() { done <- server.Serve(ctx) }()
			t.Cleanup(func() {
				cancel()
				_ = server.Close()
				if err := <-done; err != nil {
					t.Error(err)
				}
			})
			var output bytes.Buffer
			code, err := runAcceptance(ctx, []string{"--data-dir", profile, "--case", "A01"}, &output)
			if err != nil || code != 1 {
				t.Fatalf("ownership import failure = exit %d, %v", code, err)
			}
			events := decodeAcceptanceEvents(t, output.Bytes())
			final := events[len(events)-1]
			if final.Report == nil || final.Report.OK() || final.Files == nil || final.Files.JSON != filepath.Join(directory, "setup", "latest.json") || !strings.Contains(final.Error, "recorded acceptance ownership") {
				t.Fatalf("ownership import failure lost its diagnostic report: %#v", final)
			}
			if _, err := os.ReadFile(final.Files.HTML); err != nil {
				t.Fatalf("setup diagnostic HTML is unavailable: %v", err)
			}
			for path, before := range original {
				after, err := os.ReadFile(path)
				if err != nil || !bytes.Equal(before, after) {
					t.Fatalf("ownership import failure changed %s: %v", path, err)
				}
			}
		})
	}
}

type acceptanceSetupTransport struct {
	acceptanceCleanupTransport
}

func (*acceptanceSetupTransport) Subscribe(_ context.Context, topic protocol.Topic) (link.Subscription, error) {
	if topic.Kind != "node" {
		return nil, errors.New("fixture only serves the Node snapshot")
	}
	snapshot := protocol.DefaultSnapshot()
	snapshot.Node = protocol.NodeID{1}
	return &acceptanceSetupSubscription{initial: &snapshot}, nil
}

type acceptanceSetupSubscription struct {
	initial *protocol.Snapshot
}

func (subscription *acceptanceSetupSubscription) Next(ctx context.Context) (protocol.Update, error) {
	if subscription.initial != nil {
		snapshot := *subscription.initial
		subscription.initial = nil
		return protocol.Update{Kind: "snapshot", Data: snapshot}, nil
	}
	<-ctx.Done()
	return protocol.Update{}, ctx.Err()
}

func (*acceptanceSetupSubscription) Close() error { return nil }

func TestAcceptanceConcurrentRunKeepsPublishedReport(t *testing.T) {
	profile := t.TempDir()
	directory := filepath.Join(profile, "acceptance")
	paths, err := acceptance.WriteReport(directory, acceptance.Report{Version: 1, Suite: "backend", Model: "previous-model", Cases: []acceptance.CaseResult{{ID: "previous", Status: "passed"}}})
	if err != nil {
		t.Fatal(err)
	}
	before, err := os.ReadFile(paths.JSON)
	if err != nil {
		t.Fatal(err)
	}
	owner, err := core.AcquireProfile(filepath.Join(directory, "runner"))
	if err != nil {
		t.Fatal(err)
	}
	defer owner.Close()
	var output bytes.Buffer
	code, err := runAcceptance(t.Context(), []string{"--data-dir", profile}, &output)
	if code != 1 || err == nil || output.Len() != 0 {
		t.Fatalf("overlapping run: code %d, output %q, error %v", code, output.String(), err)
	}
	after, err := os.ReadFile(paths.JSON)
	if err != nil || !bytes.Equal(before, after) {
		t.Fatal("a rejected run replaced the previous report")
	}
}

func TestAcceptanceRejectsCasesOutsideSelectedSuite(t *testing.T) {
	profile := filepath.Join(t.TempDir(), "unopened")
	for _, args := range [][]string{
		{"--suite", "backend", "--case", "A01", "--case", "E2E01"},
		{"--suite", "ui", "--case", "ui.send", "--case", "A01"},
		{"--suite", "e2e", "--case", "ui.send"},
		{"--suite", "quick", "--case", "E2E01"},
	} {
		var output bytes.Buffer
		code, err := runAcceptance(t.Context(), append([]string{"--data-dir", profile}, args...), &output)
		if code != 1 || err == nil || output.Len() != 0 {
			t.Fatalf("out-of-scope cases %v: code %d, output %q, error %v", args, code, output.String(), err)
		}
		if _, err := os.Stat(profile); !errors.Is(err, os.ErrNotExist) {
			t.Fatalf("invalid case selection opened a profile: %v", err)
		}
	}
}

func decodeAcceptanceEvents(t *testing.T, data []byte) []acceptanceEvent {
	t.Helper()
	decoder := json.NewDecoder(bytes.NewReader(data))
	events := []acceptanceEvent{}
	for {
		var event acceptanceEvent
		if err := decoder.Decode(&event); errors.Is(err, io.EOF) {
			return events
		} else if err != nil {
			t.Fatalf("invalid acceptance JSON event: %v", err)
		}
		events = append(events, event)
	}
}
