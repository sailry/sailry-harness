package store

import (
	"context"
	"database/sql"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
)

func storedAgentTool(ctx context.Context, db *sql.Conn, record canonical.Record, sequence uint64, block canonical.Block) (*rawAgentTool, error) {
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
	token, err := encode(response.ID)
	if err != nil {
		return nil, err
	}
	records, err := readBodies[canonical.Record](ctx, db, `SELECT body FROM agent_events WHERE turn=?1 AND sequence<=?2
 AND (typeof(body)!='blob' OR instr(body,?6)>0)
 AND json_extract(body,'$.provenance.branch')=?3 AND EXISTS(
 SELECT 1 FROM json_each(body,'$.content') part WHERE json_extract(part.value,'$.type')='tool_request'
 AND json_extract(part.value,'$.id')=?4 AND json_extract(part.value,'$.name')=?5) ORDER BY sequence DESC`, record.Turn.String(), sequence, record.Provenance.Branch, response.ID, response.Name, token)
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
