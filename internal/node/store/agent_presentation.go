package store

import (
	"context"
	"database/sql"
	"encoding/hex"
	"encoding/json"
	"fmt"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/protocol"
	"lukechampine.com/blake3"
)

func prepareCanonicalRecord(ctx context.Context, db *sql.Conn, record *canonical.Record) error {
	message := record.Message()
	if message == nil {
		return nil
	}
	denials, err := readBodies[protocol.ConversationApproval](ctx, db, "SELECT body FROM agent_approvals WHERE turn=?1 AND json_extract(body,'$.state')='denied' AND json_type(body,'$.reason')='text' ORDER BY rowid", record.Turn.String())
	if err != nil {
		return err
	}
	for _, denial := range denials {
		request, e := approvalRequest(ctx, db, denial)
		if e != nil {
			return e
		}
		for _, block := range message.Content {
			if response, ok := block.Data.(*canonical.ToolResponse); ok && response.Format == canonical.JSON && response.ID == request.ID && response.Name == request.Name {
				body, e := encode(map[string]string{"error": fmt.Sprintf("Tool execution was denied. Reason: %s", *denial.Reason)})
				if e != nil {
					return e
				}
				sum := blake3.Sum256(body)
				response.Revision = hex.EncodeToString(sum[:])
				response.Result = canonical.Payload{Value: body}
				response.Status = canonical.ToolErrorStatus
			}
		}
	}
	for _, block := range message.Content {
		response, ok := block.Data.(*canonical.ToolResponse)
		if !ok || response.Status != canonical.ToolErrorStatus {
			continue
		}
		if record.Metadata.Presentation == nil {
			record.Metadata.Presentation = &canonical.Presentation{}
		}
		presentation := record.Metadata.Presentation
		exists := false
		for _, v := range presentation.Failures {
			if v.Index == block.Index && v.ID == response.ID && v.Name == response.Name {
				exists = true
				break
			}
		}
		if exists {
			continue
		}
		failure := protocol.ToolFailure{Origin: protocol.ToolFailureOriginRuntime}
		if response.Format == canonical.MCP || response.Format == canonical.MCPError {
			failure.Origin = protocol.ToolFailureOriginTool
		}
		if response.Format == canonical.MCPError {
			value := agentObject(agentJSON(response.Result.Value))
			body, _ := json.Marshal(value["data"])
			var fault protocol.Fault
			if json.Unmarshal(body, &fault) == nil {
				failure.Origin = protocol.ToolFailureOriginRuntime
				failure.Code = &fault.Code
			}
		}
		presentation.Failures = append(presentation.Failures, canonical.ToolFailure{Index: block.Index, ID: response.ID, Name: response.Name, Failure: failure})
	}
	return nil
}
