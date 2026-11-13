package acceptance

import (
	"context"
	"encoding/json"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func toolFixtureView(name string) turnView {
	callID := "fixture-call"
	summary := protocol.DefaultToolDetailSummary()
	summary.Revision = strings.Repeat("a", 64)
	call := protocol.DefaultConversationPartToolCallPayload()
	call.ID, call.Name, call.Summary = &callID, name, &summary
	entry := protocol.DefaultConversationEntry()
	entry.ID, entry.Turn, entry.Branch = "fixture-request", protocol.TurnID{1}, "fixture-branch"
	entry.Parts = []protocol.ConversationPart{{Kind: "tool_call", Data: call}}
	page := protocol.DefaultConversationPage()
	page.Session, page.Entries = protocol.SessionID{1}, []protocol.ConversationEntry{entry}
	return turnView{page: page, run: protocol.ConversationRun{Turn: entry.Turn}}
}

func TestCanonicalToolsPairOnlyTheirOwnResult(t *testing.T) {
	for _, test := range []string{"matched", "branch", "turn", "name", "duplicate"} {
		t.Run(test, func(t *testing.T) {
			name := PackageToolAlias("files", "read_file")
			view := toolFixtureView(name)
			callID := "fixture-call"
			summary := protocol.DefaultToolDetailSummary()
			summary.Revision = strings.Repeat("b", 64)
			result := protocol.DefaultConversationPartToolResultPayload()
			result.ID, result.Name, result.Summary = &callID, name, &summary
			entry := protocol.DefaultConversationEntry()
			entry.ID, entry.Turn, entry.Branch = "fixture-response", view.run.Turn, "fixture-branch"
			switch test {
			case "branch":
				entry.Branch = "other-branch"
			case "turn":
				entry.Turn = protocol.TurnID{2}
			case "name":
				result.Name = PackageToolAlias("files", "write_file")
			case "duplicate":
				duplicate := view.page.Entries[0]
				duplicate.ID = "ambiguous-request"
				view.page.Entries = append(view.page.Entries, duplicate)
			}
			entry.Parts = []protocol.ConversationPart{{Kind: "tool_result", Data: result}}
			view.page.Entries = append(view.page.Entries, entry)
			calls := view.tools()
			if len(calls) == 0 {
				t.Fatal("canonical request was lost")
			}
			if (calls[0].reference.Entry == entry.ID) != (test == "matched") {
				t.Fatal("result pairing crossed a turn, branch, call name or ambiguous identity")
			}
			if test == "matched" && calls[0].reference.Revision != summary.Revision {
				t.Fatal("the result did not retain its own detail revision")
			}
		})
	}
}

func TestPackageToolsUseCapturedDeclarations(t *testing.T) {
	for _, test := range []struct {
		name, declared, owner string
		mismatch              bool
		want                  string
	}{
		{"declared", "read_file", "files", false, "read_file"},
		{"undeclared", "write_file", "files", false, ""},
		{"other_owner", "read_file", "other-package", false, ""},
		{"different_capture", "read_file", "files", true, ""},
	} {
		t.Run(test.name, func(t *testing.T) {
			alias := PackageToolAlias(test.owner, "read_file")
			view := toolFixtureView(alias)
			capture := protocol.DefaultQueuedTurn()
			capture.ID, capture.Session, capture.Request, capture.Kind = view.run.Turn, view.page.Session, protocol.RequestID{1}, protocol.ConversationRunKindTask
			capture.Config.Provider, capture.Config.Model = protocol.ProviderID{1}, "fixture"
			capture.Config.Mode, capture.Config.Permission = protocol.WorkModeCode, protocol.PermissionProject
			reference := protocol.PluginReference{Name: test.owner, Digest: strings.Repeat("a", 64)}
			capture.Plugins = []protocol.PluginReference{reference}
			info := protocol.DefaultPluginInfo()
			info.Summary.Name, info.Summary.Digest = reference.Name, reference.Digest
			if test.mismatch {
				info.Summary.Digest = strings.Repeat("b", 64)
			}
			extension := protocol.DefaultPluginExtension()
			declaration := protocol.DefaultToolDeclaration()
			declaration.Name = test.declared
			extension.Tools = []protocol.ToolDeclaration{declaration}
			info.Extension = &extension
			transport := &fixtureTransport{dispatch: func(_ context.Context, request protocol.Request) (link.Admission, error) {
				switch request.Command.Kind {
				case "read_turn_capture":
					return fixtureResponse(request, protocol.Output{Kind: "queued_turn", Data: capture}), nil
				case "read_plugin_version":
					input, ok := request.Command.Data.(*protocol.CommandReadPluginVersionPayload)
					if !ok || input.Package != reference {
						t.Fatal("tool declaration was read from another package version")
					}
					return fixtureResponse(request, protocol.Output{Kind: "plugin", Data: info}), nil
				default:
					t.Fatalf("unexpected tool mapping request %q", request.Command.Kind)
					return link.Admission{}, nil
				}
			}}
			controller := client.New(transport)
			t.Cleanup(func() { _ = controller.Close() })
			run := &caseRun{ctx: t.Context(), controller: controller, session: protocol.Session{ID: view.page.Session}, result: &CaseResult{}}
			resolved, err := run.resolveToolNames(view)
			if (err != nil) != test.mismatch {
				t.Fatalf("mapping result = %v, mismatch %v", err, test.mismatch)
			}
			if err != nil {
				return
			}
			want := test.want
			if want == "" {
				want = alias
			}
			if calls := resolved.tools(); len(calls) != 1 || calls[0].name != want {
				t.Fatalf("tool declaration mapping = %#v, want %q", calls, want)
			}
		})
	}
}

func TestCanonicalDetailDoesNotRequireExpandableUI(t *testing.T) {
	for _, completed := range []bool{false, true} {
		t.Run(map[bool]string{false: "pending", true: "summarized_result"}[completed], func(t *testing.T) {
			call := toolFixtureView("ask_user").tools()[0]
			detail := protocol.DefaultToolDetail()
			detail.Arguments = json.RawMessage(`{"prompt":"fixture"}`)
			if completed {
				result := json.RawMessage(`{"kind":"fixture_result"}`)
				detail.Result = &result
			}
			encoded, err := json.Marshal(detail)
			if err != nil {
				t.Fatal(err)
			}
			transport := &fixtureTransport{dispatch: func(_ context.Context, request protocol.Request) (link.Admission, error) {
				input, ok := request.Command.Data.(*protocol.CommandReadToolDetailPayload)
				if request.Command.Kind != "read_tool_detail" || !ok || input.Session != call.reference.Session || input.Turn != call.reference.Turn || input.Entry != call.reference.Entry || input.Index != call.reference.Index || input.ExpectedRevision != call.reference.Revision {
					t.Fatal("canonical detail read changed its captured reference")
				}
				page := protocol.ToolDetailPage{Reference: call.reference, TotalBytes: uint64(len(encoded)), Text: string(encoded)}
				return fixtureResponse(request, protocol.Output{Kind: "tool_detail", Data: page}), nil
			}}
			controller := client.New(transport)
			t.Cleanup(func() { _ = controller.Close() })
			run := &caseRun{ctx: t.Context(), controller: controller, result: &CaseResult{}}
			actual, err := run.detail(call)
			if err != nil || argumentString(actual, "prompt") != "fixture" || (actual.Result != nil) != completed || call.summary.HasDetails {
				t.Fatalf("canonical detail was confused with UI expandability: %v", err)
			}
		})
	}
}

func TestCommandCompletionUsesRawOutcomeAndOptionalProjection(t *testing.T) {
	for _, test := range []struct {
		name                     string
		expected, raw, projected int32
		failure                  string
		projection, valid        bool
	}{
		{"success", 0, 0, 0, "", false, true},
		{"exit7_without_content", 7, 7, 0, "tool", false, true},
		{"exit127_with_projection", 127, 127, 127, "tool", true, true},
		{"incorrect_raw", 7, 127, 7, "tool", true, false},
		{"incorrect_projection", 7, 7, 127, "tool", true, false},
		{"missing_failure", 7, 7, 0, "", false, false},
		{"false_success_failure", 0, 0, 0, "tool", false, false},
		{"runtime_failure", 7, 7, 0, "runtime", false, false},
	} {
		t.Run(test.name, func(t *testing.T) {
			summary := protocol.DefaultToolDetailSummary()
			if test.failure != "" {
				summary.Failure = &protocol.ToolFailure{Origin: protocol.ToolFailureOrigin(test.failure)}
			}
			completion := protocol.DefaultProcessCompletion()
			completion.Outcome = protocol.ProcessOutcome{Kind: "exited", Data: test.raw}
			completion.Stdout.Text = "SAILRY-RESULT\n"
			encoded, err := json.Marshal(protocol.Output{Kind: "command_result", Data: completion})
			if err != nil {
				t.Fatal(err)
			}
			detail := protocol.DefaultToolDetail()
			detail.Arguments = json.RawMessage(`{"command":"exit fixture","access":"workspace"}`)
			result := json.RawMessage(encoded)
			detail.Result = &result
			if test.projection {
				resolved := protocol.DefaultToolDetailResolved()
				resolved.Command = &protocol.ProcessOutcome{Kind: "exited", Data: test.projected}
				detail.Resolved = &resolved
			}
			run := &caseRun{result: &CaseResult{}}
			err = run.assertCommandCompletion(toolCall{summary: &summary}, detail, "exit fixture", "SAILRY-RESULT", test.expected)
			if (err == nil) != test.valid {
				t.Fatalf("canonical completion validity = %v, want %v", err, test.valid)
			}
			if test.failure == "runtime" {
				assertBlocked(t, err)
			}
		})
	}
}
