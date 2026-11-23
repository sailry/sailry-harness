package store

import (
	"bytes"
	"context"
	"database/sql"
	"encoding/json"
	"time"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// insertAgentUsageRecord projects a reportable canonical response into the
// compact agent_usage_records index so usage reports never scan full message
// payloads from agent_events.
func insertAgentUsageRecord(ctx context.Context, db *sql.Conn, sequence uint64, session protocol.SessionID, record canonical.Record) error {
	message := record.MutableMessage()
	if message == nil || message.Usage == nil {
		return nil
	}
	usage := message.Usage
	exec := record.Metadata.Execution
	if !reportableCanonicalUsage(usage, exec) {
		return nil
	}
	trimmed := trimCanonicalUsage(*usage)
	usageBody, err := encode(trimmed)
	if err != nil {
		return err
	}
	var sourceBody []byte
	var elapsed, first sql.NullInt64
	var media sql.NullString
	reasoningStreamed := 0
	if exec != nil {
		if exec.Source != nil {
			sourceBody, err = encode(exec.Source)
			if err != nil {
				return err
			}
		}
		if exec.Timing != nil {
			if exec.Timing.ElapsedUs != nil {
				elapsed = sql.NullInt64{Int64: int64(*exec.Timing.ElapsedUs), Valid: true}
			}
			if exec.Timing.FirstTokenUs != nil {
				first = sql.NullInt64{Int64: int64(*exec.Timing.FirstTokenUs), Valid: true}
			}
		}
		if exec.Media != nil {
			media = sql.NullString{String: string(*exec.Media), Valid: true}
		}
		if exec.Observation != nil && len(exec.Observation.Attempts) > 0 && exec.Observation.Attempts[0].ReasoningStreamed {
			reasoningStreamed = 1
		}
	}
	created := record.Created.Format(time.RFC3339Nano)
	createdSec := created
	if len(createdSec) > 19 {
		createdSec = createdSec[:19]
	}
	_, err = db.ExecContext(ctx,
		`INSERT INTO agent_usage_records(sequence,session,turn,created_sec,created,usage,source,elapsed_us,first_token_us,media,reasoning_streamed)
		 VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)`,
		sequence, session.String(), record.Turn.String(), createdSec, created, usageBody, sourceBody, elapsed, first, media, reasoningStreamed)
	return err
}

func reportableCanonicalUsage(usage *canonical.Usage, exec *canonical.Execution) bool {
	if usage == nil {
		return false
	}
	if usage.Input != nil && usage.Output != nil {
		return true
	}
	if exec == nil {
		return false
	}
	if exec.Media != nil {
		return true
	}
	if exec.Purpose != nil && *exec.Purpose == canonical.PurposeSearch {
		return true
	}
	return exec.Timing != nil && exec.Timing.ElapsedUs != nil && *exec.Timing.ElapsedUs > 0 && (usage.Input != nil || usage.Output != nil)
}

// trimCanonicalUsage keeps only cache_creation from a provider usage object,
// matching agentUsageTrimmed so large provider payloads stay out of reports.
func trimCanonicalUsage(usage canonical.Usage) canonical.Usage {
	raw := bytes.TrimSpace(usage.Provider)
	if len(raw) == 0 || raw[0] != '{' {
		return usage
	}
	var object map[string]json.RawMessage
	if json.Unmarshal(raw, &object) != nil || object == nil {
		return usage
	}
	creation := json.RawMessage("null")
	if value, exists := object["cache_creation"]; exists && len(bytes.TrimSpace(value)) > 0 {
		creation = value
	}
	body, err := json.Marshal(struct {
		CacheCreation json.RawMessage `json:"cache_creation"`
	}{CacheCreation: creation})
	if err == nil {
		usage.Provider = body
	}
	return usage
}

const agentUsageBackfillSQL = `INSERT OR IGNORE INTO agent_usage_records(
    sequence,session,turn,created_sec,created,usage,source,elapsed_us,first_token_us,media,reasoning_streamed
)
SELECT
    e.sequence,
    e.session,
    e.turn,
    substr(json_extract(e.body,'$.created'),1,19),
    json_extract(e.body,'$.created'),
    CAST(CASE
        WHEN json_type(coalesce(json_extract(e.body,'$.usage'),json_extract(e.body,'$.summary.usage')),'$.provider')='object'
        THEN json_set(
            json_remove(coalesce(json_extract(e.body,'$.usage'),json_extract(e.body,'$.summary.usage')),'$.provider'),
            '$.provider',
            json_object('cache_creation',json(coalesce(json_extract(e.body,'$.usage'),json_extract(e.body,'$.summary.usage')) -> '$.provider.cache_creation'))
        )
        ELSE coalesce(json_extract(e.body,'$.usage'),json_extract(e.body,'$.summary.usage'))
    END AS BLOB),
    CAST(json_extract(e.body,'$.metadata.execution.source') AS BLOB),
    json_extract(e.body,'$.metadata.execution.timing.elapsed_us'),
    json_extract(e.body,'$.metadata.execution.timing.first_token_us'),
    json_extract(e.body,'$.metadata.execution.media'),
    CASE WHEN coalesce(json_extract(e.body,'$.metadata.execution.observation.attempts[0].reasoning_streamed'),0) THEN 1 ELSE 0 END
FROM agent_events e
WHERE ` + agentReportableUsage

func backfillAgentUsageRecords(ctx context.Context, db *sql.Conn) error {
	_, err := db.ExecContext(ctx, agentUsageBackfillSQL)
	return err
}
