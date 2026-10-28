CREATE INDEX IF NOT EXISTS agent_usage ON agent_events(
    turn,
    max(0,json_extract(body,'$.usage_metadata.prompt_token_count')),
    max(0,json_extract(body,'$.usage_metadata.candidates_token_count')),
    max(0,coalesce(json_extract(body,'$.usage_metadata.cache_read_input_token_count'),0)),
    max(0,coalesce(json_extract(body,'$.usage_metadata.thinking_token_count'),0))
) WHERE json_type(body,'$.usage_metadata')='object';
CREATE INDEX IF NOT EXISTS agent_usage_time ON agent_events(
    substr(json_extract(body,'$.timestamp'),1,19),
    sequence,
    turn,
    json_extract(body,'$.timestamp'),
    max(0,json_extract(body,'$.usage_metadata.prompt_token_count')),
    max(0,json_extract(body,'$.usage_metadata.candidates_token_count')),
    max(0,coalesce(json_extract(body,'$.usage_metadata.cache_read_input_token_count'),0)),
    max(0,coalesce(json_extract(body,'$.usage_metadata.thinking_token_count'),0))
) WHERE json_type(body,'$.usage_metadata')='object';
