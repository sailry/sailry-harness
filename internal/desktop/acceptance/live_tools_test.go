package acceptance

import (
	"context"
	"encoding/json"
	"reflect"
	"strings"
	"testing"

	core "github.com/sailry/sailry-harness/internal/acceptance"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func liveCommandToolPage(name string, session protocol.SessionID, turn protocol.TurnID) protocol.ConversationPage {
	id := "fixture-call"
	call := protocol.DefaultConversationPartToolCallPayload()
	call.ID, call.Name, call.Presentation = &id, name, protocol.ToolPresentationContent
	callSummary := protocol.DefaultToolDetailSummary()
	callSummary.Revision = strings.Repeat("b", 64)
	call.Summary = &callSummary
	request := protocol.DefaultConversationEntry()
	request.ID, request.Turn, request.Branch = "fixture-request", turn, "fixture-branch"
	request.Parts = []protocol.ConversationPart{{Kind: "tool_call", Data: call}}
	summary := protocol.DefaultToolDetailSummary()
	summary.Revision, summary.HasDetails = strings.Repeat("a", 64), true
	result := protocol.DefaultConversationPartToolResultPayload()
	result.ID, result.Name, result.Summary = &id, name, &summary
	response := protocol.DefaultConversationEntry()
	response.ID, response.Turn, response.Branch = "fixture-response", turn, request.Branch
	response.Parts = []protocol.ConversationPart{{Kind: "tool_result", Data: result}}
	page := protocol.DefaultConversationPage()
	page.Session, page.Entries = session, []protocol.ConversationEntry{request, response}
	return page
}

func TestLiveToolReferenceUsesOnlyItsPairedResult(t *testing.T) {
	for _, name := range []string{"run_command", core.PackageToolAlias("commands", "run_command")} {
		t.Run(name, func(t *testing.T) {
			for _, fixture := range []string{"matched", "branch", "turn", "call", "name", "duplicate", "summary", "presentation"} {
				t.Run(fixture, func(t *testing.T) {
					id := "fixture-call"
					page := protocol.DefaultConversationPage()
					page.Session = protocol.SessionID{2}
					turn := protocol.TurnID{3}
					call := protocol.DefaultConversationPartToolCallPayload()
					call.ID, call.Name = &id, name
					call.Presentation = protocol.ToolPresentationContent
					callSummary := protocol.DefaultToolDetailSummary()
					callSummary.Revision = strings.Repeat("b", 64)
					call.Summary = &callSummary
					request := protocol.DefaultConversationEntry()
					request.ID, request.Turn, request.Branch = "fixture-request", turn, "fixture-branch"
					request.Parts = []protocol.ConversationPart{{Kind: "tool_call", Data: call}}
					summary := protocol.DefaultToolDetailSummary()
					summary.Revision, summary.HasDetails = strings.Repeat("a", 64), true
					result := protocol.DefaultConversationPartToolResultPayload()
					result.ID, result.Name, result.Summary = &id, name, &summary
					response := protocol.DefaultConversationEntry()
					response.ID, response.Turn, response.Branch = "fixture-response", turn, request.Branch
					page.Entries = []protocol.ConversationEntry{request}
					switch fixture {
					case "branch":
						response.Branch = "another-branch"
					case "turn":
						response.Turn = protocol.TurnID{4}
					case "call":
						other := "another-call"
						result.ID = &other
					case "name":
						result.Name = core.PackageToolAlias("another-package", "run_command")
					case "duplicate":
						page.Entries = append(page.Entries, request)
					case "summary":
						result.Summary = nil
					case "presentation":
						call.Presentation = protocol.ToolPresentationSummary
						page.Entries[0].Parts[0].Data = call
					}
					response.Parts = []protocol.ConversationPart{{Kind: "text", Data: "fixture"}, {Kind: "tool_result", Data: result}}
					page.Entries = append(page.Entries, response)
					reference, actual, err := liveToolReference(page, turn, name)
					if fixture != "matched" {
						if err == nil {
							t.Fatal("an unpaired or ambiguous tool result satisfied the UI fixture")
						}
						return
					}
					if err != nil || actual == nil || reference.Entry != response.ID || reference.Index != 1 || reference.Revision != summary.Revision || reference.Call == nil || *reference.Call != id || reference.Session != page.Session || reference.Turn != turn {
						t.Fatalf("result detail reference lost its response identity: %v, %#v", err, reference)
					}
				})
			}
		})
	}
}

func TestLiveCommandResultPreservesRawFailureWithoutDisplayProjection(t *testing.T) {
	for _, fixture := range []string{"original", "wrong_exit", "wrong_stdout", "unrestricted", "changed_command", "background"} {
		t.Run(fixture, func(t *testing.T) {
			arguments := map[string]any{"command": liveCommandFixture, "access": "workspace", "background": false, "timeout_ms": 5000}
			completion := protocol.DefaultProcessCompletion()
			completion.Outcome, completion.Stdout.Text = protocol.ProcessOutcome{Kind: "exited", Data: int32(7)}, "SAILRY-EXIT7\n"
			switch fixture {
			case "wrong_exit":
				completion.Outcome.Data = int32(0)
			case "wrong_stdout":
				completion.Stdout.Text = "OTHER\n"
			case "unrestricted":
				arguments["access"] = "unrestricted"
			case "changed_command":
				arguments["command"] = "true"
			case "background":
				arguments["background"] = true
			}
			encoded, err := json.Marshal(protocol.Output{Kind: "command_result", Data: completion})
			if err != nil {
				t.Fatal(err)
			}
			raw := json.RawMessage(encoded)
			detail := protocol.DefaultToolDetail()
			detail.Arguments, err = json.Marshal(arguments)
			if err != nil {
				t.Fatal(err)
			}
			detail.Result = &raw
			actual, err := liveCommandResult(detail)
			if fixture != "original" {
				if err == nil {
					t.Fatal("a changed command or process result satisfied the exact fixture")
				}
				return
			}
			if err != nil {
				t.Fatalf("valid raw failure required an optional display projection: %v", err)
			}
			var unformatted, formatted any
			if json.Unmarshal(raw, &unformatted) != nil || json.Unmarshal([]byte(actual), &formatted) != nil {
				t.Fatal("the visible original result is no longer JSON")
			}
			if !reflect.DeepEqual(unformatted, formatted) || !strings.Contains(actual, "SAILRY-EXIT7") {
				t.Fatal("raw failure body changed while formatting")
			}
		})
	}
}

func TestLiveCoreCommandToolUsesCanonicalNameAndTurnCapture(t *testing.T) {
	for _, fixture := range []string{"native", "commands_installed", "wrong_turn", "wrong_session", "wrong_page_session", "missing_tool", "missing_result", "other_tool", "presentation", "uncaptured_alias"} {
		t.Run(fixture, func(t *testing.T) {
			turn, session := protocol.TurnID{3}, protocol.SessionID{2}
			capture := protocol.DefaultQueuedTurn()
			capture.ID, capture.Session, capture.Request, capture.Kind = turn, session, protocol.RequestID{4}, protocol.ConversationRunKindTask
			capture.Config.Provider, capture.Config.Model = protocol.ProviderID{5}, "fixture"
			capture.Config.Mode, capture.Config.Permission = protocol.WorkModeCode, protocol.PermissionProject
			page := liveCommandToolPage("run_command", session, turn)
			switch fixture {
			case "commands_installed":
				capture.Plugins = []protocol.PluginReference{{Name: "commands", Digest: strings.Repeat("a", 64)}}
			case "wrong_turn":
				capture.ID = protocol.TurnID{9}
			case "wrong_session":
				capture.Session = protocol.SessionID{9}
			case "wrong_page_session":
				page.Session = protocol.SessionID{9}
			case "missing_tool":
				page.Entries = nil
			case "missing_result":
				page.Entries = page.Entries[:1]
			case "other_tool":
				page = liveCommandToolPage("another_run_command", session, turn)
			case "presentation":
				call := page.Entries[0].Parts[0].Data.(protocol.ConversationPartToolCallPayload)
				call.Presentation = protocol.ToolPresentationSummary
				page.Entries[0].Parts[0].Data = call
			case "uncaptured_alias":
				page = liveCommandToolPage(core.PackageToolAlias("commands", "run_command"), session, turn)
			}
			reads := 0
			controller := client.New(&liveFixtureTransport{dispatch: func(_ context.Context, request protocol.Request) (protocol.Output, error) {
				input, ok := request.Command.Data.(*protocol.CommandReadTurnCapturePayload)
				if request.Command.Kind != "read_turn_capture" || !ok || input.Session != session || input.Turn != turn {
					t.Fatalf("native command resolution requested a package or another turn: %q", request.Command.Kind)
				}
				reads++
				return protocol.Output{Kind: "queued_turn", Data: capture}, nil
			}})
			t.Cleanup(func() { _ = controller.Close() })
			w := &liveWindow{window: &window{ctx: t.Context(), result: &core.CaseResult{}}, controller: controller, turn: turn, session: protocol.Session{ID: session}}
			name, err := w.commandToolName(page)
			valid := fixture == "native" || fixture == "commands_installed"
			if reads != 1 || (err == nil) != valid || valid && name != "run_command" {
				t.Fatalf("native command identity or capture was not enforced: name=%q reads=%d error=%v", name, reads, err)
			}
		})
	}
}

func TestLiveCommandToolUsesItsCapturedDeclaration(t *testing.T) {
	for _, fixture := range []string{"declared", "other_owner", "digest", "summary", "server", "undeclared", "wrong_call"} {
		t.Run(fixture, func(t *testing.T) {
			capture := protocol.DefaultQueuedTurn()
			capture.ID, capture.Session, capture.Request, capture.Kind = protocol.TurnID{3}, protocol.SessionID{2}, protocol.RequestID{4}, protocol.ConversationRunKindTask
			capture.Config.Provider, capture.Config.Model = protocol.ProviderID{5}, "fixture"
			capture.Config.Mode, capture.Config.Permission = protocol.WorkModeCode, protocol.PermissionProject
			reference := protocol.PluginReference{Name: "commands", Digest: strings.Repeat("a", 64)}
			if fixture == "other_owner" {
				reference.Name = "another-package"
			}
			capture.Plugins = []protocol.PluginReference{reference}
			info := protocol.DefaultPluginInfo()
			info.Summary.Name, info.Summary.Digest = reference.Name, reference.Digest
			if fixture == "digest" {
				info.Summary.Digest = strings.Repeat("b", 64)
			}
			extension := protocol.DefaultPluginExtension()
			tool := protocol.DefaultToolDeclaration()
			tool.Name, tool.Presentation = "run_command", protocol.ToolPresentationContent
			if fixture == "summary" {
				tool.Presentation = protocol.ToolPresentationSummary
			}
			if fixture == "server" {
				tool.Server = new("fixture")
			}
			if fixture == "undeclared" {
				tool.Name = "another_tool"
			}
			extension.Tools, info.Extension = []protocol.ToolDeclaration{tool}, &extension
			transport := &liveFixtureTransport{dispatch: func(_ context.Context, request protocol.Request) (protocol.Output, error) {
				switch input := request.Command.Data.(type) {
				case *protocol.CommandReadTurnCapturePayload:
					if input.Session != capture.Session || input.Turn != capture.ID {
						t.Fatal("command declaration read outside its UI-sent turn")
					}
					return protocol.Output{Kind: "queued_turn", Data: capture}, nil
				case *protocol.CommandReadPluginVersionPayload:
					if input.Package != reference {
						t.Fatal("command declaration changed the captured package version")
					}
					return protocol.Output{Kind: "plugin", Data: info}, nil
				default:
					t.Fatalf("unexpected declaration command %q", request.Command.Kind)
				}
				return protocol.Output{}, nil
			}}
			controller := client.New(transport)
			t.Cleanup(func() { _ = controller.Close() })
			w := &liveWindow{window: &window{ctx: t.Context(), result: &core.CaseResult{}}, controller: controller, turn: capture.ID, session: protocol.Session{ID: capture.Session}}
			page := liveCommandToolPage(core.PackageToolAlias(reference.Name, "run_command"), capture.Session, capture.ID)
			if fixture == "wrong_call" {
				page = liveCommandToolPage(core.PackageToolAlias("another-package", "run_command"), capture.Session, capture.ID)
			}
			name, err := w.commandToolName(page)
			if fixture != "declared" {
				if err == nil {
					t.Fatal("another owner, version or presentation satisfied the command fixture")
				}
				return
			}
			if err != nil || name != core.PackageToolAlias("commands", "run_command") {
				t.Fatalf("captured command identity was not resolved: %v, %q", err, name)
			}
		})
	}
}
