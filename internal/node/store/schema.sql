CREATE TABLE IF NOT EXISTS node (
    singleton INTEGER PRIMARY KEY CHECK(singleton=1),
    identity BLOB NOT NULL CHECK(length(identity)=32),
    storage_generation INTEGER NOT NULL DEFAULT 1,
    plugin_defaults INTEGER NOT NULL DEFAULT 0 CHECK(plugin_defaults IN (0,1)),
    tool_defaults INTEGER NOT NULL DEFAULT 0 CHECK(tool_defaults IN (0,1)),
    tool_metadata_names INTEGER NOT NULL DEFAULT 0 CHECK(tool_metadata_names IN (0,1))
);
CREATE TABLE counters (
    namespace TEXT PRIMARY KEY,
    value INTEGER NOT NULL CHECK(typeof(value)='integer' AND value>0)
);
CREATE TABLE IF NOT EXISTS oauth_hosts (
    singleton INTEGER PRIMARY KEY CHECK(singleton=1),
    host_id TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS oauth_registrations (
    provider TEXT PRIMARY KEY,
    client_id TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS requests (
    caller BLOB NOT NULL,
    id TEXT NOT NULL,
    body BLOB NOT NULL,
    status TEXT NOT NULL CHECK(status IN ('admitted','completed','unknown')),
    result TEXT,
    at INTEGER NOT NULL DEFAULT 0,
    scope BLOB,
    PRIMARY KEY(caller,id)
);
CREATE TABLE session_forwards (
 caller BLOB NOT NULL,
 request TEXT NOT NULL,
 body BLOB NOT NULL,
 PRIMARY KEY(caller,request),
 FOREIGN KEY(caller,request) REFERENCES requests(caller,id) ON DELETE CASCADE
);
CREATE TABLE IF NOT EXISTS projects (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    path TEXT NOT NULL UNIQUE,
    appearance BLOB NOT NULL,
    owner TEXT REFERENCES users(id),
    visibility TEXT NOT NULL DEFAULT 'private' CHECK(visibility IN ('private','shared'))
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
    owner TEXT REFERENCES users(id),
    worktree TEXT REFERENCES worktrees(id),
    revision INTEGER NOT NULL,
    history_revision INTEGER NOT NULL DEFAULT 1
);
CREATE TABLE IF NOT EXISTS session_attention (
    session TEXT PRIMARY KEY REFERENCES sessions(id),
    revision INTEGER NOT NULL,
    unread INTEGER NOT NULL CHECK(unread IN (0,1))
);
CREATE TABLE IF NOT EXISTS session_associations (
    session TEXT PRIMARY KEY REFERENCES sessions(id),
    namespace TEXT NOT NULL,
    kind TEXT NOT NULL,
    identity TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS session_association_identity ON session_associations(namespace,kind,identity);
CREATE TABLE IF NOT EXISTS artifacts (
    id TEXT PRIMARY KEY,
    session TEXT NOT NULL REFERENCES sessions(id),
    body BLOB NOT NULL
);
CREATE INDEX IF NOT EXISTS artifact_session ON artifacts(session);
CREATE TABLE IF NOT EXISTS artifact_revisions (
    artifact TEXT NOT NULL REFERENCES artifacts(id) ON DELETE CASCADE,
    revision INTEGER NOT NULL CHECK(revision>0),
    body BLOB NOT NULL,
    digest TEXT NOT NULL,
    warnings BLOB NOT NULL,
    PRIMARY KEY(artifact,revision)
);
CREATE TABLE IF NOT EXISTS worktrees (
    id TEXT PRIMARY KEY,
    project TEXT REFERENCES projects(id),
    owner TEXT REFERENCES users(id),
    path TEXT NOT NULL UNIQUE,
    main INTEGER NOT NULL CHECK(main IN (0,1))
);
CREATE UNIQUE INDEX IF NOT EXISTS worktree_main ON worktrees(project) WHERE main=1;
CREATE TABLE IF NOT EXISTS removed_worktrees (
    worktree TEXT PRIMARY KEY REFERENCES worktrees(id)
);
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
CREATE TABLE telemetry_settings (
    singleton INTEGER PRIMARY KEY CHECK(singleton=1),
    body BLOB NOT NULL
);
CREATE TABLE knowledge_collections (
    id TEXT PRIMARY KEY,
    revision INTEGER NOT NULL CHECK(revision>0),
    generation INTEGER NOT NULL CHECK(generation>0),
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
    roles BLOB,
    instructions BLOB NOT NULL DEFAULT X'5B5D',
    session TEXT NOT NULL,
    revision INTEGER NOT NULL,
    request TEXT NOT NULL,
    caller BLOB NOT NULL,
    user TEXT REFERENCES users(id),
    FOREIGN KEY(session,revision) REFERENCES session_revisions(session,revision),
    FOREIGN KEY(caller,request) REFERENCES requests(caller,id)
);
CREATE INDEX IF NOT EXISTS turns_session ON turns(session);
CREATE TABLE IF NOT EXISTS events (
    cursor INTEGER PRIMARY KEY,
    audience BLOB,
    body BLOB NOT NULL
);
CREATE INDEX IF NOT EXISTS usage_metadata_events ON events(cursor)
WHERE json_extract(body,'$.kind') IN (
    'project_registered','project_changed','project_removed',
    'ssh_changed','ssh_removed','database_changed','database_removed',
    'session_changed','sessions_removed','session_rewound');
CREATE TABLE IF NOT EXISTS agent_delegations (
    turn TEXT PRIMARY KEY REFERENCES turns(id),
    parent_turn TEXT NOT NULL REFERENCES turns(id),
    entry TEXT NOT NULL,
    part INTEGER NOT NULL,
    body BLOB NOT NULL
);
CREATE INDEX IF NOT EXISTS agent_delegations_call ON agent_delegations(parent_turn,entry,part);
CREATE TABLE users (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    administrator INTEGER NOT NULL CHECK(administrator IN (0,1)),
    local INTEGER NOT NULL DEFAULT 0 CHECK(local IN (0,1)),
    state TEXT NOT NULL DEFAULT 'active' CHECK(state IN ('active','merged')),
    merged_into TEXT REFERENCES users(id),
    controller BLOB CHECK(controller IS NULL OR length(controller)=32)
);
CREATE UNIQUE INDEX users_local ON users(local) WHERE local=1;
CREATE TABLE IF NOT EXISTS link_peers (
    identity BLOB PRIMARY KEY CHECK(length(identity)=32),
    direction TEXT NOT NULL DEFAULT 'both' CHECK(direction IN ('controller','host','both')),
    legacy INTEGER NOT NULL DEFAULT 1 CHECK(legacy IN (0,1)),
    user TEXT REFERENCES users(id),
    has_node INTEGER CHECK(has_node IS NULL OR has_node IN (0,1))
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
CREATE TABLE plugin_user_settings (
    user TEXT NOT NULL REFERENCES users(id),
    plugin TEXT NOT NULL,
    revision INTEGER NOT NULL,
    body BLOB NOT NULL,
    PRIMARY KEY(user,plugin,revision)
);
CREATE TABLE plugin_connections (
    id TEXT PRIMARY KEY,
    plugin TEXT NOT NULL,
    connector TEXT NOT NULL,
    revision INTEGER NOT NULL CHECK(revision>0),
    body BLOB NOT NULL,
    user TEXT REFERENCES users(id)
);
CREATE INDEX plugin_connections_owner ON plugin_connections(plugin,connector);
CREATE INDEX plugin_connections_user ON plugin_connections(user,plugin,connector);
CREATE TABLE plugin_connection_captures (
    connection TEXT NOT NULL REFERENCES plugin_connections(id) ON DELETE CASCADE,
    user TEXT NOT NULL REFERENCES users(id),
    PRIMARY KEY(connection,user)
);
CREATE TABLE turn_connections (
    turn TEXT NOT NULL REFERENCES turns(id),
    plugin TEXT NOT NULL,
    connector TEXT NOT NULL,
    connection TEXT NOT NULL,
    revision INTEGER NOT NULL CHECK(revision>0),
    PRIMARY KEY(turn,connection)
);
CREATE INDEX turn_connections_owner ON turn_connections(turn,plugin,connector);
CREATE TABLE plugin_values (
    name TEXT NOT NULL,
    key TEXT NOT NULL,
    revision INTEGER NOT NULL CHECK(revision>0),
    value BLOB,
    index_data BLOB,
    PRIMARY KEY(name,key)
);
CREATE TABLE plugin_space_values (
    space TEXT NOT NULL,
    name TEXT NOT NULL,
    key TEXT NOT NULL,
    revision INTEGER NOT NULL CHECK(revision>0),
    value BLOB,
    index_data BLOB,
    PRIMARY KEY(space,name,key)
);
CREATE TABLE plugin_search_spaces (
    space TEXT NOT NULL,
    name TEXT NOT NULL,
    PRIMARY KEY(space,name)
);
-- Opaque private checkpoints share immutable records across conversation branches.
CREATE TABLE plugin_conversation_records (
    sequence INTEGER PRIMARY KEY,
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
    owner TEXT REFERENCES users(id),
    body BLOB NOT NULL
);
CREATE TABLE IF NOT EXISTS terminal_settings (
    singleton INTEGER PRIMARY KEY CHECK(singleton=1),
    body BLOB NOT NULL
);
CREATE TABLE IF NOT EXISTS sandbox_settings (
    singleton INTEGER PRIMARY KEY CHECK(singleton=1),
    body BLOB NOT NULL
);
CREATE TABLE IF NOT EXISTS search_settings (
    singleton INTEGER PRIMARY KEY CHECK(singleton=1),
    body BLOB NOT NULL
);
CREATE TABLE IF NOT EXISTS providers (
    id TEXT PRIMARY KEY,
    user TEXT REFERENCES users(id),
    shared INTEGER NOT NULL DEFAULT 0 CHECK(shared IN (0,1)),
    body BLOB NOT NULL
);
CREATE TABLE IF NOT EXISTS roles (
    id TEXT PRIMARY KEY,
    user TEXT REFERENCES users(id),
    key TEXT NOT NULL UNIQUE,
    body BLOB NOT NULL
);
CREATE TABLE IF NOT EXISTS teams (
    id TEXT PRIMARY KEY,
    user TEXT REFERENCES users(id),
    key TEXT NOT NULL UNIQUE,
    body BLOB NOT NULL
);
CREATE TABLE IF NOT EXISTS team_settings (
    singleton INTEGER PRIMARY KEY CHECK(singleton=1),
    body BLOB NOT NULL
);
CREATE TABLE IF NOT EXISTS ssh_profiles (
    id TEXT PRIMARY KEY,
    user TEXT REFERENCES users(id),
    body BLOB NOT NULL,
    credential TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS database_profiles (
    id TEXT PRIMARY KEY,
    user TEXT REFERENCES users(id),
    body BLOB NOT NULL,
    password TEXT NOT NULL,
    password_target BLOB
);
CREATE TABLE IF NOT EXISTS agent_runs (
    turn TEXT PRIMARY KEY REFERENCES turns(id),
    ready INTEGER NOT NULL CHECK(ready IN (0,1)),
    body BLOB NOT NULL,
    provider BLOB,
    input BLOB DEFAULT NULL,
    hooks BLOB NOT NULL,
    compaction_percent INTEGER NOT NULL DEFAULT 80 CHECK(compaction_percent BETWEEN 1 AND 99),
    media BLOB DEFAULT NULL
);
CREATE TABLE IF NOT EXISTS agent_events (
    sequence INTEGER PRIMARY KEY,
    session TEXT NOT NULL REFERENCES sessions(id),
    id TEXT NOT NULL,
    turn TEXT NOT NULL REFERENCES turns(id),
    body BLOB NOT NULL,
    UNIQUE(session,id)
);
CREATE TABLE IF NOT EXISTS agent_credential_recoveries (
    sequence INTEGER PRIMARY KEY,
    turn TEXT NOT NULL REFERENCES turns(id),
    request TEXT NOT NULL,
    body BLOB NOT NULL,
    UNIQUE(turn,request)
);
CREATE TABLE IF NOT EXISTS agent_blobs (
    digest TEXT PRIMARY KEY CHECK(length(digest)=64),
    bytes BLOB NOT NULL
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
    paused INTEGER NOT NULL CHECK(paused IN (0,1)),
    pause_reason TEXT
);
CREATE TABLE IF NOT EXISTS agent_pending (
    turn TEXT PRIMARY KEY REFERENCES turns(id),
    position INTEGER NOT NULL,
    revision INTEGER NOT NULL,
    request TEXT NOT NULL,
    caller BLOB NOT NULL,
    recipient TEXT REFERENCES turns(id),
    FOREIGN KEY(caller,request) REFERENCES requests(caller,id)
);
CREATE INDEX IF NOT EXISTS agent_events_id ON agent_events(id);
CREATE INDEX IF NOT EXISTS agent_events_turn ON agent_events(turn,sequence);
CREATE INDEX IF NOT EXISTS agent_visibility ON agent_events(
    json_extract(body,'$.message'),sequence DESC
) WHERE json_extract(body,'$.kind')='operation' AND json_extract(body,'$.operation')='visibility';
CREATE INDEX IF NOT EXISTS agent_usage ON agent_events(
    turn,
    max(0,coalesce(json_extract(body,'$.usage.input'),json_extract(body,'$.summary.usage.input'))),
    max(0,coalesce(json_extract(body,'$.usage.output'),json_extract(body,'$.summary.usage.output'))),
    max(0,coalesce(coalesce(json_extract(body,'$.usage.cache_read'),json_extract(body,'$.summary.usage.cache_read')),0)),
    max(0,coalesce(coalesce(json_extract(body,'$.usage.reasoning'),json_extract(body,'$.summary.usage.reasoning')),0))
) WHERE coalesce(json_type(body,'$.usage'),json_type(body,'$.summary.usage'))='object';
CREATE INDEX IF NOT EXISTS agent_usage_time ON agent_events(
    substr(json_extract(body,'$.created'),1,19),
    sequence,
    turn,
    json_extract(body,'$.created'),
    max(0,coalesce(json_extract(body,'$.usage.input'),json_extract(body,'$.summary.usage.input'))),
    max(0,coalesce(json_extract(body,'$.usage.output'),json_extract(body,'$.summary.usage.output'))),
    max(0,coalesce(coalesce(json_extract(body,'$.usage.cache_read'),json_extract(body,'$.summary.usage.cache_read')),0)),
    max(0,coalesce(coalesce(json_extract(body,'$.usage.reasoning'),json_extract(body,'$.summary.usage.reasoning')),0))
) WHERE coalesce(json_type(body,'$.usage'),json_type(body,'$.summary.usage'))='object';
CREATE TABLE IF NOT EXISTS agent_approvals (
    id TEXT PRIMARY KEY,
    turn TEXT NOT NULL REFERENCES turns(id),
    call TEXT NOT NULL,
    body BLOB NOT NULL,
    binding BLOB NOT NULL,
    prepared BLOB NOT NULL DEFAULT X'6e756c6c',
    request TEXT NOT NULL,
    claimed INTEGER NOT NULL CHECK(claimed IN (0,1)),
    UNIQUE(turn,call)
);
CREATE TABLE IF NOT EXISTS agent_approval_rules (
    session TEXT NOT NULL REFERENCES sessions(id),
    worktree TEXT NOT NULL REFERENCES worktrees(id),
    binding BLOB NOT NULL,
    choice BLOB NOT NULL,
    approval TEXT NOT NULL REFERENCES agent_approvals(id),
    PRIMARY KEY(session,worktree,binding,choice)
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
-- Later files of one change set; the approval's checkpoint row holds the first file.
CREATE TABLE IF NOT EXISTS file_checkpoint_entries (
    id TEXT PRIMARY KEY,
    checkpoint TEXT NOT NULL REFERENCES file_checkpoints(id),
    position INTEGER NOT NULL,
    body BLOB NOT NULL,
    before_text TEXT,
    after_text TEXT NOT NULL,
    UNIQUE(checkpoint,position)
);
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
CREATE TABLE plugin_user_installations (
    user TEXT NOT NULL REFERENCES users(id),
    name TEXT NOT NULL,
    revision INTEGER NOT NULL,
    settings_revision INTEGER NOT NULL DEFAULT 0,
    installed INTEGER NOT NULL CHECK(installed IN (0,1)),
    body BLOB NOT NULL,
    PRIMARY KEY(user,name)
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
CREATE TABLE model_score_status (
    id INTEGER PRIMARY KEY CHECK(id=1),
    body BLOB NOT NULL
);
CREATE TABLE model_scores (
    model TEXT NOT NULL,
    effort TEXT NOT NULL,
    body BLOB NOT NULL,
    PRIMARY KEY(model,effort)
);
CREATE TABLE model_score_aliases (
    alias TEXT PRIMARY KEY,
    model TEXT NOT NULL
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
CREATE TABLE dispatch_user_handlers (
    user TEXT NOT NULL REFERENCES users(id),
    package TEXT NOT NULL,
    name TEXT NOT NULL,
    source TEXT NOT NULL,
    topic TEXT NOT NULL,
    body BLOB NOT NULL,
    reference BLOB NOT NULL,
    PRIMARY KEY(user,package,name)
);
CREATE INDEX dispatch_user_handlers_source ON dispatch_user_handlers(user,source,topic);
CREATE TABLE dispatch_user_schedules (
    user TEXT NOT NULL REFERENCES users(id),
    package TEXT NOT NULL,
    id TEXT NOT NULL,
    next_ms INTEGER,
    body BLOB NOT NULL,
    PRIMARY KEY(user,package,id)
);
CREATE INDEX dispatch_user_schedules_due ON dispatch_user_schedules(user,next_ms);
CREATE TABLE dispatch_events (
    user TEXT REFERENCES users(id),
    sequence INTEGER PRIMARY KEY,
    id TEXT NOT NULL UNIQUE,
    body BLOB NOT NULL
);
CREATE TABLE dispatch_jobs (
    user TEXT REFERENCES users(id),
    storage_user TEXT REFERENCES users(id),
    sequence INTEGER PRIMARY KEY,
    id TEXT NOT NULL UNIQUE,
    package TEXT NOT NULL,
    queue TEXT NOT NULL,
    status TEXT NOT NULL,
    body BLOB NOT NULL,
    callback BLOB NOT NULL,
    reference BLOB NOT NULL
);
CREATE INDEX dispatch_jobs_queue ON dispatch_jobs(package,queue,status,sequence);
CREATE INDEX dispatch_jobs_user ON dispatch_jobs(user,status,sequence);
CREATE TABLE notifications (
    sequence INTEGER PRIMARY KEY,
    id TEXT NOT NULL UNIQUE,
    owner TEXT REFERENCES users(id),
    body BLOB NOT NULL
);

CREATE TABLE code_index_settings (singleton INTEGER PRIMARY KEY CHECK(singleton=1),body BLOB NOT NULL);
CREATE TABLE python_settings (singleton INTEGER PRIMARY KEY CHECK(singleton=1),body BLOB NOT NULL);

CREATE TABLE hook_settings (
    worktree TEXT PRIMARY KEY REFERENCES worktrees(id) ON DELETE CASCADE,
    body BLOB NOT NULL
);
CREATE TABLE hook_executions (
    turn TEXT NOT NULL REFERENCES turns(id) ON DELETE CASCADE,
    phase TEXT NOT NULL CHECK(phase IN ('before_tool','after_tool','turn_end')),
    ordinal INTEGER NOT NULL CHECK(ordinal>=0),
    result BLOB,
    PRIMARY KEY(turn,phase,ordinal)
);

CREATE TABLE backup_settings (singleton INTEGER PRIMARY KEY CHECK(singleton=1),body BLOB NOT NULL);

CREATE TABLE IF NOT EXISTS agent_background (
    id TEXT PRIMARY KEY,
    session TEXT NOT NULL REFERENCES sessions(id),
    turn TEXT NOT NULL REFERENCES turns(id),
    kind TEXT NOT NULL CHECK(kind IN ('agent','command')),
    source TEXT NOT NULL,
    state TEXT NOT NULL CHECK(state IN ('running','pending','delivered','consumed','dismissed')),
    delivery TEXT REFERENCES turns(id),
    sequence INTEGER NOT NULL,
    body BLOB NOT NULL
);
CREATE INDEX IF NOT EXISTS agent_background_session ON agent_background(session,state,sequence);
CREATE INDEX IF NOT EXISTS agent_background_sequence ON agent_background(sequence);
CREATE TABLE IF NOT EXISTS background_settings (
    singleton INTEGER PRIMARY KEY CHECK(singleton=1),
    body BLOB NOT NULL
);

CREATE INDEX IF NOT EXISTS agent_background_source ON agent_background(kind,source,state);

CREATE TABLE IF NOT EXISTS network_settings (
    singleton INTEGER PRIMARY KEY CHECK(singleton=1),
    body BLOB NOT NULL
);

-- Startup and recovery visit current state without decoding historical payloads.
CREATE INDEX IF NOT EXISTS requests_status ON requests(status);
CREATE INDEX IF NOT EXISTS events_kind ON events(json_extract(body,'$.kind'));
CREATE INDEX IF NOT EXISTS agent_runs_status ON agent_runs(json_extract(body,'$.status'));
CREATE INDEX IF NOT EXISTS agent_approvals_state ON agent_approvals(json_extract(body,'$.state'));
CREATE INDEX IF NOT EXISTS agent_questions_state ON agent_questions(json_extract(body,'$.state.kind'));
CREATE INDEX IF NOT EXISTS agent_background_state ON agent_background(state);
CREATE INDEX IF NOT EXISTS dispatch_jobs_status ON dispatch_jobs(status);

CREATE TABLE channel_presets (singleton INTEGER PRIMARY KEY CHECK(singleton=1),body BLOB NOT NULL);

CREATE TABLE local_models (
    id TEXT PRIMARY KEY,
    kind TEXT NOT NULL,
    digest TEXT NOT NULL UNIQUE,
    body BLOB NOT NULL
);
CREATE TABLE local_model_settings (
    singleton INTEGER PRIMARY KEY CHECK(singleton=1),
    body BLOB NOT NULL
);

CREATE TABLE IF NOT EXISTS power_settings (singleton INTEGER PRIMARY KEY CHECK(singleton=1), body BLOB NOT NULL);

CREATE TABLE compaction_settings (singleton INTEGER PRIMARY KEY CHECK(singleton=1), body BLOB NOT NULL);

CREATE TABLE removed_resource_audiences (kind TEXT NOT NULL,id TEXT NOT NULL,audience BLOB NOT NULL,PRIMARY KEY(kind,id));
CREATE TABLE IF NOT EXISTS user_defaults (
    user TEXT PRIMARY KEY REFERENCES users(id),
    revision INTEGER NOT NULL,
    config BLOB NOT NULL
);
CREATE TABLE IF NOT EXISTS user_team_settings (
    user TEXT PRIMARY KEY REFERENCES users(id),
    body BLOB NOT NULL
);

CREATE TABLE user_personal_settings (user TEXT NOT NULL REFERENCES users(id),kind TEXT NOT NULL,body BLOB NOT NULL,PRIMARY KEY(user,kind));
CREATE TABLE identity_memberships (
    device BLOB PRIMARY KEY CHECK(length(device)=32),
    user TEXT NOT NULL REFERENCES users(id),
    sponsor BLOB NOT NULL CHECK(length(sponsor)=32),
    generation INTEGER NOT NULL CHECK(generation>0),
    active INTEGER NOT NULL CHECK(active IN (0,1))
);
CREATE TABLE identity_sponsorships (
    sponsor BLOB NOT NULL CHECK(length(sponsor)=32),
    request TEXT NOT NULL,
    device BLOB NOT NULL CHECK(length(device)=32),
    user TEXT NOT NULL REFERENCES users(id),
    address TEXT NOT NULL,
    state TEXT NOT NULL CHECK(state IN ('pending','accepted','cancelled')),
    PRIMARY KEY(sponsor,request)
);
CREATE INDEX identity_sponsorships_device ON identity_sponsorships(device,state);
CREATE TABLE identity_directory (
    user TEXT PRIMARY KEY REFERENCES users(id),
    revision INTEGER NOT NULL CHECK(revision>0),
    hosts BLOB NOT NULL
);
CREATE TABLE identity_deliveries (
    request TEXT PRIMARY KEY,
    user TEXT NOT NULL REFERENCES users(id),
    host BLOB NOT NULL CHECK(length(host)=32),
    address TEXT NOT NULL,
    device BLOB NOT NULL CHECK(length(device)=32),
    generation INTEGER NOT NULL CHECK(generation>0),
    kind TEXT NOT NULL CHECK(kind IN ('sponsor','revoke')),
    body BLOB NOT NULL,
    state TEXT NOT NULL CHECK(state IN ('pending','unknown','completed','cancelled','blocked')),
    fault BLOB,
    UNIQUE(host,device,generation,kind)
);
CREATE INDEX identity_deliveries_state ON identity_deliveries(state,host);
CREATE TABLE identity_turn_origins (
    turn TEXT PRIMARY KEY REFERENCES turns(id) ON DELETE CASCADE,
    user TEXT NOT NULL REFERENCES users(id)
);
CREATE TABLE identity_storage_origin (
    singleton INTEGER PRIMARY KEY CHECK(singleton=1),
    user TEXT NOT NULL REFERENCES users(id)
);
CREATE TABLE identity_merge_conflicts (
    source TEXT NOT NULL REFERENCES users(id),
    target TEXT NOT NULL REFERENCES users(id),
    kind TEXT NOT NULL,
    key TEXT NOT NULL,
    body BLOB NOT NULL,
    PRIMARY KEY(source,kind,key)
);

CREATE TABLE identity_peer_origins (
    peer BLOB PRIMARY KEY CHECK(length(peer)=32),
    name TEXT NOT NULL DEFAULT '',
    direct INTEGER NOT NULL CHECK(direct IN (0,1)),
    direct_direction TEXT NOT NULL DEFAULT '' CHECK(direct_direction IN ('','controller','host','both')),
    my_device INTEGER NOT NULL CHECK(my_device IN (0,1)),
    removed INTEGER NOT NULL CHECK(removed IN (0,1)),
    sources BLOB NOT NULL,
    directory_revision INTEGER NOT NULL DEFAULT 0 CHECK(directory_revision>=0)
);

CREATE TABLE identity_sponsor_epochs (
    sponsor BLOB NOT NULL CHECK(length(sponsor)=32),
    device BLOB NOT NULL CHECK(length(device)=32),
    generation INTEGER NOT NULL CHECK(generation>0),
    revoked INTEGER NOT NULL CHECK(revoked IN (0,1)),
    PRIMARY KEY(sponsor,device)
);

CREATE TABLE identity_host_removals (
    request TEXT PRIMARY KEY,
    host BLOB NOT NULL CHECK(length(host)=32),
    body BLOB NOT NULL,
    state TEXT NOT NULL CHECK(state IN ('pending','unknown','completed','failed')),
    fault BLOB,
    active INTEGER NOT NULL CHECK(active IN (0,1))
);
CREATE UNIQUE INDEX identity_host_removals_active ON identity_host_removals(host) WHERE active=1;
-- Directory metadata only; projects and their contents remain on their Node.
CREATE TABLE resource_catalog_sources (
    user TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    source BLOB NOT NULL REFERENCES link_peers(identity) ON DELETE CASCADE CHECK(length(source)=32),
    body BLOB NOT NULL,
    PRIMARY KEY(user,source)
);
CREATE TABLE IF NOT EXISTS configuration_follow_sources (
 user TEXT PRIMARY KEY REFERENCES users(id),
 source BLOB CHECK(source IS NULL OR length(source)=32),
 revision INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS configuration_follow_targets (
 user TEXT NOT NULL REFERENCES users(id),
 host BLOB NOT NULL CHECK(length(host)=32),
 revision INTEGER NOT NULL,
 configuration INTEGER NOT NULL CHECK(configuration IN (0,1)),
 secrets INTEGER NOT NULL CHECK(secrets IN (0,1)),
 failure BLOB,
 PRIMARY KEY(user,host)
);
CREATE TABLE IF NOT EXISTS configuration_follow_records (
 user TEXT NOT NULL REFERENCES users(id),
 category TEXT NOT NULL,
 id TEXT NOT NULL,
 local_id TEXT NOT NULL DEFAULT '',
 source BLOB NOT NULL CHECK(length(source)=32),
 revision INTEGER NOT NULL,
 digest TEXT NOT NULL,
 state TEXT NOT NULL CHECK(state IN ('applied','pending','conflict','authorization')),
 materialized INTEGER NOT NULL DEFAULT 0 CHECK(materialized IN (0,1)),
 message TEXT NOT NULL,
 body BLOB,
 PRIMARY KEY(user,category,id)
);
CREATE TABLE IF NOT EXISTS configuration_follow_versions (
 user TEXT NOT NULL REFERENCES users(id),
 category TEXT NOT NULL,
 source BLOB NOT NULL CHECK(length(source)=32),
 revision INTEGER NOT NULL,
 digest TEXT NOT NULL,
 PRIMARY KEY(user,category,source)
);
CREATE TABLE configuration_export_revisions (
 user TEXT NOT NULL REFERENCES users(id),
 category TEXT NOT NULL,
 id TEXT NOT NULL,
 revision INTEGER NOT NULL CHECK(revision>0),
 digest TEXT NOT NULL,
 PRIMARY KEY(user,category,id)
);
CREATE TABLE IF NOT EXISTS configuration_follow_deliveries (
 user TEXT NOT NULL REFERENCES users(id),
 host BLOB NOT NULL CHECK(length(host)=32),
 category TEXT NOT NULL,
 request BLOB NOT NULL,
 PRIMARY KEY(user,host,category)
);
CREATE TABLE IF NOT EXISTS configuration_follow_mcp_overrides (
 user TEXT NOT NULL REFERENCES users(id),
 plugin TEXT NOT NULL,
 server TEXT NOT NULL,
 PRIMARY KEY(user,plugin,server)
);
CREATE TABLE IF NOT EXISTS configuration_follow_packages (
 digest TEXT PRIMARY KEY,
 name TEXT NOT NULL,
 body BLOB NOT NULL,
 mcp BLOB NOT NULL
);

CREATE TABLE configuration_cache_history (
 user TEXT NOT NULL REFERENCES users(id),
 category TEXT NOT NULL,
 id TEXT NOT NULL,
 revision INTEGER NOT NULL CHECK(revision>0),
 digest TEXT NOT NULL,
 body BLOB NOT NULL,
 secret TEXT,
 expires_at_ms INTEGER,
 PRIMARY KEY(user,category,id,revision)
);
CREATE TABLE configuration_cache (
 user TEXT NOT NULL REFERENCES users(id),
 category TEXT NOT NULL,
 id TEXT NOT NULL,
 revision INTEGER NOT NULL CHECK(revision>0),
 digest TEXT NOT NULL,
 body BLOB NOT NULL,
 secret TEXT,
 expires_at_ms INTEGER,
 PRIMARY KEY(user,category,id)
);

CREATE TABLE personal_kv_clock (
 user TEXT PRIMARY KEY REFERENCES users(id),
 revision INTEGER NOT NULL DEFAULT 0 CHECK(revision>=0),
 sequence INTEGER NOT NULL DEFAULT 0 CHECK(sequence>=0)
);
CREATE TABLE personal_kv_base (
 user TEXT NOT NULL REFERENCES users(id),
 plugin TEXT NOT NULL,
 key TEXT NOT NULL,
 revision INTEGER NOT NULL CHECK(revision>=0),
 value BLOB,
 index_data BLOB,
 PRIMARY KEY(user,plugin,key)
);
CREATE TABLE personal_kv_outbox (
 user TEXT NOT NULL REFERENCES users(id),
 request TEXT NOT NULL,
 sequence INTEGER NOT NULL CHECK(sequence>0),
 digest TEXT NOT NULL,
 body BLOB NOT NULL,
 PRIMARY KEY(user,request),
 UNIQUE(user,sequence)
);
CREATE TABLE personal_kv_receipts (
 user TEXT NOT NULL REFERENCES users(id),
 executor BLOB NOT NULL CHECK(length(executor)=32),
 request TEXT NOT NULL,
 sequence INTEGER NOT NULL CHECK(sequence>0),
 digest TEXT NOT NULL,
 body BLOB NOT NULL,
 PRIMARY KEY(user,executor,request),
 UNIQUE(user,executor,sequence)
);
CREATE TABLE personal_kv_snapshot (
 user TEXT PRIMARY KEY REFERENCES users(id),
 generation INTEGER NOT NULL CHECK(generation>=0),
 cursor BLOB
);
CREATE TABLE personal_kv_snapshot_records (
 user TEXT NOT NULL REFERENCES users(id),
 plugin TEXT NOT NULL,
 key TEXT NOT NULL,
 body BLOB NOT NULL,
 PRIMARY KEY(user,plugin,key)
);

CREATE TABLE ssh_local_trust (
 user TEXT NOT NULL REFERENCES users(id),
 profile TEXT NOT NULL,
 host TEXT NOT NULL,
 port INTEGER NOT NULL,
 key BLOB NOT NULL,
 PRIMARY KEY(user,profile,host,port)
);

CREATE TABLE configuration_snapshots (
 user TEXT NOT NULL REFERENCES users(id),
 root TEXT NOT NULL,
 source BLOB NOT NULL CHECK(length(source)=32),
 category TEXT NOT NULL,
 revision INTEGER NOT NULL CHECK(revision>0),
 bytes INTEGER NOT NULL CHECK(bytes>0),
 digest TEXT NOT NULL,
 next_page INTEGER NOT NULL DEFAULT 0 CHECK(next_page>=0),
 state TEXT NOT NULL DEFAULT 'receiving' CHECK(state IN ('receiving','complete','superseded')),
 PRIMARY KEY(user,root)
);
CREATE INDEX configuration_snapshots_category ON configuration_snapshots(user,category,source);
CREATE TABLE configuration_snapshot_pages (
 user TEXT NOT NULL,
 root TEXT NOT NULL,
 page INTEGER NOT NULL CHECK(page>=0),
 digest TEXT NOT NULL,
 data BLOB,
 PRIMARY KEY(user,root,page),
 FOREIGN KEY(user,root) REFERENCES configuration_snapshots(user,root) ON DELETE CASCADE
);
