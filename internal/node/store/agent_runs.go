package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"slices"
	"time"

	"github.com/sailry/sailry-harness/internal/node/agent/operations"
	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// captureTurnPlugins names the packages a new turn freezes: the enabled
// compatible inventory, or the bundled packages before the first inventory.
// An assistant resolves its current owner here; the turn then retains it.
func captureTurnPlugins(ctx context.Context, db *sql.Conn, assistant *protocol.PluginConversationBinding) ([]protocol.PluginReference, error) {
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
	if assistant != nil {
		owner, err := assistantPackage(ctx, db, *assistant)
		if err != nil {
			return nil, err
		}
		reference := plugins.PackageReference(owner)
		if index := slices.IndexFunc(references, func(candidate protocol.PluginReference) bool { return candidate.Name == reference.Name }); index >= 0 {
			references[index] = reference
		} else {
			references = append(references, reference)
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
	if err = bindTurnAssistant(&value); err != nil {
		return value, err
	}
	return value, nil
}

// turnSettings is everything a turn freezes when it is admitted.
type turnSettings struct {
	turn       protocol.QueuedTurn
	provider   []byte
	hooks      []byte
	compaction CompactionPolicy
	media      []byte
}

func admitAgentTurn(ctx context.Context, db *sql.Conn, node, caller protocol.NodeID, request protocol.Request, session protocol.Session, kind protocol.ConversationRunKind, ready bool) (protocol.QueuedTurn, error) {
	settings, err := captureTurnSettings(ctx, db, node, session, kind)
	if err != nil {
		return settings.turn, err
	}
	return insertAgentTurn(ctx, db, caller, request, session, settings, ready)
}

// captureTurnSettings validates and freezes the session's current settings.
func captureTurnSettings(ctx context.Context, db *sql.Conn, node protocol.NodeID, session protocol.Session, kind protocol.ConversationRunKind) (turnSettings, error) {
	value := protocol.DefaultQueuedTurn()
	value.Session = session.ID
	value.Revision = session.Revision
	value.Config = session.Config
	value.Roles = session.Roles
	settings := turnSettings{turn: value}
	// A forwarded session keeps the roles its source Node captured.
	if session.Profile == nil {
		if err := RefreshTurnRoles(ctx, db, node, &value.Roles); err != nil {
			return settings, err
		}
	}
	value.Kind = kind
	var err error
	if value.Plugins, err = captureTurnPlugins(ctx, db, session.Config.Assistant); err != nil {
		return settings, err
	}
	if err = bindTurnAssistant(&value); err != nil {
		return settings, err
	}
	settings.turn = value
	if err = validateAssistant(ctx, db, session.Project, value.Config); err != nil {
		return settings, err
	}
	if session.Delegation != nil {
		return settings, fault(protocol.ErrorCodePermissionDenied, "child conversation is read-only")
	}
	if hasRemoteModelCapture(session, node) {
		return settings, fault(protocol.ErrorCodeNotConfigured, "new turns require a model channel configured on this Node")
	}
	if session.Profile == nil {
		if err := validateConfig(ctx, db, node, session.Config); err != nil {
			return settings, err
		}
	} else if err := validateCapturedConfig(ctx, db, node, *session.Profile, session.Config); err != nil {
		return settings, err
	}
	var selectedProvider *protocol.ConversationProvider
	if session.Profile != nil {
		// A forwarded session runs on the provider its source Node captured.
		selectedProvider = &session.Profile.Provider
	} else {
		config, err := readProviderConfiguration(ctx, db, session.Config.Provider)
		if err != nil {
			return settings, err
		}
		if config != nil && config.Provider != nil {
			if !config.Listed {
				return settings, protocol.NewConfigurationFault(protocol.ConfigurationProviderRemoved, "model provider is unavailable")
			}
			selectedProvider = config.Provider
		}
	}
	if selectedProvider == nil {
		return settings, protocol.NewConfigurationFault(protocol.ConfigurationProviderMissing, "model provider is unavailable")
	}
	if !selectedProvider.Enabled {
		return settings, protocol.NewConfigurationFault(protocol.ConfigurationProviderDisabled, "model provider is unavailable")
	}
	var selected bool
	for _, model := range selectedProvider.Models {
		if model.ID == session.Config.Model {
			selected = true
			break
		}
	}
	if !selected {
		return settings, invalid("selected model is not configured on provider")
	}
	captured, err := captureTurnProvider(ctx, db, *selectedProvider, value.Config.Model)
	if err != nil {
		return settings, err
	}
	if settings.provider, err = encode(captured); err != nil {
		return settings, err
	}
	if settings.compaction, err = readCompactionPolicy(ctx, db); err != nil {
		return settings, err
	}
	if settings.hooks, err = captureHooks(ctx, db); err != nil {
		return settings, err
	}
	// Media assignments are frozen here; later settings changes reach only
	// turns admitted after them.
	assignments, err := CaptureMedia(ctx, db)
	if err != nil {
		return settings, err
	}
	settings.media, err = encode(assignments)
	return settings, err
}

// readTurnSettings reads the settings an admitted turn froze.
func readTurnSettings(ctx context.Context, db *sql.Conn, turn protocol.TurnID) (turnSettings, error) {
	value, err := readTurnCapture(ctx, db, turn)
	settings := turnSettings{turn: value}
	if err != nil {
		return settings, err
	}
	err = db.QueryRowContext(ctx, "SELECT provider,hooks,compaction_percent,media,compaction_default FROM agent_runs WHERE turn=?1", turn.String()).Scan(&settings.provider, &settings.hooks, &settings.compaction.Percent, &settings.media, &settings.compaction.UseDefault)
	return settings, err
}

// saveTurnSettings replaces the frozen settings of a turn that has not started.
func saveTurnSettings(ctx context.Context, db *sql.Conn, turn protocol.TurnID, settings turnSettings) error {
	plugins, err := encode(settings.turn.Plugins)
	if err != nil {
		return err
	}
	roles, err := encode(settings.turn.Roles)
	if err != nil {
		return err
	}
	if _, err = db.ExecContext(ctx, "UPDATE turns SET revision=?2,plugins=?3,roles=?4 WHERE id=?1", turn.String(), settings.turn.Revision, plugins, roles); err != nil {
		return err
	}
	if _, err = db.ExecContext(ctx, "UPDATE agent_runs SET provider=?2,hooks=?3,compaction_percent=?4,media=?5,compaction_default=?6 WHERE turn=?1", turn.String(), settings.provider, settings.hooks, settings.compaction.Percent, settings.media, settings.compaction.UseDefault); err != nil {
		return err
	}
	run, err := ReadAgentRun(ctx, db, turn)
	if err != nil {
		return err
	}
	run.Revision = settings.turn.Revision
	return putAgentRun(ctx, db, run)
}

func insertAgentTurn(ctx context.Context, db *sql.Conn, caller protocol.NodeID, request protocol.Request, session protocol.Session, settings turnSettings, ready bool) (protocol.QueuedTurn, error) {
	value := settings.turn
	value.ID = protocol.NewTurnID()
	value.Session = session.ID
	value.Request = request.ID
	var count uint64
	if err := db.QueryRowContext(ctx, "SELECT count(*) FROM agent_pending p JOIN turns t ON t.id=p.turn WHERE t.session=?1", session.ID.String()).Scan(&count); err != nil {
		return value, err
	}
	if count >= 256 {
		return value, fault(protocol.ErrorCodeBusy, "session queue is full")
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
	user, err := CurrentUser(ctx, db)
	if err != nil {
		return value, err
	}
	if _, err = db.ExecContext(ctx, "INSERT INTO turns(id,session,revision,request,caller,kind,plugins,roles,rowid,user) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)", value.ID.String(), session.ID.String(), value.Revision, request.ID.String(), caller[:], string(value.Kind), plugins, roles, seq, user.ID.String()); err != nil {
		return value, err
	}
	if err = retainTurnOrigin(ctx, db, value.ID); err != nil {
		return value, err
	}
	run := protocol.DefaultConversationRun()
	run.Turn = value.ID
	run.Session = value.Session
	run.Worktree = session.Worktree
	run.Kind = value.Kind
	run.Sequence = seq
	run.Revision = value.Revision
	run.Status = protocol.ConversationStatusQueued
	body, err := encode(run)
	if err != nil {
		return value, err
	}
	if _, err = db.ExecContext(ctx, "INSERT INTO agent_runs(turn,ready,body,provider,hooks,compaction_percent,media,compaction_default) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)", value.ID.String(), ready, body, settings.provider, settings.hooks, settings.compaction.Percent, settings.media, settings.compaction.UseDefault); err != nil {
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
		_, err = db.ExecContext(ctx, "UPDATE agent_queues SET paused=0,pause_reason=NULL WHERE session=?1", session.ID.String())
	}
	return value, err
}
func ClaimNext(ctx context.Context, db *sql.Conn, now time.Time) (*AgentInvocation, []protocol.Event, error) {
	events := []protocol.Event{}
	delivered := false
	for {
		var id string
		err := db.QueryRowContext(ctx, "SELECT a.turn FROM agent_runs a JOIN agent_pending p ON p.turn=a.turn JOIN turns t ON t.id=a.turn JOIN agent_queues q ON q.session=t.session WHERE a.ready=1 AND q.paused=0 AND p.recipient IS NULL AND NOT EXISTS(SELECT 1 FROM agent_delegations d WHERE d.turn=a.turn) AND json_extract(a.body,'$.status')='queued' AND NOT EXISTS(SELECT 1 FROM agent_runs active JOIN turns other ON other.id=active.turn WHERE other.session=t.session AND json_extract(active.body,'$.status') IN ('running','stopping')) ORDER BY p.position,t.rowid LIMIT 1").Scan(&id)
		if errors.Is(err, sql.ErrNoRows) {
			if delivered {
				return nil, events, nil
			}
			delivered = true
			admitted, changed, err := scheduleDeliveries(ctx, db, now)
			events = append(events, changed...)
			if err != nil || !admitted {
				return nil, events, err
			}
			continue
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
	origin, err := OriginalTurnUser(ctx, db, turn)
	if err != nil {
		return nil, nil, err
	}
	user, err := ResolveUser(ctx, db, origin.ID)
	if err != nil {
		return nil, nil, err
	}
	ctx = WithCapturedUser(WithUser(ctx, user), origin.ID)
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
	inv.User, inv.StorageUser = user, origin.ID
	if err = captureAssistant(ctx, db, inv); err != nil {
		return rejectInvocationCapture(ctx, db, turn, err, now)
	}
	catalog, err := operations.Bundled()
	if err != nil {
		return nil, nil, err
	}
	var provider, instructions, caller, hookSettings, media []byte
	var path string
	var project sql.NullString
	var main bool
	if err = db.QueryRowContext(ctx, "SELECT a.hooks,a.provider,a.compaction_percent,t.instructions,t.caller,w.path,w.project,w.main,a.media,a.compaction_default FROM agent_runs a JOIN turns t ON t.id=a.turn JOIN worktrees w ON w.id=?2 WHERE a.turn=?1", turn.String(), run.Worktree.String()).Scan(&hookSettings, &provider, &inv.Compaction.Percent, &instructions, &caller, &path, &project, &main, &media, &inv.Compaction.UseDefault); err != nil {
		return nil, nil, err
	}
	if media != nil {
		if err = json.Unmarshal(media, &inv.Media); err != nil {
			return nil, nil, err
		}
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
		failure, known := errors.AsType[*protocol.Fault](err)
		if initialized || !known || failure.Code != protocol.ErrorCodeNotConfigured {
			return rejectInvocationCapture(ctx, db, turn, err, now)
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
		node, err := storageNode(ctx, db)
		if err != nil {
			return nil, nil, err
		}
		// Resolve the admitted credential itself: a controller may select a
		// newer key while the executor still retains this captured generation.
		if inv.Provider != nil {
			inv.Secret, err = resolveCapturedProviderKey(ctx, db, node, *ref, *inv.Provider)
		} else {
			inv.Secret, err = resolveProviderKey(ctx, db, node, *ref, session.Config.Provider)
		}
		if err != nil && !errors.As(err, &inv.AuthorizationFailure) {
			return nil, nil, err
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
	if session.Delegation == nil {
		if err = addressSuccessors(ctx, db, run); err != nil {
			return nil, nil, err
		}
	}
	// An automatic turn that starts takes back the unread mark its
	// predecessor raised: the session is working by itself again, and the
	// turn that ends the chain raises the mark. Otherwise every automatic
	// turn of a package loop would flag a session the user has already read.
	if run.Notice || run.Continuation != nil {
		if _, err = db.ExecContext(ctx, "UPDATE session_attention SET revision=revision+1,unread=0 WHERE session=?1 AND unread=1", run.Session.String()); err != nil {
			return nil, nil, err
		}
	}
	if err = bumpQueue(ctx, db, run.Session); err != nil {
		return nil, nil, err
	}
	events, err := activityEvent(ctx, db, run.Session)
	return inv, events, err
}

// A permanent error in this turn's captured configuration must not block the
// next ready turn. FinishTurn preserves input that never reached history.
func rejectInvocationCapture(ctx context.Context, db *sql.Conn, turn protocol.TurnID, cause error, now time.Time) (*AgentInvocation, []protocol.Event, error) {
	failure, ok := errors.AsType[*protocol.Fault](cause)
	if !ok || failure.Code != protocol.ErrorCodeNotConfigured && failure.Code != protocol.ErrorCodeInvalidRequest {
		return nil, nil, cause
	}
	_, events, err := FinishTurn(ctx, db, turn, protocol.ConversationStatusFailed, failure, now)
	return nil, events, err
}

func FinishTurn(ctx context.Context, db *sql.Conn, turn protocol.TurnID, status protocol.ConversationStatus, failure *protocol.Fault, now time.Time) (protocol.ConversationRun, []protocol.Event, error) {
	run, err := ReadAgentRun(ctx, db, turn)
	if err != nil {
		return run, nil, err
	}
	origin, err := readDelegation(ctx, db, turn)
	if err != nil {
		return run, nil, err
	}
	// stop_turn already paused the queue; whatever resumed it since then, such
	// as Send now, is the user's later decision and is not undone here.
	stopped := run.Status == protocol.ConversationStatusStopping
	// A deliberate stop ends the turn cancelled whatever it was doing; only a
	// lost owner (Node shutdown, restart recovery) is interrupted.
	if stopped && status != protocol.ConversationStatusInterrupted {
		status = protocol.ConversationStatusCancelled
		if failure != nil && failure.Code != protocol.ErrorCodeOutcomeUnknown {
			failure = nil
		}
	}
	run.Status = status
	run.Waiting = nil
	run.Error = failure
	ms := now.UnixMilli()
	run.FinishedMs = &ms
	if _, err = db.ExecContext(ctx, "UPDATE agent_runs SET ready=0 WHERE turn=?1", turn.String()); err != nil {
		return run, nil, err
	}
	if err = putAgentRun(ctx, db, run); err != nil {
		return run, nil, err
	}
	// Queue membership depends on run status, not just pending rows. An input
	// stopped before its first save reappears here even if Stop already paused
	// the queue or a later submission resumed it.
	if err = bumpQueue(ctx, db, run.Session); err != nil {
		return run, nil, err
	}
	closed := "cancelled"
	if status == protocol.ConversationStatusInterrupted {
		closed = "interrupted"
	}
	if err = closeInteractions(ctx, db, turn, closed); err != nil {
		return run, nil, err
	}
	pause, retained := false, false
	if origin != nil {
		pause = failure != nil || status != protocol.ConversationStatusCompleted
	} else {
		// Guidance its recipient never joined becomes ordinary queued input.
		if err = releaseGuidance(ctx, db, run); err != nil {
			return run, nil, err
		}
		if !stopped {
			// A failed turn keeps the queue and automatic delivery running.
			// Only a lost owner or input that never reached history holds them.
			if retained, err = retainedTurn(ctx, db, turn); err != nil {
				return run, nil, err
			}
			pause = status == protocol.ConversationStatusInterrupted || retained
		}
	}
	if pause {
		switch {
		case status == protocol.ConversationStatusInterrupted:
			err = pauseQueue(ctx, db, run.Session, protocol.ConversationQueuePauseInterrupted)
		case retained:
			err = pauseQueue(ctx, db, run.Session, protocol.ConversationQueuePauseUnrecorded)
		case status == protocol.ConversationStatusCancelled:
			err = pauseQueue(ctx, db, run.Session, protocol.ConversationQueuePauseStopped)
		default:
			// A failed child holds its own queue; no reason is shown for it.
			err = setQueuePaused(ctx, db, run.Session, true)
		}
		if err != nil {
			return run, nil, err
		}
	}
	// Attention marks a main task turn that ended without the user: completed,
	// failed or interrupted. Stopped turns, compaction and child runs do not.
	if origin == nil && run.Kind == protocol.ConversationRunKindTask && status != protocol.ConversationStatusCancelled {
		if _, err = db.ExecContext(ctx, "INSERT INTO session_attention(session,revision,unread) VALUES(?1,1,1) ON CONFLICT(session) DO UPDATE SET revision=revision+1,unread=1", run.Session.String()); err != nil {
			return run, nil, err
		}
	}
	events, err := activityEvent(ctx, db, run.Session)
	if err != nil {
		return run, nil, err
	}
	notice, err := runNotice(ctx, db, run)
	if err != nil {
		return run, nil, err
	}
	events = append(events, notice...)
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
	// Results a notice turn ended without taking wait for the user. Held
	// results another turn ended without taking stay held, for this reason.
	var hold *protocol.ConversationBackgroundHold
	switch status {
	case protocol.ConversationStatusCancelled:
		hold = new(protocol.ConversationBackgroundHoldStopped)
	case protocol.ConversationStatusInterrupted:
		hold = new(protocol.ConversationBackgroundHoldInterrupted)
	default:
		hold = new(protocol.ConversationBackgroundHoldFailed)
	}
	changed, err = ReleaseBackground(ctx, db, turn, hold, run.Notice)
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
	node, err := nodeIdentity(ctx, db)
	if err != nil {
		return run, nil, err
	}
	_, changed, err = ScheduleBackground(ctx, db, node, owner, now)
	events = append(events, changed...)
	return run, events, err
}

// recoveredKey holds when the Node last started, in agent_state under the
// owner "node": a run that finished earlier ended in an earlier process.
const recoveredKey = "sailry.recovered_ms"

func RecoverRuns(ctx context.Context, db *sql.Conn, now time.Time) ([]protocol.Event, error) {
	stamp, err := encode(now.UnixMilli())
	if err != nil {
		return nil, err
	}
	if _, err = db.ExecContext(ctx, "INSERT INTO agent_state(owner,key,body) VALUES('node',?1,?2) ON CONFLICT(owner,key) DO UPDATE SET body=excluded.body", recoveredKey, stamp); err != nil {
		return nil, err
	}
	// A queued child the user stopped before the restart was a deliberate
	// stop: it ends cancelled, not as lost work.
	events, err := SettleStoppedChildren(ctx, db, now)
	if err != nil {
		return nil, err
	}
	// A delegated run that never started is claimed only through its in-memory
	// owner, which a restart loses; left queued it would block every later
	// child of its conversation.
	runs, err := readBodies[protocol.ConversationRun](ctx, db, "SELECT a.body FROM agent_runs a WHERE json_extract(a.body,'$.status') IN ('running','stopping') OR (json_extract(a.body,'$.status')='queued' AND EXISTS(SELECT 1 FROM agent_delegations d WHERE d.turn=a.turn) AND NOT EXISTS(SELECT 1 FROM agent_pending p WHERE p.turn=a.turn))")
	if err != nil {
		return nil, err
	}
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
	// No turn survives a restart, so no guidance has a recipient left to join.
	if _, err = db.ExecContext(ctx, "UPDATE agent_queues SET revision=revision+1 WHERE session IN (SELECT t.session FROM agent_pending p JOIN turns t ON t.id=p.turn WHERE p.recipient IS NOT NULL)"); err != nil {
		return nil, err
	}
	if _, err = db.ExecContext(ctx, "UPDATE agent_pending SET recipient=NULL,revision=revision+1 WHERE recipient IS NOT NULL"); err != nil {
		return nil, err
	}
	_, err = db.ExecContext(ctx, "UPDATE agent_queues SET paused=1,pause_reason='interrupted',revision=revision+1 WHERE paused=0 AND EXISTS(SELECT 1 FROM agent_pending p JOIN turns t ON t.id=p.turn WHERE t.session=agent_queues.session)")
	return events, err
}
