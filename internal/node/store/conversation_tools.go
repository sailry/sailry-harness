package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// A history page only needs the requests paired with responses on that page.
// A single-detail read can narrow the lookup to one call and branch.
type toolRequests map[protocol.TurnID]*pageToolRequests

type pageToolRequests struct {
	ids     map[string]bool
	matches map[toolCallKey][]toolRequestRecord
}
type toolCallKey struct{ branch, id, name string }
type toolRequestRecord struct {
	sequence uint64
	tool     *rawAgentTool
}

func (requests toolRequests) include(record canonical.Record) {
	if m := record.Message(); m != nil {
		for _, block := range m.Content {
			if r, ok := block.Data.(*canonical.ToolResponse); ok {
				p := requests[record.Turn]
				if p == nil {
					p = &pageToolRequests{ids: map[string]bool{}}
					requests[record.Turn] = p
				}
				p.ids[r.ID] = true
			}
		}
	}
}

func (requests toolRequests) find(ctx context.Context, db *sql.Conn, record canonical.Record, sequence uint64, response *canonical.ToolResponse) (*rawAgentTool, error) {
	key := toolCallKey{record.Provenance.Branch, response.ID, response.Name}
	page := requests[record.Turn]
	single := page == nil
	if single {
		page = &pageToolRequests{ids: map[string]bool{response.ID: true}}
	}
	if page.matches == nil {
		var rows *sql.Rows
		var err error
		if single {
			token, e := encode(response.ID)
			if e != nil {
				return nil, e
			}
			rows, err = db.QueryContext(ctx, `SELECT sequence,body FROM agent_events WHERE turn=?1 AND sequence<=?2
 AND (typeof(body)!='blob' OR instr(body,?6)>0)
 AND json_extract(body,'$.provenance.branch')=?3 AND EXISTS(
 SELECT 1 FROM json_each(body,'$.content') part WHERE json_extract(part.value,'$.type')='tool_request'
 AND json_extract(part.value,'$.id')=?4 AND json_extract(part.value,'$.name')=?5)`, record.Turn.String(), sequence, key.branch, key.id, key.name, token)
		} else {
			ids, e := encode(page.ids)
			if e != nil {
				return nil, e
			}
			rows, err = db.QueryContext(ctx, `SELECT sequence,body FROM agent_events WHERE turn=?1 AND EXISTS(
 SELECT 1 FROM json_each(body,'$.content') part WHERE json_extract(part.value,'$.type')='tool_request'
 AND json_extract(part.value,'$.id') IN (SELECT key FROM json_each(?2)))`, record.Turn.String(), string(ids))
		}
		if err != nil {
			return nil, err
		}
		defer rows.Close()
		page.matches = map[toolCallKey][]toolRequestRecord{}
		for rows.Next() {
			var seq uint64
			var body sql.RawBytes
			var r canonical.Record
			if err = rows.Scan(&seq, &body); err != nil {
				return nil, err
			}
			if err = json.Unmarshal(body, &r); err != nil {
				return nil, err
			}
			if m := r.Message(); m != nil {
				for _, b := range m.Content {
					if c, ok := b.Data.(*canonical.ToolRequest); ok && page.ids[c.ID] {
						k := toolCallKey{r.Provenance.Branch, c.ID, c.Name}
						page.matches[k] = append(page.matches[k], toolRequestRecord{seq, rawTool(r, b)})
					}
				}
			}
		}
		if err = errors.Join(rows.Err(), rows.Close()); err != nil {
			return nil, err
		}
	}
	var found *rawAgentTool
	for _, v := range page.matches[key] {
		if v.sequence <= sequence {
			if found != nil {
				return nil, nil
			}
			copy := *v.tool
			found = &copy
		}
	}
	return found, nil
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
	source, err := requests.find(ctx, db, record, sequence, response)
	if err != nil {
		return nil, err
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
