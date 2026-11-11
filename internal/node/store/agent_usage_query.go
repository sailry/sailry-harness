package store

// agentUsagePreset selects the built-in preset of the channel a response used:
// the one captured with its run or with the media model assigned to the
// session, else the channel as it is saved now.
const agentUsagePreset = `coalesce(
 CASE WHEN json_extract(captured,'$.id')=json_extract(provider,'$.id') AND json_extract(captured,'$.preset.source.kind')='builtin' THEN json_extract(captured,'$.preset.id') END,
 CASE WHEN json_extract(assigned,'$.id')=json_extract(provider,'$.id') AND json_extract(assigned,'$.preset.source.kind')='builtin' THEN json_extract(assigned,'$.preset.id') END,
 (SELECT json_extract(saved.body,'$.provider.preset.id') FROM providers saved WHERE saved.id=json_extract(provider,'$.id') AND json_extract(saved.body,'$.provider.preset.source.kind')='builtin'))`

// The indexed query reads captured execution configuration and canonical usage.
const agentUsageRows = `WITH responses AS (SELECT e.body,coalesce(json_extract(e.body,'$.usage'),json_extract(e.body,'$.summary.usage')) AS usage,e.sequence,e.session,e.turn,s.project,r.worktree,json_extract(r.config,'$.resource') AS resource,json_extract(r.config,'$.assistant') AS assistant,
    coalesce(json_extract(e.body,'$.metadata.execution.source.id'),
        CASE WHEN json_extract(e.body,'$.metadata.execution.media') IS NULL OR m.value IS NULL
        THEN json_extract(r.config,'$.provider') ELSE json_extract(m.value,'$.provider.id') END) AS provider_id,
    coalesce(json_extract(e.body,'$.metadata.execution.source.model'),
        CASE WHEN json_extract(e.body,'$.metadata.execution.media') IS NULL OR m.value IS NULL
        THEN json_extract(r.config,'$.model') ELSE json_extract(m.value,'$.model') END) AS model,
    coalesce(json_extract(e.body,'$.metadata.execution.source'),
        CASE WHEN json_extract(e.body,'$.metadata.execution.media') IS NULL OR m.value IS NULL
        THEN a.provider ELSE json_extract(m.value,'$.provider') END) AS provider,
    a.provider AS captured,json_extract(m.value,'$.provider') AS assigned
    FROM agent_events e JOIN turns t ON t.id=e.turn
    JOIN agent_runs a ON a.turn=t.id
    JOIN session_revisions r ON r.session=t.session AND r.revision=t.revision
    JOIN sessions s ON s.id=t.session
    LEFT JOIN session_media media ON media.session=t.session
    LEFT JOIN json_each(media.body) m ON m.key=json_extract(e.body,'$.metadata.execution.media')
    WHERE coalesce(json_type(e.body,'$.usage'),json_type(e.body,'$.summary.usage'))='object'
    AND coalesce(json_extract(e.body,'$.usage.input'),json_extract(e.body,'$.summary.usage.input'))>=0
    AND coalesce(json_extract(e.body,'$.usage.output'),json_extract(e.body,'$.summary.usage.output'))>=0
    AND substr(json_extract(e.body,'$.created'),1,19)>=?1
    AND substr(json_extract(e.body,'$.created'),1,19)<=?2
    AND (?3='[]' OR s.project IN (SELECT value FROM json_each(?3)))
    AND (?4='[]' OR r.worktree IN (SELECT value FROM json_each(?4))))
    SELECT
    json_extract(e.body,'$.created'),
    max(0,json_extract(e.usage,'$.input')),
    max(0,json_extract(e.usage,'$.output')),
    max(0,coalesce(json_extract(e.usage,'$.cache_read'),0)),
    max(0,coalesce(json_extract(e.usage,'$.reasoning'),0)),
    e.provider_id,e.model,e.project,e.worktree,
    json_extract(e.provider,'$.api'),json_extract(e.provider,'$.authentication'),
    json_extract(e.provider,'$.endpoint'),e.usage,
    json_extract(e.body,'$.metadata.execution.timing.elapsed_us'),
    json_extract(e.body,'$.metadata.execution.media'),e.sequence,e.session,e.turn,
    json_extract(e.body,'$.metadata.execution.timing.first_token_us'),
    json_extract(e.provider,'$.name'),coalesce(
        (SELECT name FROM projects WHERE id=e.project),
        CASE json_extract(e.resource,'$.kind')
            WHEN 'database' THEN (SELECT json_extract(body,'$.name') FROM database_profiles WHERE id=json_extract(e.resource,'$.id'))
            WHEN 'ssh' THEN (SELECT json_extract(body,'$.name') FROM ssh_profiles WHERE id=json_extract(e.resource,'$.id'))
        END, json_extract(e.resource,'$.id'),
        json_extract(e.assistant,'$.package.name') || ':' || json_extract(e.assistant,'$.id'),''),e.resource,e.assistant,` + agentUsagePreset + `
    FROM responses e
    WHERE (?5='[]' OR e.provider_id IN (SELECT value FROM json_each(?5)))
    AND (?6='[]' OR e.model IN (SELECT value FROM json_each(?6)))
    ORDER BY substr(json_extract(e.body,'$.created'),1,19),e.sequence LIMIT ?7`
