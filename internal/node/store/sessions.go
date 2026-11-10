package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"slices"
	"sort"
	"strings"
	"unicode"

	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/protocol"
)

const sessionTitleKey = "sailry.session_title"

func ReadSession(ctx context.Context, db *sql.Conn, id protocol.SessionID) (protocol.Session, error) {
	value := protocol.DefaultSession()
	value.ID = id
	var project sql.NullString
	var worktree string
	var config, roles, profile []byte
	err := db.QueryRowContext(ctx, "SELECT s.rowid,s.project,s.worktree,s.revision,s.state='archived',r.config,r.roles,r.profile FROM sessions s JOIN session_revisions r ON r.session=s.id AND r.revision=s.revision WHERE s.id=?1 AND s.state!='removed'", id.String()).Scan(&value.CreationOrder, &project, &worktree, &value.Revision, &value.Archived, &config, &roles, &profile)
	if errors.Is(err, sql.ErrNoRows) {
		return value, fault(protocol.ErrorCodeNotFound, "session does not exist on this Node")
	}
	if err != nil {
		return value, err
	}
	if project.Valid {
		parsed, err := protocol.ParseProjectID(project.String)
		if err != nil {
			return value, err
		}
		value.Project = &parsed
	}
	value.Worktree, err = protocol.ParseWorktreeID(worktree)
	if err != nil {
		return value, err
	}
	if err := json.Unmarshal(config, &value.Config); err != nil {
		return value, err
	}
	if err := json.Unmarshal(roles, &value.Roles); err != nil {
		return value, err
	}
	if len(profile) != 0 {
		if err := json.Unmarshal(profile, &value.Profile); err != nil {
			return value, err
		}
	}
	association := protocol.ConversationAssociation{}
	err = db.QueryRowContext(ctx, "SELECT namespace,kind,identity FROM session_associations WHERE session=?1", id.String()).Scan(&association.Namespace, &association.Kind, &association.ID)
	if err == nil {
		value.Association = &association
	} else if !errors.Is(err, sql.ErrNoRows) {
		return value, err
	}
	var source, through string
	err = db.QueryRowContext(ctx, "SELECT source,through_turn FROM session_forks WHERE session=?1", id.String()).Scan(&source, &through)
	if err == nil {
		origin, err := protocol.ParseSessionID(source)
		if err != nil {
			return value, err
		}
		turn, err := protocol.ParseTurnID(through)
		if err != nil {
			return value, err
		}
		value.Fork = &protocol.ConversationFork{Session: origin, Through: turn}
	} else if !errors.Is(err, sql.ErrNoRows) {
		return value, err
	}
	delegation, err := readBody[protocol.Delegation](ctx, db, "SELECT d.body FROM agent_delegations d JOIN turns t ON t.id=d.turn WHERE t.session=?1 ORDER BY t.rowid LIMIT 1", id.String())
	if err != nil {
		return value, err
	}
	if delegation != nil {
		value.Delegation = &delegation
	}
	value.Activity, err = readActivity(ctx, db, id)
	return value, err
}

func sessionOrder(ctx context.Context, db *sql.Conn) ([]protocol.SessionID, error) {
	rows, err := db.QueryContext(ctx, "SELECT id FROM sessions WHERE state!='removed' ORDER BY rowid DESC")
	if err != nil {
		return nil, err
	}
	values := []protocol.SessionID{}
	for rows.Next() {
		var text string
		if err := rows.Scan(&text); err != nil {
			rows.Close()
			return nil, err
		}
		id, err := protocol.ParseSessionID(text)
		if err != nil {
			rows.Close()
			return nil, err
		}
		values = append(values, id)
	}
	err = errors.Join(rows.Err(), rows.Close())
	if err != nil {
		return nil, err
	}
	saved, err := readBody[protocol.Event](ctx, db, "SELECT body FROM events WHERE json_extract(body,'$.kind')='sessions_reordered' ORDER BY cursor DESC LIMIT 1")
	if err != nil {
		return nil, err
	}
	if saved != nil {
		order, ok := saved.Data.(*[]protocol.SessionID)
		if !ok {
			return nil, invalid("invalid saved session order")
		}
		positions := make(map[protocol.SessionID]int, len(*order))
		for index, id := range *order {
			positions[id] = index + 1
		}
		sort.SliceStable(values, func(i, j int) bool { return positions[values[i]] < positions[values[j]] })
	}
	return values, nil
}

func ReadSessions(ctx context.Context, db *sql.Conn) ([]protocol.Session, error) {
	ids, err := sessionOrder(ctx, db)
	if err != nil {
		return nil, err
	}
	values := make([]protocol.Session, 0, len(ids))
	for _, id := range ids {
		value, err := ReadSession(ctx, db, id)
		if err != nil {
			return nil, err
		}
		values = append(values, value)
	}
	return values, nil
}

func insertSessionRevision(ctx context.Context, db *sql.Conn, value protocol.Session) error {
	var previous []byte
	err := db.QueryRowContext(ctx, "SELECT config FROM session_revisions WHERE session=?1 ORDER BY revision DESC LIMIT 1", value.ID.String()).Scan(&previous)
	if err != nil && !errors.Is(err, sql.ErrNoRows) {
		return err
	}
	config, err := preserveConfig(previous, value.Config)
	if err != nil {
		return err
	}
	roles, err := encode(value.Roles)
	if err != nil {
		return err
	}
	var profile any
	if value.Profile != nil {
		profile, err = encode(value.Profile)
		if err != nil {
			return err
		}
	}
	_, err = db.ExecContext(ctx, "INSERT INTO session_revisions(session,revision,worktree,config,profile,roles) VALUES(?1,?2,?3,?4,?5,?6)", value.ID.String(), value.Revision, value.Worktree.String(), config, profile, roles)
	return err
}

func reviseSession(ctx context.Context, db *sql.Conn, value protocol.Session) (protocol.Output, []protocol.Event, error) {
	if value.Delegation != nil {
		return protocol.Output{}, nil, fault(protocol.ErrorCodePermissionDenied, "child conversation is read-only")
	}
	revision, err := nextRevision(value.Revision)
	if err != nil {
		return protocol.Output{}, nil, err
	}
	value.Revision = revision
	if err := insertSessionRevision(ctx, db, value); err != nil {
		return protocol.Output{}, nil, err
	}
	_, err = db.ExecContext(ctx, "UPDATE sessions SET revision=?2,worktree=?3 WHERE id=?1", value.ID.String(), value.Revision, value.Worktree.String())
	return protocol.Output{Kind: "session", Data: value}, []protocol.Event{{Kind: "session_changed", Data: &value}}, err
}

func createSession(ctx context.Context, db *sql.Conn, node protocol.NodeID, profile string, input protocol.CommandCreateSessionPayload, capturedRoles ...protocol.RoleSnapshot) (protocol.Output, []protocol.Event, error) {
	config := input.Config
	if config == nil {
		defaults, err := ReadDefaults(ctx, db)
		if err != nil {
			return protocol.Output{}, nil, err
		}
		config = defaults.Config
	}
	if config == nil {
		return protocol.Output{}, nil, invalid("Node model defaults are not configured")
	}
	if err := validateConfig(ctx, db, node, *config); err != nil {
		return protocol.Output{}, nil, err
	}
	if err := validateAssistant(ctx, db, input.Project, *config); err != nil {
		return protocol.Output{}, nil, err
	}
	value := protocol.DefaultSession()
	value.ID, value.Project, value.Config, value.Revision = protocol.NewSessionID(), input.Project, *config, 1
	var err error
	if len(capturedRoles) > 0 {
		value.Roles = capturedRoles[0]
	} else {
		value.Roles, err = CaptureSessionRoles(ctx, db, node, protocol.ConversationExecutionTeam, nil, nil)
		if err != nil {
			return protocol.Output{}, nil, err
		}
	}
	if input.Project != nil {
		var selected string
		query := "SELECT id FROM worktrees WHERE project=?1 AND main=1"
		args := []any{input.Project.String()}
		if input.Worktree != nil {
			query = "SELECT id FROM worktrees WHERE project=?1 AND id=?2 AND id NOT IN (SELECT worktree FROM removed_worktrees)"
			args = append(args, input.Worktree.String())
		}
		err := db.QueryRowContext(ctx, query, args...).Scan(&selected)
		if errors.Is(err, sql.ErrNoRows) {
			return protocol.Output{}, nil, fault(protocol.ErrorCodeNotFound, "worktree does not belong to the selected project")
		}
		if err != nil {
			return protocol.Output{}, nil, err
		}
		value.Worktree, err = protocol.ParseWorktreeID(selected)
		if err != nil {
			return protocol.Output{}, nil, err
		}
	} else if config.Assistant != nil {
		value.Worktree, err = assistantWorkspace(ctx, db, profile, *config.Assistant)
		if err != nil {
			return protocol.Output{}, nil, err
		}
		if input.Worktree != nil && *input.Worktree != value.Worktree {
			return protocol.Output{}, nil, invalid("worktree does not belong to the plugin assistant")
		}
	} else {
		if input.Worktree != nil {
			return protocol.Output{}, nil, invalid("an unassigned conversation creates its own workspace")
		}
		value.Worktree = protocol.NewWorktreeID()
		if _, err := db.ExecContext(ctx, "INSERT INTO worktrees(id,project,path,main) VALUES(?1,NULL,?2,0)", value.Worktree.String(), core.SessionWorkspace(profile, value.ID)); err != nil {
			return protocol.Output{}, nil, err
		}
	}
	value.CreationOrder, err = NextCounter(ctx, db, CounterSessions)
	if err != nil {
		return protocol.Output{}, nil, err
	}
	var project any
	if input.Project != nil {
		project = input.Project.String()
	}
	if _, err := db.ExecContext(ctx, "INSERT INTO sessions(rowid,id,project,worktree,revision) VALUES(?1,?2,?3,?4,1)", value.CreationOrder, value.ID.String(), project, value.Worktree.String()); err != nil {
		return protocol.Output{}, nil, err
	}
	if err := insertSessionRevision(ctx, db, value); err != nil {
		return protocol.Output{}, nil, err
	}
	if _, err := db.ExecContext(ctx, "INSERT INTO session_media(session,body) VALUES(?1,?2)", value.ID.String(), []byte("{}")); err != nil {
		return protocol.Output{}, nil, err
	}
	return protocol.Output{Kind: "session", Data: value}, []protocol.Event{{Kind: "session_changed", Data: &value}}, nil
}

func checkedSession(ctx context.Context, db *sql.Conn, id protocol.SessionID, expected uint64) (protocol.Session, error) {
	value, err := ReadSession(ctx, db, id)
	if err != nil {
		return value, err
	}
	if value.Delegation != nil {
		return value, fault(protocol.ErrorCodePermissionDenied, "child conversation is read-only")
	}
	return value, checkRevision(value.Revision, expected)
}

func renameSession(ctx context.Context, db *sql.Conn, input protocol.CommandRenameSessionPayload) (protocol.Output, []protocol.Event, error) {
	value, err := checkedSession(ctx, db, input.Session, input.ExpectedRevision)
	if err != nil {
		return protocol.Output{}, nil, err
	}
	title := strings.TrimSpace(input.Title)
	if title == "" || strings.ContainsFunc(title, unicode.IsControl) {
		return protocol.Output{}, nil, invalid("session title must be nonempty and contain no control characters")
	}
	body, err := encode(title)
	if err != nil {
		return protocol.Output{}, nil, err
	}
	if _, err := db.ExecContext(ctx, "INSERT INTO agent_state(owner,key,body) VALUES(?1,?2,?3) ON CONFLICT(owner,key) DO UPDATE SET body=excluded.body", value.ID.String(), sessionTitleKey, body); err != nil {
		return protocol.Output{}, nil, err
	}
	value.Activity.Title = shortTitle(title)
	return reviseSession(ctx, db, value)
}

func shortTitle(title string) string {
	text := []rune(strings.Join(strings.Fields(title), " "))
	filtered := text[:0]
	for _, r := range text {
		if !unicode.IsControl(r) {
			filtered = append(filtered, r)
		}
	}
	if len(filtered) <= 48 {
		return string(filtered)
	}
	return strings.TrimRightFunc(string(filtered[:47]), unicode.IsSpace) + "…"
}

func archiveSession(ctx context.Context, db *sql.Conn, input protocol.CommandSetSessionArchivedPayload) (protocol.Output, []protocol.Event, error) {
	value, err := checkedSession(ctx, db, input.Session, input.ExpectedRevision)
	if err != nil {
		return protocol.Output{}, nil, err
	}
	state := "active"
	if input.Archived {
		state = "archived"
	}
	if _, err := db.ExecContext(ctx, "UPDATE sessions SET state=?2 WHERE id=?1", input.Session.String(), state); err != nil {
		return protocol.Output{}, nil, err
	}
	value.Archived = input.Archived
	return reviseSession(ctx, db, value)
}

func setSessionRead(ctx context.Context, db *sql.Conn, input protocol.CommandSetSessionReadPayload) (protocol.Output, []protocol.Event, error) {
	value, err := ReadSession(ctx, db, input.Session)
	if err != nil {
		return protocol.Output{}, nil, err
	}
	attention := value.Activity.Attention
	if attention.Unread != input.Read {
		return protocol.Output{Kind: "session", Data: value}, nil, nil
	}
	if err := checkRevision(attention.Revision, input.ExpectedRevision); err != nil {
		return protocol.Output{}, nil, err
	}
	if _, err := db.ExecContext(ctx, "INSERT INTO session_attention(session,revision,unread) VALUES(?1,1,?2) ON CONFLICT(session) DO UPDATE SET revision=revision+1,unread=excluded.unread", input.Session.String(), !input.Read); err != nil {
		return protocol.Output{}, nil, err
	}
	value.Activity.Attention = protocol.ActivityAttention{Revision: attention.Revision + 1, Unread: !input.Read}
	return protocol.Output{Kind: "session", Data: value}, []protocol.Event{{Kind: "conversation_changed", Data: protocol.EventConversationChangedPayload{Session: value.ID, Activity: &value.Activity}}}, nil
}

func removeSession(ctx context.Context, db *sql.Conn, id protocol.SessionID, expected uint64) (protocol.Output, []protocol.Event, error) {
	if _, err := checkedSession(ctx, db, id, expected); err != nil {
		return protocol.Output{}, nil, err
	}
	ids := []protocol.SessionID{id}
	seen := map[protocol.SessionID]bool{id: true}
	for index := 0; index < len(ids); index++ {
		rows, err := db.QueryContext(ctx, "SELECT child.session FROM agent_delegations d JOIN turns child ON child.id=d.turn JOIN turns parent ON parent.id=d.parent_turn WHERE parent.session=?1", ids[index].String())
		if err != nil {
			return protocol.Output{}, nil, err
		}
		for rows.Next() {
			var text string
			if err := rows.Scan(&text); err != nil {
				rows.Close()
				return protocol.Output{}, nil, err
			}
			child, err := protocol.ParseSessionID(text)
			if err != nil {
				rows.Close()
				return protocol.Output{}, nil, err
			}
			if !seen[child] {
				seen[child] = true
				ids = append(ids, child)
			}
		}
		if err := errors.Join(rows.Err(), rows.Close()); err != nil {
			return protocol.Output{}, nil, err
		}
	}
	for _, id := range ids {
		var busy bool
		if err := db.QueryRowContext(ctx, "SELECT EXISTS(SELECT 1 FROM agent_runs a JOIN turns t ON t.id=a.turn WHERE t.session=?1 AND json_extract(a.body,'$.status') IN ('queued','running','stopping'))", id.String()).Scan(&busy); err != nil {
			return protocol.Output{}, nil, err
		}
		if busy {
			return protocol.Output{}, nil, fault(protocol.ErrorCodeBusy, "stop the session before deleting it")
		}
	}
	for _, id := range ids {
		if _, err := db.ExecContext(ctx, "UPDATE sessions SET state='removed' WHERE id=?1", id.String()); err != nil {
			return protocol.Output{}, nil, err
		}
	}
	return protocol.Output{Kind: "sessions_removed", Data: ids}, []protocol.Event{{Kind: "sessions_removed", Data: ids}}, nil
}

func batchSessions(ctx context.Context, db *sql.Conn, expected []protocol.SessionRevision, removing bool) (protocol.Output, []protocol.Event, error) {
	seen := map[protocol.SessionID]bool{}
	values := make([]protocol.Session, 0, len(expected))
	for _, entry := range expected {
		if seen[entry.Session] {
			return protocol.Output{}, nil, invalid("session batch contains duplicate sessions")
		}
		seen[entry.Session] = true
		value, err := checkedSession(ctx, db, entry.Session, entry.Revision)
		if err != nil {
			return protocol.Output{}, nil, err
		}
		values = append(values, value)
	}
	changes := []protocol.Event{}
	archived := []protocol.Session{}
	removed := []protocol.SessionID{}
	for _, value := range values {
		if removing {
			output, events, err := removeSession(ctx, db, value.ID, value.Revision)
			if err != nil {
				return protocol.Output{}, nil, err
			}
			removed = append(removed, output.Data.([]protocol.SessionID)...)
			changes = append(changes, events...)
		} else if value.Archived {
			archived = append(archived, value)
		} else {
			output, events, err := archiveSession(ctx, db, protocol.CommandSetSessionArchivedPayload{Session: value.ID, ExpectedRevision: value.Revision, Archived: true})
			if err != nil {
				return protocol.Output{}, nil, err
			}
			archived = append(archived, output.Data.(protocol.Session))
			changes = append(changes, events...)
		}
	}
	if removing {
		return protocol.Output{Kind: "sessions_removed", Data: removed}, changes, nil
	}
	return protocol.Output{Kind: "sessions_archived", Data: archived}, changes, nil
}

func reorderSessions(ctx context.Context, db *sql.Conn, input protocol.CommandSetSessionOrderPayload) (protocol.Output, []protocol.Event, error) {
	values, err := ReadSessions(ctx, db)
	if err != nil {
		return protocol.Output{}, nil, err
	}
	order := make([]protocol.SessionID, 0, len(values))
	current := []protocol.SessionID{}
	members := map[protocol.SessionID]bool{}
	for _, value := range values {
		order = append(order, value.ID)
		if value.Project != nil && *value.Project == input.Project {
			current = append(current, value.ID)
			members[value.ID] = true
		}
	}
	if len(members) == 0 {
		return protocol.Output{}, nil, fault(protocol.ErrorCodeNotFound, "project has no sessions")
	}
	valid := len(input.Sessions) == len(members)
	seen := map[protocol.SessionID]bool{}
	for _, id := range input.Sessions {
		if !members[id] || seen[id] {
			valid = false
		}
		seen[id] = true
	}
	if !valid {
		return protocol.Output{}, nil, invalid("session order must contain each project session exactly once")
	}
	if !slices.Equal(current, input.Expected) {
		return protocol.Output{}, nil, fault(protocol.ErrorCodeRevisionConflict, "session order changed")
	}
	if slices.Equal(current, input.Sessions) {
		return protocol.Output{Kind: "session_order", Data: order}, nil, nil
	}
	index := 0
	for position, id := range order {
		if members[id] {
			order[position] = input.Sessions[index]
			index++
		}
	}
	return protocol.Output{Kind: "session_order", Data: order}, []protocol.Event{{Kind: "sessions_reordered", Data: order}}, nil
}
