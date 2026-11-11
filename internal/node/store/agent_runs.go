package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"time"

	"github.com/sailry/sailry-harness/internal/node/agent/operations"
	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// captureTurnPlugins names the packages a new turn freezes: the enabled
// compatible inventory, or the bundled packages before the first inventory.
func captureTurnPlugins(ctx context.Context, db *sql.Conn) ([]protocol.PluginReference, error) {
	catalog, err := operations.Bundled()
	if err != nil {
		return nil, err
	}
	references := append([]protocol.PluginReference(nil), catalog.References...)
	var initialized bool
	if err := db.QueryRowContext(ctx, "SELECT plugin_defaults FROM node WHERE singleton=1").Scan(&initialized); err != nil {
		return nil, err
	}
	if initialized {
		infos, err := ReadPluginInfos(ctx, db)
		if err != nil {
			return nil, err
		}
		references = []protocol.PluginReference{}
		for _, info := range infos {
			if info.Summary.Enabled && info.Summary.Compatibility.Kind == "compatible" {
				references = append(references, protocol.PluginReference{Name: info.Summary.Name, Digest: info.Summary.Digest, SettingsRevision: info.Summary.SettingsRevision})
			}
		}
	}
	return references, nil
}

func ReadAgentRun(ctx context.Context, db *sql.Conn, turn protocol.TurnID) (protocol.ConversationRun, error) {
	v, e := readBody[protocol.ConversationRun](ctx, db, "SELECT body FROM agent_runs WHERE turn=?1", turn.String())
	if e != nil {
		return protocol.ConversationRun{}, e
	}
	if v == nil {
		return protocol.ConversationRun{}, fault(protocol.ErrorCodeNotFound, "turn does not exist")
	}
	return *v, nil
}
func putAgentRun(ctx context.Context, db *sql.Conn, run protocol.ConversationRun) error {
	run.Input = nil
	body, err := encode(run)
	if err != nil {
		return err
	}
	_, err = db.ExecContext(ctx, "UPDATE agent_runs SET body=?2 WHERE turn=?1", run.Turn.String(), body)
	return err
}
func visibleAgentRun(ctx context.Context, db *sql.Conn, session protocol.SessionID, turn protocol.TurnID) (protocol.ConversationRun, error) {
	run, err := ReadAgentRun(ctx, db, turn)
	if err != nil {
		return run, err
	}
	var visible bool
	if err = db.QueryRowContext(ctx, "SELECT EXISTS(SELECT 1 FROM conversation_turns WHERE session=?1 AND turn=?2)", session.String(), turn.String()).Scan(&visible); err != nil {
		return run, err
	}
	if !visible {
		return run, fault(protocol.ErrorCodeWrongTarget, "turn is not in this conversation")
	}
	if run.Session != session {
		origin := run.Session
		run.Origin = &origin
		run.Session = session
	}
	return run, nil
}
func readTurnCapture(ctx context.Context, db *sql.Conn, turn protocol.TurnID) (protocol.QueuedTurn, error) {
	value := protocol.DefaultQueuedTurn()
	value.ID = turn
	var session, request, kind string
	var config, roles, plugins []byte
	err := db.QueryRowContext(ctx, "SELECT t.session,t.request,t.revision,r.config,t.kind,t.roles,t.plugins FROM turns t JOIN session_revisions r ON r.session=t.session AND r.revision=t.revision WHERE t.id=?1", turn.String()).Scan(&session, &request, &value.Revision, &config, &kind, &roles, &plugins)
	if errors.Is(err, sql.ErrNoRows) {
		return value, fault(protocol.ErrorCodeNotFound, "turn capture does not exist")
	}
	if err != nil {
		return value, err
	}
	if value.Session, err = protocol.ParseSessionID(session); err != nil {
		return value, err
	}
	if value.Request, err = protocol.ParseRequestID(request); err != nil {
		return value, err
	}
	value.Kind = protocol.ConversationRunKind(kind)
	if err = json.Unmarshal(config, &value.Config); err != nil {
		return value, err
	}
	if len(roles) > 0 {
		if err = json.Unmarshal(roles, &value.Roles); err != nil {
			return value, err
		}
	}
	if err = json.Unmarshal(plugins, &value.Plugins); err != nil {
		return value, err
	}
	return value, nil
}
func admitAgentTurn(ctx context.Context, db *sql.Conn, node, caller protocol.NodeID, request protocol.Request, session protocol.Session, kind protocol.ConversationRunKind, ready bool) (protocol.QueuedTurn, error) {
	value := protocol.DefaultQueuedTurn()
	value.ID = protocol.NewTurnID()
	value.Session = session.ID
	value.Request = request.ID
	value.Revision = session.Revision
	value.Config = session.Config
	value.Roles = session.Roles
	if err := RefreshTurnRoles(ctx, db, &value.Roles); err != nil {
		return value, err
	}
	value.Kind = kind
	var err error
	if value.Plugins, err = captureTurnPlugins(ctx, db); err != nil {
		return value, err
	}
	if session.Delegation != nil {
		return value, fault(protocol.ErrorCodePermissionDenied, "child conversation is read-only")
	}
	if err := validateConfig(ctx, db, node, session.Config); err != nil {
		return value, err
	}
	if session.Config.Assistant != nil {
		// An assistant turn freezes only its owner and the packages it imports.
		packages, err := assistantPackages(ctx, db, *session.Config.Assistant)
		if err != nil {
			return value, err
		}
		value.Plugins = []protocol.PluginReference{}
		for _, info := range packages {
			value.Plugins = append(value.Plugins, plugins.PackageReference(info))
		}
	}
	var count uint64
	if err := db.QueryRowContext(ctx, "SELECT count(*) FROM agent_pending p JOIN turns t ON t.id=p.turn WHERE t.session=?1", session.ID.String()).Scan(&count); err != nil {
		return value, err
	}
	if count >= 256 {
		return value, fault(protocol.ErrorCodeBusy, "session queue is full")
	}
	config, err := readBody[providerConfiguration](ctx, db, "SELECT body FROM providers WHERE id=?1", session.Config.Provider.String())
	if err != nil {
		return value, err
	}
	if config == nil || !config.Listed || config.Provider == nil || !config.Provider.Enabled {
		return value, fault(protocol.ErrorCodeNotConfigured, "model provider is unavailable")
	}
	var selected bool
	for _, model := range config.Provider.Models {
		if model.ID == session.Config.Model {
			selected = true
			break
		}
	}
	if !selected {
		return value, invalid("selected model is not configured on provider")
	}
	seq, err := NextCounter(ctx, db, CounterTurns)
	if err != nil {
		return value, err
	}
	plugins, err := encode(value.Plugins)
	if err != nil {
		return value, err
	}
	roles, err := encode(value.Roles)
	if err != nil {
		return value, err
	}
	if _, err = db.ExecContext(ctx, "INSERT INTO turns(id,session,revision,request,caller,kind,plugins,roles,rowid) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)", value.ID.String(), session.ID.String(), session.Revision, request.ID.String(), caller[:], string(kind), plugins, roles, seq); err != nil {
		return value, err
	}
	run := protocol.DefaultConversationRun()
	run.Turn = value.ID
	run.Session = value.Session
	run.Worktree = session.Worktree
	run.Kind = kind
	run.Sequence = seq
	run.Revision = session.Revision
	run.Status = protocol.ConversationStatusQueued
	body, err := encode(run)
	if err != nil {
		return value, err
	}
	captured, err := CaptureProvider(ctx, db, *config.Provider)
	if err != nil {
		return value, err
	}
	provider, err := encode(captured)
	if err != nil {
		return value, err
	}
	settings, err := ReadCompaction(ctx, db)
	if err != nil {
		return value, err
	}
	frozenHooks, err := ReadHooks(ctx, db, session.Worktree)
	if err != nil {
		return value, err
	}
	hooks, err := encode(frozenHooks)
	if err != nil {
		return value, err
	}
	if _, err = db.ExecContext(ctx, "INSERT INTO agent_runs(turn,ready,body,provider,hooks,compaction_percent) VALUES(?1,?2,?3,?4,?5,?6)", value.ID.String(), ready, body, provider, hooks, settings.Percent); err != nil {
		return value, err
	}
	if _, err = db.ExecContext(ctx, "INSERT INTO conversation_turns(session,turn) VALUES(?1,?2)", session.ID.String(), value.ID.String()); err != nil {
		return value, err
	}
	if _, err = db.ExecContext(ctx, "INSERT INTO agent_pending(turn,position,revision,request,caller) SELECT ?1,coalesce(max(p.position),0)+1,1,?2,?3 FROM agent_pending p JOIN turns t ON t.id=p.turn WHERE t.session=?4", value.ID.String(), request.ID.String(), caller[:], session.ID.String()); err != nil {
		return value, err
	}
	if err = bumpQueue(ctx, db, session.ID); err != nil {
		return value, err
	}
	if ready {
		_, err = db.ExecContext(ctx, "UPDATE agent_queues SET paused=0 WHERE session=?1", session.ID.String())
	}
	return value, err
}
func ClaimNext(ctx context.Context, db *sql.Conn, now time.Time) (*AgentInvocation, []protocol.Event, error) {
	events := []protocol.Event{}
	for {
		var id string
		err := db.QueryRowContext(ctx, "SELECT a.turn FROM agent_runs a JOIN agent_pending p ON p.turn=a.turn JOIN turns t ON t.id=a.turn JOIN agent_queues q ON q.session=t.session WHERE a.ready=1 AND q.paused=0 AND p.recipient IS NULL AND NOT EXISTS(SELECT 1 FROM agent_delegations d WHERE d.turn=a.turn) AND json_extract(a.body,'$.status')='queued' AND NOT EXISTS(SELECT 1 FROM agent_runs active JOIN turns other ON other.id=active.turn WHERE other.session=t.session AND json_extract(active.body,'$.status') IN ('running','stopping')) ORDER BY p.position,t.rowid LIMIT 1").Scan(&id)
		if errors.Is(err, sql.ErrNoRows) {
			return nil, events, nil
		}
		if err != nil {
			return nil, events, err
		}
		turn, err := protocol.ParseTurnID(id)
		if err != nil {
			return nil, events, err
		}
		inv, changed, err := claimAgentInvocation(ctx, db, turn, now)
		events = append(events, changed...)
		if err != nil || inv != nil || len(changed) == 0 {
			return inv, events, err
		}
	}
}
func claimAgentInvocation(ctx context.Context, db *sql.Conn, turn protocol.TurnID, now time.Time) (*AgentInvocation, []protocol.Event, error) {
	initial, err := ReadAgentRun(ctx, db, turn)
	if err != nil {
		return nil, nil, err
	}
	unavailable, err := checkAutomaticInput(ctx, db, initial)
	if err != nil {
		return nil, nil, err
	}
	if unavailable != nil {
		events, err := cancelAutomaticInput(ctx, db, initial, unavailable, now)
		return nil, events, err
	}
	queued, err := queuedInvocationInput(ctx, db, turn)
	if err != nil {
		return nil, nil, err
	}
	run, err := ReadAgentRun(ctx, db, turn)
	if err != nil {
		return nil, nil, err
	}
	session, err := ReadSession(ctx, db, run.Session)
	if err != nil {
		return nil, nil, err
	}
	session.Config = queued.Turn.Config
	session.Revision = queued.Turn.Revision
	session.Worktree = run.Worktree
	session.Roles = queued.Turn.Roles
	inv := &AgentInvocation{Session: session, Run: run, Turn: queued.Turn, Message: queued.Message, Instructions: []AgentInstruction{}}
	if err = captureAssistant(ctx, db, inv); err != nil {
		return nil, nil, err
	}
	catalog, err := operations.Bundled()
	if err != nil {
		return nil, nil, err
	}
	var provider, instructions, caller, hookSettings []byte
	var path string
	var project sql.NullString
	var main bool
	if err = db.QueryRowContext(ctx, "SELECT a.hooks,a.provider,a.compaction_percent,t.instructions,t.caller,w.path,w.project,w.main FROM agent_runs a JOIN turns t ON t.id=a.turn JOIN worktrees w ON w.id=?2 WHERE a.turn=?1", turn.String(), run.Worktree.String()).Scan(&hookSettings, &provider, &inv.CompactionPercent, &instructions, &caller, &path, &project, &main); err != nil {
		return nil, nil, err
	}
	if err = json.Unmarshal(hookSettings, &inv.Hooks); err != nil {
		return nil, nil, err
	}
	if err = json.Unmarshal(provider, &inv.Provider); err != nil {
		return nil, nil, err
	}
	for _, reference := range inv.Turn.Plugins {
		info, err := ReadPluginVersion(ctx, db, reference)
		if err == nil {
			if info.Extension != nil && info.Extension.Instructions != nil {
				tools := []string{}
				for _, tool := range info.Extension.Tools {
					tools = append(tools, operations.Alias(reference.Name, tool.Name))
				}
				inv.Instructions = append(inv.Instructions, AgentInstruction{Text: *info.Extension.Instructions, AnyTools: tools})
			}
			continue
		}
		var initialized bool
		if err := db.QueryRowContext(ctx, "SELECT plugin_defaults FROM node WHERE singleton=1").Scan(&initialized); err != nil {
			return nil, nil, err
		}
		if initialized {
			return nil, nil, err
		}
		for _, instruction := range catalog.Instructions {
			if instruction.Owner.Name == reference.Name && instruction.Owner.Digest == reference.Digest {
				inv.Instructions = append(inv.Instructions, AgentInstruction{Text: instruction.Text, AnyTools: instruction.AnyTools})
			}
		}
	}
	var userInstructions []AgentInstruction
	if err = json.Unmarshal(instructions, &userInstructions); err != nil {
		return nil, nil, err
	}
	inv.Instructions = append(inv.Instructions, userInstructions...)
	if len(caller) != 32 {
		return nil, nil, invalid("invalid caller identity")
	}
	copy(inv.Caller[:], caller)
	inv.Worktree = protocol.Worktree{ID: run.Worktree, Path: path, Main: main}
	if project.Valid {
		v, e := protocol.ParseProjectID(project.String)
		if e != nil {
			return nil, nil, e
		}
		inv.Worktree.Project = &v
	}
	ref := session.Config.Credential
	if ref == nil && inv.Provider != nil {
		ref = inv.Provider.Credential
	}
	if ref != nil {
		stored, e := readBody[providerConfiguration](ctx, db, "SELECT body FROM providers WHERE id=?1", session.Config.Provider.String())
		if e != nil {
			return nil, nil, e
		}
		if stored == nil {
			inv.AuthorizationFailure = &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "provider authorization is unavailable"}
		} else {
			saved, exists := stored.Authorizations[ref.ID]
			if !exists || saved.Key == nil {
				inv.AuthorizationFailure = &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "provider authorization is revoked"}
			} else if saved.ExpiresAtMs != nil && *saved.ExpiresAtMs <= uint64(now.UnixMilli()) {
				inv.AuthorizationFailure = &protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "provider authorization expired"}
			} else {
				inv.Secret = saved.Key
			}
		}
	}
	run.Status = protocol.ConversationStatusRunning
	ms := now.UnixMilli()
	run.StartedMs = &ms
	inv.Run = run
	input, err := encode(inv.Message)
	if err != nil {
		return nil, nil, err
	}
	if _, err = db.ExecContext(ctx, "UPDATE agent_runs SET input=coalesce(input,?2) WHERE turn=?1", turn.String(), input); err != nil {
		return nil, nil, err
	}
	if err = putAgentRun(ctx, db, run); err != nil {
		return nil, nil, err
	}
	if run.Kind == protocol.ConversationRunKindCompaction {
		_, err = db.ExecContext(ctx, "DELETE FROM agent_pending WHERE turn=?1", turn.String())
	}
	if err != nil {
		return nil, nil, err
	}
	if err = bumpQueue(ctx, db, run.Session); err != nil {
		return nil, nil, err
	}
	events, err := activityEvent(ctx, db, run.Session)
	return inv, events, err
}
func FinishTurn(ctx context.Context, db *sql.Conn, turn protocol.TurnID, status protocol.ConversationStatus, failure *protocol.Fault, now time.Time) (protocol.ConversationRun, []protocol.Event, error) {
	run, err := ReadAgentRun(ctx, db, turn)
	if err != nil {
		return run, nil, err
	}
	if run.Status == protocol.ConversationStatusStopping && status == protocol.ConversationStatusCompleted {
		status = protocol.ConversationStatusInterrupted
	}
	run.Status = status
	run.Error = failure
	ms := now.UnixMilli()
	run.FinishedMs = &ms
	if _, err = db.ExecContext(ctx, "UPDATE agent_runs SET ready=0 WHERE turn=?1", turn.String()); err != nil {
		return run, nil, err
	}
	if err = putAgentRun(ctx, db, run); err != nil {
		return run, nil, err
	}
	if err = closeInteractions(ctx, db, turn, "interrupted"); err != nil {
		return run, nil, err
	}
	if failure != nil || status == protocol.ConversationStatusFailed || status == protocol.ConversationStatusInterrupted || status == protocol.ConversationStatusCancelled {
		if err = setQueuePaused(ctx, db, run.Session, true); err != nil {
			return run, nil, err
		}
	}
	if _, err = db.ExecContext(ctx, "INSERT INTO session_attention(session,revision,unread) VALUES(?1,1,1) ON CONFLICT(session) DO UPDATE SET revision=revision+1,unread=1", run.Session.String()); err != nil {
		return run, nil, err
	}
	events, err := activityEvent(ctx, db, run.Session)
	if err != nil {
		return run, nil, err
	}
	origin, err := readDelegation(ctx, db, turn)
	if err != nil {
		return run, nil, err
	}
	if origin != nil {
		queue, queueErr := ReadQueue(ctx, db, run.Session)
		if queueErr != nil {
			return run, nil, queueErr
		}
		if queue.Paused {
			err = retainChildGuidance(ctx, db, run)
		}
		if err != nil {
			return run, nil, err
		}
	}
	changed, err := FinishChildBackground(ctx, db, run, now)
	if err != nil {
		return run, nil, err
	}
	events = append(events, changed...)
	changed, err = ReleaseBackground(ctx, db, turn, run.Notice)
	if err != nil {
		return run, nil, err
	}
	events = append(events, changed...)
	owner := run.Session
	if origin != nil {
		owner = origin.Session
		changed, err = activityEvent(ctx, db, owner)
		if err != nil {
			return run, nil, err
		}
		events = append(events, changed...)
	}
	var identity []byte
	if err = db.QueryRowContext(ctx, "SELECT identity FROM node WHERE singleton=1").Scan(&identity); err != nil {
		return run, nil, err
	}
	if len(identity) != 32 {
		return run, nil, invalid("invalid Node identity")
	}
	var node protocol.NodeID
	copy(node[:], identity)
	_, changed, err = ScheduleBackground(ctx, db, node, owner, now)
	events = append(events, changed...)
	return run, events, err
}
func RecoverRuns(ctx context.Context, db *sql.Conn, now time.Time) ([]protocol.Event, error) {
	runs, err := readBodies[protocol.ConversationRun](ctx, db, "SELECT body FROM agent_runs WHERE json_extract(body,'$.status') IN ('running','stopping')")
	if err != nil {
		return nil, err
	}
	events := []protocol.Event{}
	for _, run := range runs {
		failure := &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "execution was interrupted; model and tool calls were not replayed"}
		_, changed, e := FinishTurn(ctx, db, run.Turn, protocol.ConversationStatusInterrupted, failure, now)
		if e != nil {
			return nil, e
		}
		events = append(events, changed...)
	}
	automatic, e := readBodies[protocol.ConversationRun](ctx, db, "SELECT a.body FROM agent_runs a WHERE json_extract(a.body,'$.status')='queued' AND (coalesce(json_extract(a.body,'$.notice'),0)=1 OR EXISTS(SELECT 1 FROM agent_continuations c WHERE c.turn=a.turn))")
	if e != nil {
		return nil, e
	}
	for _, run := range automatic {
		changed, e := cancelAutomaticInput(ctx, db, run, &protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown, Message: "automatic input was interrupted before execution"}, now)
		if e != nil {
			return nil, e
		}
		events = append(events, changed...)
	}
	_, err = db.ExecContext(ctx, "UPDATE agent_queues SET paused=1,revision=revision+1 WHERE paused=0 AND EXISTS(SELECT 1 FROM agent_pending p JOIN turns t ON t.id=p.turn WHERE t.session=agent_queues.session)")
	return events, err
}
