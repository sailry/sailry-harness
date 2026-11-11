package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// toolRequests keeps the tool-request records of each turn a page has resolved,
// so a page scans a turn once instead of once per response. A nil value
// resolves a single response with one narrow query.
type toolRequests map[protocol.TurnID][]toolRequestRecord

type toolRequestRecord struct {
	sequence uint64
	record   canonical.Record
}

func (requests toolRequests) find(ctx context.Context, db *sql.Conn, record canonical.Record, sequence uint64, response *canonical.ToolResponse) ([]canonical.Record, error) {
	if requests == nil {
		token, err := encode(response.ID)
		if err != nil {
			return nil, err
		}
		return readBodies[canonical.Record](ctx, db, `SELECT body FROM agent_events WHERE turn=?1 AND sequence<=?2
 AND (typeof(body)!='blob' OR instr(body,?6)>0)
 AND json_extract(body,'$.provenance.branch')=?3 AND EXISTS(
 SELECT 1 FROM json_each(body,'$.content') part WHERE json_extract(part.value,'$.type')='tool_request'
 AND json_extract(part.value,'$.id')=?4 AND json_extract(part.value,'$.name')=?5) ORDER BY sequence DESC`, record.Turn.String(), sequence, record.Provenance.Branch, response.ID, response.Name, token)
	}
	turn, ok := requests[record.Turn]
	if !ok {
		rows, err := db.QueryContext(ctx, `SELECT sequence,body FROM agent_events WHERE turn=?1 AND EXISTS(
 SELECT 1 FROM json_each(body,'$.content') part WHERE json_extract(part.value,'$.type')='tool_request')`, record.Turn.String())
		if err != nil {
			return nil, err
		}
		turn = []toolRequestRecord{}
		for rows.Next() {
			var value toolRequestRecord
			var body []byte
			if err = rows.Scan(&value.sequence, &body); err == nil {
				err = json.Unmarshal(body, &value.record)
			}
			if err != nil {
				rows.Close()
				return nil, err
			}
			turn = append(turn, value)
		}
		if err = errors.Join(rows.Err(), rows.Close()); err != nil {
			return nil, err
		}
		requests[record.Turn] = turn
	}
	records := []canonical.Record{}
	for _, value := range turn {
		if value.sequence <= sequence && value.record.Provenance.Branch == record.Provenance.Branch {
			records = append(records, value.record)
		}
	}
	return records, nil
}

func storedAgentTool(ctx context.Context, db *sql.Conn, record canonical.Record, sequence uint64, block canonical.Block, requests toolRequests) (*rawAgentTool, error) {
	raw := rawTool(record, block)
	if raw == nil {
		return nil, nil
	}
	response, ok := block.Data.(*canonical.ToolResponse)
	if !ok {
		return raw, nil
	}
	hydrated, err := HydrateCanonicalPayload(ctx, db, response.Result)
	if err != nil {
		return nil, err
	}
	raw.Result = &hydrated
	images, err := projectAgentImages(ctx, db, record, block, response)
	if err != nil {
		return nil, err
	}
	raw.Images = images
	records, err := requests.find(ctx, db, record, sequence, response)
	if err != nil {
		return nil, err
	}
	var source *rawAgentTool
	for _, r := range records {
		if m := r.Message(); m != nil {
			for _, b := range m.Content {
				if call, ok := b.Data.(*canonical.ToolRequest); ok && call.ID == response.ID && call.Name == response.Name {
					if source != nil {
						return raw, nil
					}
					source = rawTool(r, b)
				}
			}
		}
	}
	if source != nil {
		source.Result = raw.Result
		source.Format = raw.Format
		source.Failure = raw.Failure
		source.Error = raw.Error
		source.Images = raw.Images
		return source, nil
	}
	return raw, nil
}
