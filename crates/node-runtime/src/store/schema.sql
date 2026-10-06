CREATE TABLE IF NOT EXISTS node (
    singleton INTEGER PRIMARY KEY CHECK(singleton=1),
    identity BLOB NOT NULL CHECK(length(identity)=32),
    plugin_defaults INTEGER NOT NULL DEFAULT 0 CHECK(plugin_defaults IN (0,1))
);
CREATE TABLE IF NOT EXISTS requests (
    caller BLOB NOT NULL,
    id TEXT NOT NULL,
    body BLOB NOT NULL,
    status TEXT NOT NULL CHECK(status IN ('admitted','completed','unknown')),
    result TEXT,
    PRIMARY KEY(caller,id)
);
CREATE TABLE IF NOT EXISTS projects (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    path TEXT NOT NULL UNIQUE,
    appearance BLOB NOT NULL
);
CREATE TABLE IF NOT EXISTS defaults (
    singleton INTEGER PRIMARY KEY CHECK(singleton=1),
    revision INTEGER NOT NULL,
    config BLOB NOT NULL
);
CREATE TABLE IF NOT EXISTS removed_projects (
    project TEXT PRIMARY KEY REFERENCES projects(id)
);
CREATE TABLE IF NOT EXISTS sessions (
    id TEXT PRIMARY KEY,
    state TEXT NOT NULL DEFAULT 'active' CHECK(state IN ('active','archived','removed')),
    project TEXT REFERENCES projects(id),
    worktree TEXT REFERENCES worktrees(id),
    revision INTEGER NOT NULL,
    history_revision INTEGER NOT NULL DEFAULT 1
);
CREATE TABLE IF NOT EXISTS session_attention (
    session TEXT PRIMARY KEY REFERENCES sessions(id),
    revision INTEGER NOT NULL,
    unread INTEGER NOT NULL CHECK(unread IN (0,1))
);
CREATE TABLE IF NOT EXISTS worktrees (
    id TEXT PRIMARY KEY,
    project TEXT REFERENCES projects(id),
    path TEXT NOT NULL UNIQUE,
    main INTEGER NOT NULL CHECK(main IN (0,1))
);
CREATE UNIQUE INDEX IF NOT EXISTS worktree_main ON worktrees(project) WHERE main=1;
CREATE TABLE IF NOT EXISTS attachments (
    id TEXT PRIMARY KEY,
    worktree TEXT NOT NULL REFERENCES worktrees(id) ON DELETE CASCADE,
    caller BLOB NOT NULL,
    body BLOB NOT NULL
);
CREATE TABLE IF NOT EXISTS session_revisions (
    session TEXT NOT NULL REFERENCES sessions(id),
    revision INTEGER NOT NULL,
    worktree TEXT NOT NULL REFERENCES worktrees(id),
    config BLOB NOT NULL,
    profile BLOB,
    roles BLOB NOT NULL,
    PRIMARY KEY(session,revision)
);
CREATE TABLE IF NOT EXISTS media_settings (
    singleton INTEGER PRIMARY KEY CHECK(singleton=1),
    body BLOB NOT NULL
);
CREATE TABLE IF NOT EXISTS session_media (
    session TEXT PRIMARY KEY REFERENCES sessions(id) ON DELETE CASCADE,
    body BLOB NOT NULL
);
CREATE TABLE IF NOT EXISTS turns (
    id TEXT PRIMARY KEY,
    kind TEXT NOT NULL CHECK(kind IN ('task','compaction')),
    plugins BLOB NOT NULL,
    session TEXT NOT NULL,
    revision INTEGER NOT NULL,
    request TEXT NOT NULL,
    caller BLOB NOT NULL,
    FOREIGN KEY(session,revision) REFERENCES session_revisions(session,revision),
    FOREIGN KEY(caller,request) REFERENCES requests(caller,id)
);
CREATE TABLE IF NOT EXISTS events (
    cursor INTEGER PRIMARY KEY AUTOINCREMENT,
    body BLOB NOT NULL
);
CREATE TABLE IF NOT EXISTS session_delegations (
    session TEXT PRIMARY KEY REFERENCES sessions(id),
    parent_turn TEXT NOT NULL REFERENCES turns(id),
    entry TEXT NOT NULL,
    part INTEGER NOT NULL,
    body BLOB NOT NULL,
    UNIQUE(parent_turn,entry,part)
);
CREATE TABLE IF NOT EXISTS link_peers (
    identity BLOB PRIMARY KEY CHECK(length(identity)=32)
);
CREATE TABLE IF NOT EXISTS link_addresses (
    identity BLOB PRIMARY KEY REFERENCES link_peers(identity) ON DELETE CASCADE,
    address TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS plugin_settings (
    plugin TEXT NOT NULL,
    revision INTEGER NOT NULL,
    body BLOB NOT NULL,
    PRIMARY KEY(plugin,revision)
);
CREATE TABLE plugin_values (
    name TEXT NOT NULL,
    key TEXT NOT NULL,
    revision INTEGER NOT NULL CHECK(revision>0),
    value BLOB,
    index_data BLOB,
    PRIMARY KEY(name,key)
);
-- Opaque private checkpoints share immutable records across conversation branches.
CREATE TABLE plugin_conversation_records (
    sequence INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    key TEXT NOT NULL,
    value BLOB
);
CREATE TABLE plugin_conversation_history (
    session TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    record INTEGER NOT NULL REFERENCES plugin_conversation_records(sequence),
    turn TEXT REFERENCES turns(id),
    PRIMARY KEY(session,record)
);
CREATE INDEX plugin_conversation_history_turn ON plugin_conversation_history(session,turn,record);
CREATE TABLE plugin_conversation_values (
    name TEXT NOT NULL,
    session TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    key TEXT NOT NULL,
    revision INTEGER NOT NULL CHECK(revision>0),
    record INTEGER REFERENCES plugin_conversation_records(sequence),
    restored INTEGER NOT NULL CHECK(restored IN (0,1)),
    PRIMARY KEY(name,session,key)
);
CREATE TABLE IF NOT EXISTS terminals (
    id TEXT PRIMARY KEY,
    worktree TEXT REFERENCES worktrees(id) ON DELETE CASCADE,
    body BLOB NOT NULL
);
CREATE TABLE IF NOT EXISTS terminal_settings (
    singleton INTEGER PRIMARY KEY CHECK(singleton=1),
    body BLOB NOT NULL
);
CREATE TABLE IF NOT EXISTS providers (
    id TEXT PRIMARY KEY,
    body BLOB NOT NULL
);
CREATE TABLE IF NOT EXISTS roles (
    id TEXT PRIMARY KEY,
    key TEXT NOT NULL UNIQUE,
    body BLOB NOT NULL
);
CREATE TABLE IF NOT EXISTS ssh_profiles (
    id TEXT PRIMARY KEY,
    body BLOB NOT NULL,
    credential TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS database_profiles (
    id TEXT PRIMARY KEY,
    body BLOB NOT NULL,
    password TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS agent_runs (
    turn TEXT PRIMARY KEY REFERENCES turns(id),
    ready INTEGER NOT NULL CHECK(ready IN (0,1)),
    body BLOB NOT NULL,
    provider BLOB
);
CREATE TABLE IF NOT EXISTS agent_events (
    sequence INTEGER PRIMARY KEY AUTOINCREMENT,
    session TEXT NOT NULL REFERENCES sessions(id),
    id TEXT NOT NULL,
    turn TEXT NOT NULL REFERENCES turns(id),
    body BLOB NOT NULL,
    UNIQUE(session,id)
);
CREATE TABLE IF NOT EXISTS conversation_turns (
    session TEXT NOT NULL REFERENCES sessions(id),
    turn TEXT NOT NULL REFERENCES turns(id),
    PRIMARY KEY(session,turn)
);
CREATE TABLE IF NOT EXISTS turn_attachments (
    turn TEXT NOT NULL REFERENCES turns(id),
    attachment TEXT NOT NULL REFERENCES attachments(id),
    position INTEGER NOT NULL,
    PRIMARY KEY(turn,attachment),
    UNIQUE(turn,position)
);
CREATE INDEX IF NOT EXISTS attachment_turns ON turn_attachments(attachment);
CREATE TABLE IF NOT EXISTS conversation_events (
    session TEXT NOT NULL REFERENCES sessions(id),
    sequence INTEGER NOT NULL REFERENCES agent_events(sequence),
    PRIMARY KEY(session,sequence)
);
CREATE TABLE IF NOT EXISTS session_forks (
    session TEXT PRIMARY KEY REFERENCES sessions(id),
    source TEXT NOT NULL REFERENCES sessions(id),
    through_turn TEXT NOT NULL REFERENCES turns(id)
);
CREATE TABLE IF NOT EXISTS agent_queues (
    session TEXT PRIMARY KEY REFERENCES sessions(id),
    revision INTEGER NOT NULL,
    paused INTEGER NOT NULL CHECK(paused IN (0,1))
);
CREATE TABLE IF NOT EXISTS agent_pending (
    turn TEXT PRIMARY KEY REFERENCES turns(id),
    position INTEGER NOT NULL,
    revision INTEGER NOT NULL,
    request TEXT NOT NULL,
    caller BLOB NOT NULL,
    FOREIGN KEY(caller,request) REFERENCES requests(caller,id)
);
CREATE INDEX IF NOT EXISTS agent_events_id ON agent_events(id);
CREATE INDEX IF NOT EXISTS agent_events_turn ON agent_events(turn,sequence);
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
CREATE TABLE IF NOT EXISTS agent_approvals (
    id TEXT PRIMARY KEY,
    turn TEXT NOT NULL REFERENCES turns(id),
    call TEXT NOT NULL,
    body BLOB NOT NULL,
    request TEXT NOT NULL,
    claimed INTEGER NOT NULL CHECK(claimed IN (0,1)),
    UNIQUE(turn,call)
);
CREATE TABLE IF NOT EXISTS agent_questions (
    id TEXT PRIMARY KEY,
    turn TEXT NOT NULL REFERENCES turns(id),
    call TEXT NOT NULL,
    body BLOB NOT NULL,
    UNIQUE(turn,call)
);
CREATE TABLE IF NOT EXISTS file_checkpoints (
    id TEXT PRIMARY KEY,
    turn TEXT NOT NULL REFERENCES turns(id),
    approval TEXT NOT NULL UNIQUE REFERENCES agent_approvals(id),
    body BLOB NOT NULL,
    before_text TEXT,
    after_text TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS file_checkpoints_turn ON file_checkpoints(turn);
CREATE TABLE IF NOT EXISTS agent_state (
    owner TEXT NOT NULL,
    key TEXT NOT NULL,
    body BLOB NOT NULL,
    PRIMARY KEY(owner,key)
);
CREATE TABLE IF NOT EXISTS agent_continuations (
    turn TEXT PRIMARY KEY REFERENCES turns(id),
    body BLOB NOT NULL
);
CREATE TABLE IF NOT EXISTS plugins (
    name TEXT PRIMARY KEY,
    revision INTEGER NOT NULL,
    -- Highest issued configuration revision; retained across removal and collection.
    settings_revision INTEGER NOT NULL DEFAULT 0,
    installed INTEGER NOT NULL CHECK(installed IN (0,1)),
    body BLOB NOT NULL
);
CREATE TABLE IF NOT EXISTS plugin_packages (
    digest TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    body BLOB NOT NULL
);
CREATE TABLE model_catalog_status (
    id INTEGER PRIMARY KEY CHECK(id=1),
    body BLOB NOT NULL
);
CREATE TABLE model_catalog (
    provider TEXT NOT NULL,
    id TEXT NOT NULL,
    body BLOB NOT NULL,
    PRIMARY KEY(provider,id)
);

CREATE TABLE dispatch_handlers (
    package TEXT NOT NULL,
    name TEXT NOT NULL,
    source TEXT NOT NULL,
    topic TEXT NOT NULL,
    body BLOB NOT NULL,
    reference BLOB NOT NULL,
    PRIMARY KEY(package,name)
);
CREATE INDEX dispatch_handlers_source ON dispatch_handlers(source,topic);
CREATE TABLE dispatch_schedules (
    package TEXT NOT NULL,
    id TEXT NOT NULL,
    next_ms INTEGER,
    body BLOB NOT NULL,
    PRIMARY KEY(package,id)
);
CREATE INDEX dispatch_schedules_due ON dispatch_schedules(next_ms);
CREATE TABLE dispatch_events (
    sequence INTEGER PRIMARY KEY AUTOINCREMENT,
    id TEXT NOT NULL UNIQUE,
    body BLOB NOT NULL
);
CREATE TABLE dispatch_jobs (
    sequence INTEGER PRIMARY KEY AUTOINCREMENT,
    id TEXT NOT NULL UNIQUE,
    package TEXT NOT NULL,
    queue TEXT NOT NULL,
    status TEXT NOT NULL,
    body BLOB NOT NULL,
    callback BLOB NOT NULL,
    reference BLOB NOT NULL
);
CREATE INDEX dispatch_jobs_queue ON dispatch_jobs(package,queue,status,sequence);
CREATE TABLE notifications (
    sequence INTEGER PRIMARY KEY AUTOINCREMENT,
    id TEXT NOT NULL UNIQUE,
    body BLOB NOT NULL
);
