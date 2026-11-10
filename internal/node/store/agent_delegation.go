package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"strings"
	"time"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type delegationArguments struct {
	Session    *protocol.SessionID  `json:"session"`
	Role       *string              `json:"role"`
	Title      *string              `json:"title"`
	Task       string               `json:"task"`
	Worktree   *protocol.WorktreeID `json:"worktree"`
	Background bool                 `json:"background"`
}

func readDelegation(ctx context.Context, db *sql.Conn, turn protocol.TurnID) (*protocol.Delegation, error) {
	return readBody[protocol.Delegation](ctx, db, "SELECT body FROM agent_delegations WHERE turn=?1", turn.String())
}
func foundingDelegation(ctx context.Context, db *sql.Conn, session protocol.SessionID) (*protocol.Delegation, error) {
	return readBody[protocol.Delegation](ctx, db, "SELECT d.body FROM agent_delegations d JOIN turns t ON t.id=d.turn WHERE t.session=?1 ORDER BY t.rowid LIMIT 1", session.String())
}
func queuedInvocationInput(ctx context.Context, db *sql.Conn, turn protocol.TurnID) (protocol.ConversationQueuedMessage, error) {
	var body []byte
	if err := db.QueryRowContext(ctx, "SELECT input FROM agent_runs WHERE turn=?1", turn.String()).Scan(&body); err != nil {
		return protocol.ConversationQueuedMessage{}, err
	}
	if len(body) == 0 {
		return readQueuedMessage(ctx, db, turn)
	}
	value := protocol.DefaultConversationQueuedMessage()
	var err error
	value.Turn, err = readTurnCapture(ctx, db, turn)
	if err != nil {
		return value, err
	}
	err = json.Unmarshal(body, &value.Message)
	return value, err
}
func DelegateAgent(ctx context.Context, db *sql.Conn, node protocol.NodeID, parent protocol.TurnID, identity canonical.ToolIdentity, selected *protocol.ConversationInput, ready bool, now time.Time) (AgentInvocation, []protocol.Event, error) {
	return delegateAgent(ctx, db, node, parent, identity, selected, ready, now, nil)
}
func delegateAgent(ctx context.Context, db *sql.Conn, node protocol.NodeID, parent protocol.TurnID, identity canonical.ToolIdentity, selected *protocol.ConversationInput, ready bool, now time.Time, guidance *protocol.ConversationRun) (AgentInvocation, []protocol.Event, error) {
	empty := AgentInvocation{}
	if identity.Name != "spawn_agent" && identity.Name != "guide_agent" {
		return empty, nil, invalid("invalid delegation tool")
	}
	parentRun, err := activeAgentRun(ctx, db, parent)
	if err != nil {
		return empty, nil, err
	}
	previous, err := ReadSession(ctx, db, parentRun.Session)
	if err != nil {
		return empty, nil, err
	}
	if previous.Delegation != nil {
		return empty, nil, fault(protocol.ErrorCodePermissionDenied, "delegated children cannot delegate")
	}
	entry, part, _, err := locateAgentCall(ctx, db, parent, identity)
	if err != nil {
		return empty, nil, err
	}
	var existing bool
	if err = db.QueryRowContext(ctx, "SELECT EXISTS(SELECT 1 FROM agent_delegations WHERE parent_turn=?1 AND entry=?2 AND part=?3)", parent.String(), entry, part).Scan(&existing); err != nil {
		return empty, nil, err
	}
	if existing {
		return empty, nil, fault(protocol.ErrorCodeConflict, "child call was already admitted; execution was not replayed")
	}
	var args delegationArguments
	if identity.Name == "guide_agent" {
		var guide struct {
			Session protocol.SessionID `json:"session"`
			Message string             `json:"message"`
		}
		if err = protocol.DecodeObject(identity.Arguments, &guide, []string{"session", "message"}, true); err != nil {
			return empty, nil, invalid("invalid child guidance arguments")
		}
		args.Session, args.Task = &guide.Session, guide.Message
	} else if err = protocol.DecodeObject(identity.Arguments, &args, []string{"task"}, true); err != nil {
		return empty, nil, invalid("invalid delegation arguments")
	}
	var fields map[string]json.RawMessage
	_ = json.Unmarshal(identity.Arguments, &fields)
	if value, ok := fields["session"]; ok && string(value) == "null" {
		return empty, nil, invalid("child session must not be null")
	}
	if args.Title != nil && (strings.TrimSpace(*args.Title) == "" || strings.ContainsAny(*args.Title, "\r\n")) {
		return empty, nil, invalid("invalid child title")
	}
	message := protocol.ConversationInput{Text: args.Task}
	if selected != nil {
		message = *selected
	}
	if err = validateAgentInput(message); err != nil {
		return empty, nil, err
	}
	capture, err := readTurnCapture(ctx, db, parent)
	if err != nil {
		return empty, nil, err
	}
	if capture.Roles.Execution != protocol.ConversationExecutionTeam {
		return empty, nil, fault(protocol.ErrorCodePermissionDenied, "model execution cannot delegate")
	}
	var role *protocol.RoleProfile
	session := previous
	var provider *protocol.ConversationProvider
	var providerBody []byte
	if err = db.QueryRowContext(ctx, "SELECT provider FROM agent_runs WHERE turn=?1", parent.String()).Scan(&providerBody); err != nil {
		return empty, nil, err
	}
	if err = json.Unmarshal(providerBody, &provider); err != nil {
		return empty, nil, err
	}
	created := args.Session == nil
	inherit := parent
	if guidance != nil {
		inherit = guidance.Turn
	}
	if created {
		session = protocol.DefaultSession()
		session.ID, session.Project, session.Worktree, session.Config, session.Revision = protocol.NewSessionID(), previous.Project, parentRun.Worktree, capture.Config, 1
		if args.Worktree != nil {
			if previous.Project == nil {
				return empty, nil, invalid("unassigned conversations cannot select another worktree")
			}
			var owns bool
			if err = db.QueryRowContext(ctx, "SELECT EXISTS(SELECT 1 FROM worktrees w WHERE w.id=?1 AND w.project=?2 AND NOT EXISTS(SELECT 1 FROM removed_worktrees r WHERE r.worktree=w.id))", args.Worktree.String(), previous.Project.String()).Scan(&owns); err != nil {
				return empty, nil, err
			}
			if !owns {
				return empty, nil, fault(protocol.ErrorCodeWrongTarget, "child worktree belongs to another project")
			}
			session.Worktree = *args.Worktree
		}
		if selected != nil && session.Worktree != parentRun.Worktree {
			return empty, nil, invalid("selected input cannot be delegated to another worktree")
		}
		if args.Role != nil {
			for _, candidate := range capture.Roles.Profiles {
				if candidate.Key == *args.Role {
					copy := candidate
					role = &copy
					break
				}
			}
			if role == nil {
				return empty, nil, invalid("role is not in the parent turn snapshot")
			}
			if role.ReadOnly {
				session.Config.Mode = protocol.WorkModePlan
			}
			model := role.Model
			if override, ok := capture.Roles.Models[role.ID]; ok {
				model = &override
			}
			if model != nil {
				provider = nil
				for _, candidate := range capture.Roles.Providers {
					if candidate.ID == model.Provider {
						copy := candidate
						provider = &copy
						break
					}
				}
				if provider == nil {
					return empty, nil, invalid("frozen role provider is missing")
				}
				session.Config, err = roleModelConfig(session.Config, *provider, *model)
				if err != nil {
					return empty, nil, err
				}
			}
		}
	} else {
		session, err = ReadSession(ctx, db, *args.Session)
		if err != nil {
			return empty, nil, err
		}
		origin, err := foundingDelegation(ctx, db, session.ID)
		if err != nil {
			return empty, nil, err
		}
		if origin == nil || origin.Session != parentRun.Session {
			return empty, nil, fault(protocol.ErrorCodeWrongTarget, "child belongs to another parent")
		}
		var busy bool
		if err = db.QueryRowContext(ctx, "SELECT EXISTS(SELECT 1 FROM agent_runs a JOIN turns t ON t.id=a.turn WHERE t.session=?1 AND json_extract(a.body,'$.status') IN ('queued','running','stopping'))", session.ID.String()).Scan(&busy); err != nil {
			return empty, nil, err
		}
		if busy && guidance == nil {
			return empty, nil, fault(protocol.ErrorCodeBusy, "child has active or pending work")
		}
		if args.Worktree != nil && *args.Worktree != session.Worktree {
			return empty, nil, fault(protocol.ErrorCodeConflict, "child worktree is immutable")
		}
		var owns bool
		if err = db.QueryRowContext(ctx, "SELECT EXISTS(SELECT 1 FROM worktrees w WHERE w.id=?1 AND NOT EXISTS(SELECT 1 FROM removed_worktrees r WHERE r.worktree=w.id))", session.Worktree.String()).Scan(&owns); err != nil {
			return empty, nil, err
		}
		if !owns {
			return empty, nil, fault(protocol.ErrorCodeNotFound, "child worktree is unavailable")
		}
		if !sameProject(previous.Project, session.Project) || previous.Project == nil && session.Worktree != parentRun.Worktree {
			return empty, nil, fault(protocol.ErrorCodeWrongTarget, "child belongs to another project or worktree")
		}
		permission := func(value protocol.Permission) int {
			switch value {
			case protocol.PermissionAsk:
				return 0
			case protocol.PermissionProject:
				return 1
			default:
				return 2
			}
		}
		if capture.Config.Mode == protocol.WorkModePlan && session.Config.Mode != protocol.WorkModePlan || permission(session.Config.Permission) > permission(capture.Config.Permission) {
			return empty, nil, fault(protocol.ErrorCodePermissionDenied, "child exceeds parent permissions")
		}
		role, err = roleFromDelegation(ctx, db, *origin)
		if err != nil {
			return empty, nil, err
		}
		if _, explicit := fields["role"]; explicit {
			var chosen *protocol.RoleID
			if args.Role != nil {
				for _, candidate := range capture.Roles.Profiles {
					if candidate.Key == *args.Role {
						copy := candidate.ID
						chosen = &copy
						break
					}
				}
				if chosen == nil {
					return empty, nil, invalid("role is not in the parent turn snapshot")
				}
			}
			if chosen == nil && role != nil || chosen != nil && (role == nil || role.ID != *chosen) {
				return empty, nil, fault(protocol.ErrorCodeConflict, "child role is immutable")
			}
		}
		if session.Profile != nil {
			copy := session.Profile.Provider
			provider = &copy
		} else {
			provider = nil
		}
	}
	if provider == nil {
		return empty, nil, fault(protocol.ErrorCodeNotConfigured, "captured child provider is unavailable")
	}
	captured, err := CaptureProvider(ctx, db, *provider)
	if err != nil {
		return empty, nil, err
	}
	provider = &captured
	if selected != nil && session.Worktree != parentRun.Worktree {
		return empty, nil, invalid("selected input cannot be delegated to another worktree")
	}
	if created {
		session.CreationOrder, err = NextCounter(ctx, db, CounterSessions)
		if err != nil {
			return empty, nil, err
		}
		session.Profile = &protocol.SessionProfile{Source: node, Provider: *provider}
		var project any
		if session.Project != nil {
			project = session.Project.String()
		}
		if _, err = db.ExecContext(ctx, "INSERT INTO sessions(rowid,id,project,worktree,revision) VALUES(?1,?2,?3,?4,1)", session.CreationOrder, session.ID.String(), project, session.Worktree.String()); err != nil {
			return empty, nil, err
		}
		if err = insertSessionRevision(ctx, db, session); err != nil {
			return empty, nil, err
		}
		if _, err = db.ExecContext(ctx, "INSERT INTO session_media(session,body) SELECT ?1,body FROM session_media WHERE session=?2", session.ID.String(), previous.ID.String()); err != nil {
			return empty, nil, err
		}
		if args.Title != nil {
			body, e := encode(*args.Title)
			if e != nil {
				return empty, nil, e
			}
			if _, err = db.ExecContext(ctx, "INSERT INTO agent_state(owner,key,body) VALUES(?1,?2,?3)", session.ID.String(), sessionTitleKey, body); err != nil {
				return empty, nil, err
			}
		}
	}
	turn := protocol.DefaultQueuedTurn()
	turn.ID, turn.Kind, turn.Session, turn.Request, turn.Revision, turn.Config, turn.Roles, turn.Plugins = protocol.NewTurnID(), protocol.ConversationRunKindTask, session.ID, capture.Request, session.Revision, session.Config, session.Roles, capture.Plugins
	if guidance != nil {
		saved, e := readTurnCapture(ctx, db, guidance.Turn)
		if e != nil {
			return empty, nil, e
		}
		turn.Config, turn.Roles, turn.Plugins, turn.Revision = saved.Config, saved.Roles, saved.Plugins, saved.Revision
		if e = db.QueryRowContext(ctx, "SELECT provider FROM agent_runs WHERE turn=?1", guidance.Turn.String()).Scan(&providerBody); e != nil {
			return empty, nil, e
		}
		if e = json.Unmarshal(providerBody, &provider); e != nil {
			return empty, nil, e
		}
	}
	seq, err := NextCounter(ctx, db, CounterTurns)
	if err != nil {
		return empty, nil, err
	}
	var caller, instructions, hooks []byte
	var percent uint8
	if err = db.QueryRowContext(ctx, "SELECT t.caller,t.instructions,a.hooks,a.compaction_percent FROM turns t JOIN agent_runs a ON a.turn=t.id WHERE t.id=?1", inherit.String()).Scan(&caller, &instructions, &hooks, &percent); err != nil {
		return empty, nil, err
	}
	if guidance != nil {
		if err = db.QueryRowContext(ctx, "SELECT caller FROM turns WHERE id=?1", parent.String()).Scan(&caller); err != nil {
			return empty, nil, err
		}
	}
	plugins, err := encode(turn.Plugins)
	if err != nil {
		return empty, nil, err
	}
	roles, err := encode(turn.Roles)
	if err != nil {
		return empty, nil, err
	}
	if _, err = db.ExecContext(ctx, "INSERT INTO turns(id,session,revision,request,caller,kind,plugins,roles,instructions,rowid) VALUES(?1,?2,?3,?4,?5,'task',?6,?7,?8,?9)", turn.ID.String(), session.ID.String(), turn.Revision, capture.Request.String(), caller, plugins, roles, instructions, seq); err != nil {
		return empty, nil, err
	}
	origin := protocol.Delegation{Session: parentRun.Session, Turn: parent, Entry: entry, Index: part}
	if role != nil {
		origin.Role = &role.ID
	}
	body, err := encode(origin)
	if err != nil {
		return empty, nil, err
	}
	if _, err = db.ExecContext(ctx, "INSERT INTO agent_delegations(turn,parent_turn,entry,part,body) VALUES(?1,?2,?3,?4,?5)", turn.ID.String(), parent.String(), entry, part, body); err != nil {
		return empty, nil, err
	}
	if err = BindTurnAttachments(ctx, db, turn.ID, message.Attachments); err != nil {
		return empty, nil, err
	}
	run := protocol.DefaultConversationRun()
	run.Turn, run.Session, run.Worktree, run.Kind, run.Sequence, run.Revision, run.Status = turn.ID, session.ID, session.Worktree, turn.Kind, seq, turn.Revision, protocol.ConversationStatusQueued
	body, err = encode(run)
	if err != nil {
		return empty, nil, err
	}
	providerBody, err = encode(provider)
	if err != nil {
		return empty, nil, err
	}
	input, err := encode(message)
	if err != nil {
		return empty, nil, err
	}
	if _, err = db.ExecContext(ctx, "INSERT INTO agent_runs(turn,ready,body,provider,hooks,compaction_percent,input) VALUES(?1,?2,?3,?4,?5,?6,?7)", turn.ID.String(), ready, body, providerBody, hooks, percent, input); err != nil {
		return empty, nil, err
	}
	if _, err = db.ExecContext(ctx, "INSERT INTO conversation_turns(session,turn) VALUES(?1,?2)", session.ID.String(), turn.ID.String()); err != nil {
		return empty, nil, err
	}
	if err = bumpQueue(ctx, db, session.ID); err != nil {
		return empty, nil, err
	}
	if _, err = db.ExecContext(ctx, "UPDATE agent_queues SET paused=0 WHERE session=?1", session.ID.String()); err != nil {
		return empty, nil, err
	}
	if guidance != nil {
		if _, err = db.ExecContext(ctx, "INSERT INTO agent_pending(turn,position,revision,request,caller) SELECT ?1,coalesce(max(p.position),0)+1,1,?2,?3 FROM agent_pending p JOIN turns t ON t.id=p.turn WHERE t.session=?4", turn.ID.String(), capture.Request.String(), caller, session.ID.String()); err != nil {
			return empty, nil, err
		}
		if err = tagGuidance(ctx, db, turn.ID, guidance.Turn); err != nil {
			return empty, nil, err
		}
	}
	inv := AgentInvocation{Session: session, Run: run, Turn: turn, Message: message, Provider: provider}
	if err = captureAssistant(ctx, db, &inv); err != nil {
		return empty, nil, err
	}
	originCopy := &origin
	inv.Session.Delegation = &originCopy
	events := []protocol.Event{}
	if created {
		events = append(events, protocol.Event{Kind: "session_changed", Data: inv.Session})
	}
	if guidance != nil {
		changed, err := activityEvent(ctx, db, parentRun.Session)
		return inv, append(events, changed...), err
	}
	changed, err := RegisterChildBackground(ctx, db, parentRun, run, func() string {
		if args.Title != nil {
			return *args.Title
		}
		return args.Task
	}(), now)
	events = append(events, changed...)
	return inv, events, err
}
func ReadChildRole(ctx context.Context, db *sql.Conn, turn protocol.TurnID) (*protocol.RoleProfile, error) {
	run, err := ReadAgentRun(ctx, db, turn)
	if err != nil {
		return nil, err
	}
	origin, err := foundingDelegation(ctx, db, run.Session)
	if err != nil || origin == nil {
		return nil, err
	}
	current, err := readDelegation(ctx, db, turn)
	if err != nil {
		return nil, err
	}
	if current == nil || current.Session != origin.Session || !sameAssistant(current.Role, origin.Role) {
		return nil, invalid("child ownership differs from its founding delegation")
	}
	if origin.Role == nil {
		return nil, nil
	}
	return roleFromDelegation(ctx, db, *origin)
}
func roleFromDelegation(ctx context.Context, db *sql.Conn, origin protocol.Delegation) (*protocol.RoleProfile, error) {
	if origin.Role == nil {
		return nil, nil
	}
	parent, err := readTurnCapture(ctx, db, origin.Turn)
	if err != nil {
		return nil, err
	}
	for _, role := range parent.Roles.Profiles {
		if role.ID == *origin.Role {
			return &role, nil
		}
	}
	return nil, invalid("frozen child role is missing")
}
