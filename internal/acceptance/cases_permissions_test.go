package acceptance

import (
	"context"
	"encoding/json"
	"errors"
	"testing"

	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestSandboxPrerequisitesDoNotClaimUnavailableIsolation(t *testing.T) {
	for _, os := range []string{"", "windows", "unknown-platform", "darwin", "ubuntu"} {
		t.Run(os, func(t *testing.T) {
			controller := client.New(&fixtureTransport{dispatch: func(_ context.Context, request protocol.Request) (link.Admission, error) {
				switch request.Command.Kind {
				case "inspect_host":
					host := protocol.DefaultHostInfo()
					if os != "" {
						host.Os = &os
					}
					return fixtureResponse(request, protocol.Output{Kind: "host_info", Data: host}), nil
				case "read_sandbox_settings":
					settings := protocol.DefaultSandboxSettings()
					settings.Enabled = false
					return fixtureResponse(request, protocol.Output{Kind: "sandbox_settings", Data: settings}), nil
				default:
					t.Fatalf("unavailable sandbox case attempted a mutation: %s", request.Command.Kind)
					return link.Admission{}, nil
				}
			}})
			t.Cleanup(func() { _ = controller.Close() })
			run := &caseRun{ctx: t.Context(), controller: controller, directory: testReportDirectory(t), result: &CaseResult{}}
			err := run.sandboxSession(protocol.PermissionAsk)
			var issue *caseIssue
			want := "skipped"
			if os == "darwin" || os == "ubuntu" {
				want = "blocked"
			}
			if !errors.As(err, &issue) || issue.status != want {
				t.Fatalf("sandbox prerequisite status = %v, want %s", err, want)
			}
		})
	}
}

func TestSandboxApprovalLimitedToExactOwnedCommand(t *testing.T) {
	for _, changed := range []string{"", "command", "tool", "session", "turn", "entry", "index", "access", "remember", "prefix", "choice", "background", "pty", "cwd", "attachments", "unknown_argument"} {
		t.Run(changed, func(t *testing.T) {
			view := toolFixtureView("run_command")
			call := view.tools()[0]
			access := protocol.ProcessAccessUnrestricted
			approval := protocol.DefaultConversationApproval()
			approval.ID, approval.Session, approval.Turn = protocol.ApprovalID{1}, view.page.Session, view.run.Turn
			approval.Entry, approval.Index, approval.Access = call.reference.Entry, call.reference.Index, &access
			approval.State, approval.Source = protocol.ConversationApprovalStatePending, protocol.ConversationApprovalSourceUser
			args := map[string]any{"command": sandboxPrint, "access": "unrestricted", "background": false, "timeout_ms": 5000}
			switch changed {
			case "command":
				args["command"] = sandboxPrint + "; true"
			case "tool":
				call.name = "other_tool"
			case "session":
				approval.Session = protocol.SessionID{2}
			case "turn":
				approval.Turn = protocol.TurnID{2}
			case "entry":
				approval.Entry = "another-call"
			case "index":
				approval.Index++
			case "access":
				access = protocol.ProcessAccessWorkspace
			case "remember":
				approval.CanRemember = true
			case "prefix":
				prefix := "printf"
				approval.Prefix = &prefix
			case "choice":
				approval.Choice = &protocol.ConversationApprovalChoice{Kind: "session"}
			case "background", "pty":
				args[changed] = true
			case "cwd":
				args["cwd"] = "/"
			case "attachments":
				args["attachments"] = []string{"foreign"}
			case "unknown_argument":
				args["environment"] = map[string]string{"PATH": "/elsewhere"}
			}
			detail := protocol.DefaultToolDetail()
			detail.Arguments, _ = json.Marshal(args)
			if got := sandboxApprovalMatches(approval, view.page.Session, view.run.Turn, call, detail, sandboxPrint); got != (changed == "") {
				t.Fatalf("approval match = %v for changed %q", got, changed)
			}
		})
	}
}

func TestSandboxDefaultAccessRequiresOmittedArgument(t *testing.T) {
	for _, access := range []string{"", `,"access":null`, `,"access":"workspace"`, `,"access":"unrestricted"`} {
		detail := protocol.DefaultToolDetail()
		detail.Arguments = json.RawMessage(`{"command":"fixture","timeout_ms":5000` + access + `}`)
		if sandboxCommandMatches(detail, "fixture", "") != (access == "") {
			t.Fatalf("default-access fixture accepted supplied access %q", access)
		}
	}
}

func TestSandboxEvidenceRequiresExecutedScope(t *testing.T) {
	for _, changed := range []string{"", "process_access", "process_session", "process_turn", "approval_access", "approval_source", "pending", "stdout", "raw_exit", "capture_permission", "denied", "outside_succeeded", "native_unavailable"} {
		t.Run(changed, func(t *testing.T) {
			view := toolFixtureView("run_command")
			call := view.tools()[0]
			completion := protocol.DefaultProcessCompletion()
			completion.Outcome, completion.Stdout.Text = protocol.ProcessOutcome{Kind: "exited", Data: int32(0)}, sandboxMarker
			info := protocol.DefaultProcessInfo()
			info.ID, info.Session, info.Turn = protocol.RequestID{3}, view.page.Session, view.run.Turn
			info.Command, info.Access, info.Status = sandboxPrint, protocol.ProcessAccessUnrestricted, protocol.ProcessStatus{Kind: "finished", Data: completion.Outcome}
			capture := protocol.DefaultQueuedTurn()
			capture.Config.Mode, capture.Config.Permission = protocol.WorkModeCode, protocol.PermissionFull
			access, source, success := protocol.ProcessAccessUnrestricted, protocol.ConversationApprovalSourceFull, true
			approval := protocol.DefaultConversationApproval()
			approval.Session, approval.Turn, approval.Entry = view.page.Session, view.run.Turn, call.reference.Entry
			approval.State, approval.Source = protocol.ConversationApprovalStateApproved, source
			switch changed {
			case "process_access":
				info.Access = protocol.ProcessAccessWorkspace
			case "process_session":
				info.Session = protocol.SessionID{2}
			case "process_turn":
				info.Turn = protocol.TurnID{2}
			case "approval_source":
				approval.Source = protocol.ConversationApprovalSourceSandbox
			case "pending":
				approval.State = protocol.ConversationApprovalStatePending
			case "stdout":
				completion.Stdout.Text = "MODEL-SAYS-SUCCESS"
			case "raw_exit":
				completion.Outcome.Data = int32(1)
			case "capture_permission":
				capture.Config.Permission = protocol.PermissionAsk
			case "denied", "outside_succeeded", "native_unavailable":
				access, source, success = protocol.ProcessAccessWorkspace, protocol.ConversationApprovalSourceSandbox, false
				capture.Config.Permission, info.Access, approval.Source = protocol.PermissionAsk, access, source
				call.summary.Failure = &protocol.ToolFailure{Origin: protocol.ToolFailureOriginTool}
				completion.Outcome.Data, completion.Stderr.Text = int32(1), "permission denied"
				if changed == "outside_succeeded" {
					completion.Outcome.Data = int32(0)
				}
				if changed == "native_unavailable" {
					completion.Stdout.Text = ""
				}
			}
			approval.Access = &access
			info.Status.Data = completion.Outcome
			// tools decodes copies of the page. Put the completion summary in
			// its canonical result, not only in the earlier projected call.
			response := protocol.DefaultConversationPartToolResultPayload()
			response.ID, response.Name, response.Summary = call.reference.Call, call.name, call.summary
			entry := protocol.DefaultConversationEntry()
			entry.ID, entry.Turn, entry.Branch, entry.Author = "fixture-result", view.run.Turn, view.page.Entries[0].Branch, "tool"
			entry.Parts = []protocol.ConversationPart{{Kind: "tool_result", Data: response}}
			view.page.Entries = append(view.page.Entries, entry)
			call = view.tools()[0]
			if changed == "approval_access" {
				other := protocol.ProcessAccessWorkspace
				approval.Access = &other
			}
			view.page.Approvals = []protocol.ConversationApproval{approval}
			detail := protocol.DefaultToolDetail()
			detail.Arguments, _ = json.Marshal(map[string]any{"command": sandboxPrint, "access": "workspace", "timeout_ms": 5000})
			raw, err := json.Marshal(map[string]any{"kind": "command_result", "data": completion, "sailry_command": map[string]any{"id": info.ID}})
			if err != nil {
				t.Fatal(err)
			}
			result := json.RawMessage(raw)
			detail.Result = &result
			encoded, err := json.Marshal(detail)
			if err != nil {
				t.Fatal(err)
			}
			controller := client.New(&fixtureTransport{dispatch: func(_ context.Context, request protocol.Request) (link.Admission, error) {
				switch request.Command.Kind {
				case "read_tool_detail":
					return fixtureResponse(request, protocol.Output{Kind: "tool_detail", Data: protocol.ToolDetailPage{Reference: call.reference, TotalBytes: uint64(len(encoded)), Text: string(encoded)}}), nil
				case "read_command":
					input := request.Command.Data.(*protocol.CommandReadCommandPayload)
					if input.Session != view.page.Session || input.ID != info.ID {
						t.Error("process inspection escaped the test session or retained result ID")
					}
					return fixtureResponse(request, protocol.Output{Kind: "command_output", Data: protocol.ProcessSnapshot{Info: info, Stdout: completion.Stdout, Stderr: completion.Stderr}}), nil
				default:
					t.Fatalf("sandbox evidence sent an unexpected command: %s", request.Command.Kind)
					return link.Admission{}, nil
				}
			}})
			t.Cleanup(func() { _ = controller.Close() })
			run := &caseRun{ctx: t.Context(), controller: controller, directory: testReportDirectory(t), result: &CaseResult{}, session: protocol.Session{ID: view.page.Session}, captures: map[protocol.TurnID]protocol.QueuedTurn{view.run.Turn: capture}}
			err = run.assertSandboxCommand(view, sandboxPrint, "workspace", access, source, success)
			if (err == nil) != (changed == "" || changed == "denied") {
				t.Fatalf("executed sandbox evidence accepted %q: %v", changed, err)
			}
			if changed == "native_unavailable" {
				assertBlocked(t, err)
			}
		})
	}
}
