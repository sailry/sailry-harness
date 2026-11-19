package store

import (
	"encoding/json"
	"reflect"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/agent/operations"
	"github.com/sailry/sailry-harness/internal/protocol"
)

const (
	pendingAgentPlan  = `{"title":"Release","steps":[{"description":"Inspect","state":"completed"},{"description":"Verify","state":"pending"},{"description":"Publish","state":"skipped"}]}`
	activeAgentPlan   = `{"title":"Release","steps":[{"description":"Inspect","state":"completed"},{"description":"Verify","state":"in_progress"},{"description":"Publish","state":"skipped"}]}`
	finishedAgentPlan = `{"title":"Release","steps":[{"description":"Inspect","state":"completed"},{"description":"Verify","state":"completed"},{"description":"Publish","state":"skipped"}]}`
)

func agentPlanRecords(turn protocol.TurnID, branch, id, name, arguments, result string) (canonical.Record, canonical.Record) {
	call := &canonical.ToolRequest{ID: id, Name: name, Arguments: json.RawMessage(arguments)}
	response := &canonical.ToolResponse{
		ID: id, Name: name, Status: canonical.ToolSuccess,
		Result: canonical.Payload{Value: json.RawMessage(`{"progress":` + result + `}`)},
	}
	requestRecord := canonical.NewRecord(turn, "message", canonical.NewMessage(canonical.Assistant, canonical.Block{Kind: "tool_request", Data: call}), time.Time{})
	requestRecord.Provenance = canonical.Provenance{Author: "assistant", Branch: branch}
	responseRecord := canonical.NewRecord(turn, "message", canonical.NewMessage(canonical.User, canonical.Block{Kind: "tool_response", Data: response}), time.Time{})
	responseRecord.Provenance = canonical.Provenance{Author: "tool", Branch: branch}
	return requestRecord, responseRecord
}

func requireAgentPlan(t *testing.T, records []canonical.Record, expected string) {
	t.Helper()
	var want protocol.ConversationProgress
	if err := json.Unmarshal([]byte(expected), &want); err != nil {
		t.Fatal(err)
	}
	if got := LatestAgentPlan(records); got == nil || !reflect.DeepEqual(*got, want) {
		t.Fatalf("latest plan = %+v, want %+v", got, want)
	}
}

func TestLatestAgentPlanPreservesHistoryAcrossGuidance(t *testing.T) {
	oldTurn := protocol.NewTurnID()
	// Historical progress.update and the current handler use the same package alias.
	name := operations.Alias("progress", "update_plan")
	oldCall, oldResult := agentPlanRecords(oldTurn, "main", "plan", name, pendingAgentPlan, pendingAgentPlan)
	planCall, planResult := agentPlanRecords(oldTurn, "main", "update", name, activeAgentPlan, activeAgentPlan)
	readCall, readResult := agentPlanRecords(oldTurn, "main", "read", operations.Alias("files", "read_file"), `{"path":"fixture"}`, `{}`)
	callBlock := planCall.Message().Content[0]
	callBlock.Index = 1
	planCall.Data = canonical.NewMessage(canonical.Assistant, readCall.Message().Content[0], callBlock)
	planBlock := planResult.Message().Content[0]
	readBlock := readResult.Message().Content[0]
	readBlock.Index = 1
	planResult.Data = canonical.NewMessage(canonical.User, planBlock, readBlock)
	readResponse := readBlock.Data.(*canonical.ToolResponse)
	readResponse.Status = canonical.ToolErrorStatus
	newTurn := protocol.NewTurnID()
	guidance := canonical.NewRecord(newTurn, "message", canonical.NewMessage(canonical.User, canonical.Text(0, "Continue, but skip publishing.")), time.Time{})
	guidance.Provenance = canonical.Provenance{Author: "user", Branch: "continued"}
	guidance.Metadata.Runtime = &canonical.Runtime{Steer: true}
	compaction := canonical.NewRecord(newTurn, "compaction", canonical.Compaction{Boundary: oldResult.ID, Summary: canonical.NewMessage(canonical.Assistant, canonical.Text(0, "Earlier discussion"))}, time.Time{})
	records := []canonical.Record{oldCall, oldResult, planCall, planResult, guidance, compaction}
	before, err := json.Marshal(records)
	if err != nil {
		t.Fatal(err)
	}
	requireAgentPlan(t, records, activeAgentPlan)
	after, err := json.Marshal(records)
	if err != nil {
		t.Fatal(err)
	}
	if string(before) != string(after) {
		t.Fatal("restoring the plan changed canonical history")
	}
}

func TestLatestAgentPlanIgnoresInvalidUpdates(t *testing.T) {
	name := operations.Alias("progress", "update_plan")
	turn := protocol.NewTurnID()
	oldCall, oldResult := agentPlanRecords(turn, "main", "previous", name, pendingAgentPlan, pendingAgentPlan)
	for _, scenario := range []string{
		"error status", "error format", "captured failure", "invalid request", "receipt mismatch", "wrong ID", "wrong name", "wrong branch", "wrong turn", "wrong author", "receipt before request", "third-party plan", "third-party progress operation",
	} {
		t.Run(scenario, func(t *testing.T) {
			call, result := agentPlanRecords(turn, "main", "update", name, activeAgentPlan, activeAgentPlan)
			request := call.Message().Content[0].Data.(*canonical.ToolRequest)
			response := result.Message().Content[0].Data.(*canonical.ToolResponse)
			switch scenario {
			case "error status":
				response.Status = canonical.ToolErrorStatus
			case "error format":
				response.Format = canonical.MCPError
			case "captured failure":
				result.Metadata.Presentation = &canonical.Presentation{Failures: []canonical.ToolFailure{{Index: 0, ID: response.ID, Name: name, Failure: protocol.ToolFailure{Origin: protocol.ToolFailureOriginTool}}}}
			case "invalid request":
				request.Error = &canonical.ToolError{Message: "Invalid tool request"}
			case "receipt mismatch":
				response.Result.Value = json.RawMessage(`{"progress":` + finishedAgentPlan + `}`)
			case "wrong ID":
				response.ID = "different"
			case "wrong name":
				request.Name = operations.Alias("other", "update_plan")
			case "wrong branch":
				result.Provenance.Branch = "elsewhere"
			case "wrong turn":
				result.Turn = protocol.NewTurnID()
			case "wrong author":
				result.Provenance.Author = "user"
			case "receipt before request":
				call, result = result, call
			case "third-party plan", "third-party progress operation":
				alias := operations.Alias("other", "update_plan")
				if scenario == "third-party progress operation" {
					alias = operations.Alias("task-notes", "progress")
				}
				request.Name, response.Name = alias, alias
				call.Metadata.Presentation = &canonical.Presentation{Tools: map[string]protocol.ToolPresentation{alias: protocol.ToolPresentationProgress}}
			}
			requireAgentPlan(t, []canonical.Record{oldCall, oldResult, call, result}, pendingAgentPlan)
		})
	}
}

func TestLatestAgentPlanSettledUpdateClearsOlderPlan(t *testing.T) {
	name := operations.Alias("progress", "update_plan")
	turn := protocol.NewTurnID()
	olderCall, olderResult := agentPlanRecords(turn, "main", "earlier", name, pendingAgentPlan, pendingAgentPlan)
	call, result := agentPlanRecords(turn, "main", "update", name, finishedAgentPlan, finishedAgentPlan)
	_, duplicate := agentPlanRecords(turn, "main", "earlier", name, pendingAgentPlan, pendingAgentPlan)
	failedCall, failedResult := agentPlanRecords(turn, "main", "broken", name, activeAgentPlan, activeAgentPlan)
	failedResult.Message().Content[0].Data.(*canonical.ToolResponse).Status = canonical.ToolErrorStatus
	if plan := LatestAgentPlan([]canonical.Record{olderCall, olderResult, call, result, duplicate, failedCall, failedResult}); plan != nil {
		t.Fatalf("settled plan must not return or reveal an older plan: %+v", plan)
	}
}
