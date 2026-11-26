package store

// agentUsagePreset selects the built-in preset of the channel a response used:
// the one captured with its run or with the media model assigned to the
// session, else the channel as it is saved now.
const agentUsagePreset = `coalesce(
 CASE WHEN json_extract(captured,'$.id')=json_extract(provider,'$.id') AND json_extract(captured,'$.preset.source.kind')='builtin' THEN json_extract(captured,'$.preset.id') END,
 CASE WHEN json_extract(assigned,'$.id')=json_extract(provider,'$.id') AND json_extract(assigned,'$.preset.source.kind')='builtin' THEN json_extract(assigned,'$.preset.id') END,
 (SELECT json_extract(saved.body,'$.provider.preset.id') FROM providers saved WHERE saved.id=json_extract(provider,'$.id') AND json_extract(saved.body,'$.provider.preset.source.kind')='builtin'))`

// The indexed query reads captured execution configuration and canonical usage.
// The report reads only cache_creation of the provider's own usage, so the
// rest of that object, which can be large, stays in the database.
const agentUsageTrimmed = `CASE WHEN json_type(e.usage,'$.provider')='object' THEN json_set(json_remove(e.usage,'$.provider'),'$.provider',json_object('cache_creation',json(e.usage -> '$.provider.cache_creation'))) ELSE e.usage END`

const (
	agentUsageObject = `coalesce(json_type(e.body,'$.usage'),json_type(e.body,'$.summary.usage'))='object'`
	agentUsageInput  = `coalesce(json_extract(e.body,'$.usage.input'),json_extract(e.body,'$.summary.usage.input'))`
	agentUsageOutput = `coalesce(json_extract(e.body,'$.usage.output'),json_extract(e.body,'$.summary.usage.output'))`
	agentUsagePair   = agentUsageInput + `>=0 AND ` + agentUsageOutput + `>=0`
)

// A partial token count identifies an ordinary request only with a measured
// duration. Media and search explicitly record individual calls.
const agentReportableUsage = agentUsageObject + ` AND ((` + agentUsagePair + `)
 OR ((json_extract(e.body,'$.metadata.execution.media') IS NOT NULL OR json_extract(e.body,'$.metadata.execution.purpose')='search'
 OR (json_extract(e.body,'$.metadata.execution.timing.elapsed_us')>0 AND coalesce(` + agentUsageInput + `,` + agentUsageOutput + `) IS NOT NULL))
 AND coalesce(` + agentUsageInput + `,0)>=0 AND coalesce(` + agentUsageOutput + `,0)>=0))`

// agentUsageRecordRows streams reportable usage facts directly in index order
// (created_sec, sequence) without touching agent_events message blobs, CTEs,
// temp sorts or per-row joins. Turn-level metadata is fetched once per turn.
const agentUsageRecordRows = `SELECT
    u.created,u.sequence,u.session,u.turn,u.usage,u.source,
    u.elapsed_us,u.first_token_us,u.media,u.reasoning_streamed
FROM agent_usage_records u
WHERE u.created_sec>=?1 AND u.created_sec<=?2
ORDER BY u.created_sec,u.sequence`

const agentUsageTurnMetaBatchQuery = `SELECT t.id,s.project,s.owner,r.worktree,r.config,a.provider
FROM turns t
JOIN agent_runs a ON a.turn=t.id
JOIN session_revisions r ON r.session=t.session AND r.revision=t.revision
JOIN sessions s ON s.id=t.session
WHERE t.id IN (SELECT CAST(value AS TEXT) FROM json_each(?1))`
