package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// toolsCompleted announces the tool batch that record, just stored at
// sequence, finishes: the turn's latest assistant message that requested
// tools has every request answered, and record holds the last answer.
// Completion follows the batch, not each result.
func toolsCompleted(ctx context.Context, db *sql.Conn, session protocol.SessionID, record canonical.Record, sequence uint64) ([]protocol.Event, error) {
	if len(toolResponses(record)) == 0 {
		return nil, nil
	}
	turn := record.Turn.String()
	var batch uint64
	var body []byte
	err := db.QueryRowContext(ctx, `SELECT sequence,body FROM agent_events e WHERE turn=?1 AND sequence<?2
		AND json_extract(body,'$.role')='assistant'
		AND EXISTS(SELECT 1 FROM json_each(e.body,'$.content') p WHERE json_extract(p.value,'$.type')='tool_request')
		ORDER BY sequence DESC LIMIT 1`, turn, sequence).Scan(&batch, &body)
	if errors.Is(err, sql.ErrNoRows) {
		return nil, nil
	}
	if err != nil {
		return nil, err
	}
	var requested canonical.Record
	if err = json.Unmarshal(body, &requested); err != nil {
		return nil, err
	}
	type call struct{ name, id string }
	open := map[call]int{}
	if message := requested.Message(); message != nil {
		for _, block := range message.Content {
			if request, ok := block.Data.(*canonical.ToolRequest); ok {
				open[call{request.Name, request.ID}]++
			}
		}
	}
	rows, err := db.QueryContext(ctx, `SELECT sequence,body FROM agent_events WHERE turn=?1 AND sequence>?2 AND sequence<=?3
		AND EXISTS(SELECT 1 FROM json_each(body,'$.content') p WHERE json_extract(p.value,'$.type')='tool_response')
		ORDER BY sequence`, turn, batch, sequence)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	for rows.Next() {
		// A result after the batch was already answered belongs to no batch.
		if len(open) == 0 {
			return nil, nil
		}
		var at uint64
		if err = rows.Scan(&at, &body); err != nil {
			return nil, err
		}
		var answered canonical.Record
		if err = json.Unmarshal(body, &answered); err != nil {
			return nil, err
		}
		for _, response := range toolResponses(answered) {
			key := call{response.Name, response.ID}
			if open[key] > 1 {
				open[key]--
			} else {
				delete(open, key)
			}
		}
		if at == sequence {
			if len(open) != 0 {
				return nil, nil
			}
			return []protocol.Event{{Kind: "tools_completed", Data: protocol.EventToolsCompletedPayload{Session: session}}}, rows.Err()
		}
	}
	return nil, rows.Err()
}

func toolResponses(record canonical.Record) []*canonical.ToolResponse {
	var values []*canonical.ToolResponse
	if message := record.Message(); message != nil {
		for _, block := range message.Content {
			if response, ok := block.Data.(*canonical.ToolResponse); ok {
				values = append(values, response)
			}
		}
	}
	return values
}
