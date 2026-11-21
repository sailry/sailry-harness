package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// Audiences are captured with the event, including removals and old ownership.
// Current project visibility is checked again on delivery after access changes.
type eventAudience struct {
	Kind    string                   `json:"kind"`
	Owner   *protocol.UserID         `json:"owner,omitempty"`
	Project *protocol.ProjectID      `json:"project,omitempty"`
	Shared  bool                     `json:"shared,omitempty"`
	Members map[string]eventAudience `json:"members,omitempty"`
}

func personalAudience(ctx context.Context, db *sql.Conn) (eventAudience, error) {
	user, err := CurrentUser(ctx, db)
	return eventAudience{Kind: "user", Owner: &user.ID}, err
}
func projectAudience(ctx context.Context, db *sql.Conn, id protocol.ProjectID) (eventAudience, error) {
	value := eventAudience{Kind: "project", Project: &id}
	var owner string
	err := db.QueryRowContext(ctx, "SELECT coalesce(owner,(SELECT id FROM users WHERE local=1)),visibility='shared' FROM projects WHERE id=?1", id.String()).Scan(&owner, &value.Shared)
	if err != nil {
		return value, err
	}
	user, err := protocol.ParseUserID(owner)
	value.Owner = &user
	return value, err
}
func resourceAudience(ctx context.Context, db *sql.Conn, kind, id string) (eventAudience, error) {
	var query string
	switch kind {
	case "session":
		query = "SELECT owner,project FROM sessions WHERE id=?1"
	case "worktree":
		query = "SELECT owner,project FROM worktrees WHERE id=?1"
	case "terminal":
		query = "SELECT coalesce(t.owner,w.owner),w.project FROM terminals t LEFT JOIN worktrees w ON w.id=t.worktree WHERE t.id=?1"
	case "notification":
		query = "SELECT coalesce(s.owner,n.owner),s.project FROM notifications n LEFT JOIN sessions s ON s.id=json_extract(n.body,'$.content.session') WHERE n.id=?1"
	default:
		return eventAudience{}, errors.New("invalid event resource")
	}
	var owner, project sql.NullString
	err := db.QueryRowContext(ctx, query, id).Scan(&owner, &project)
	if errors.Is(err, sql.ErrNoRows) {
		var body []byte
		err = db.QueryRowContext(ctx, "SELECT audience FROM removed_resource_audiences WHERE kind=?1 AND id=?2", kind, id).Scan(&body)
		if err == nil {
			var value eventAudience
			err = json.Unmarshal(body, &value)
			return value, err
		}
		if errors.Is(err, sql.ErrNoRows) {
			return personalAudience(ctx, db)
		}
	}
	if err != nil {
		return eventAudience{}, err
	}
	if project.Valid {
		id, err := protocol.ParseProjectID(project.String)
		if err != nil {
			return eventAudience{}, err
		}
		return projectAudience(ctx, db, id)
	}
	if owner.Valid {
		id, err := protocol.ParseUserID(owner.String)
		return eventAudience{Kind: "user", Owner: &id}, err
	}
	local, err := LocalUser(ctx, db)
	return eventAudience{Kind: "user", Owner: &local.ID}, err
}
func retainResourceAudience(ctx context.Context, db *sql.Conn, kind, id string) error {
	audience, err := resourceAudience(ctx, db, kind, id)
	if err != nil {
		return err
	}
	body, err := json.Marshal(audience)
	if err != nil {
		return err
	}
	_, err = db.ExecContext(ctx, "INSERT INTO removed_resource_audiences(kind,id,audience) VALUES(?1,?2,?3) ON CONFLICT(kind,id) DO UPDATE SET audience=excluded.audience", kind, id, body)
	return err
}

func captureEventAudience(ctx context.Context, db *sql.Conn, event protocol.Event) (eventAudience, error) {
	// Project changes invalidate every projection. No project fields are sent to
	// a user losing access; a fresh filtered snapshot removes cached descendants.
	switch event.Kind {
	case "project_registered", "project_changed", "project_removed":
		return eventAudience{Kind: "reset"}, nil
	case "provider_changed", "provider_removed", "provider_quota_changed":
		return eventAudience{Kind: "reset"}, nil
	case "knowledge_collections_changed", "projection_invalidated":
		return eventAudience{Kind: "reset"}, nil
	case "model_catalog_changed":
		return eventAudience{Kind: "host"}, nil
	}
	if event.Kind == "plugin_values_changed" {
		var payload *protocol.EventPluginValuesChangedPayload
		switch value := event.Data.(type) {
		case protocol.EventPluginValuesChangedPayload:
			payload = &value
		case *protocol.EventPluginValuesChangedPayload:
			payload = value
		}
		if payload != nil && payload.Project != nil {
			return projectAudience(ctx, db, *payload.Project)
		}
	}
	body, err := json.Marshal(event)
	if err != nil {
		return eventAudience{}, err
	}
	var envelope struct {
		Data json.RawMessage `json:"data"`
	}
	if err = json.Unmarshal(body, &envelope); err != nil {
		return eventAudience{}, err
	}
	var data struct {
		ID      string          `json:"id"`
		Session json.RawMessage `json:"session"`
		Turn    struct {
			Session protocol.SessionID `json:"session"`
		} `json:"turn"`
	}
	switch event.Kind {
	case "sessions_removed", "sessions_reordered":
		var ids []protocol.SessionID
		if err = json.Unmarshal(envelope.Data, &ids); err != nil {
			return eventAudience{}, err
		}
		a := eventAudience{Kind: "members", Members: map[string]eventAudience{}}
		for _, id := range ids {
			scope, err := resourceAudience(ctx, db, "session", id.String())
			if err != nil {
				return a, err
			}
			a.Members[id.String()] = scope
		}
		return a, nil
	case "notifications_read", "notifications_dismissed":
		var v struct {
			IDs []protocol.NotificationID `json:"ids"`
		}
		if err = json.Unmarshal(envelope.Data, &v); err != nil {
			return eventAudience{}, err
		}
		a := eventAudience{Kind: "members", Members: map[string]eventAudience{}}
		for _, id := range v.IDs {
			scope, err := resourceAudience(ctx, db, "notification", id.String())
			if err != nil {
				return a, err
			}
			a.Members[id.String()] = scope
		}
		return a, nil
	}
	if err = json.Unmarshal(envelope.Data, &data); err != nil {
		return personalAudience(ctx, db)
	}
	switch event.Kind {
	case "session_changed":
		return resourceAudience(ctx, db, "session", data.ID)
	case "terminal_changed":
		return resourceAudience(ctx, db, "terminal", data.ID)
	case "worktree_registered", "worktree_removed":
		return resourceAudience(ctx, db, "worktree", data.ID)
	case "notification_changed":
		return resourceAudience(ctx, db, "notification", data.ID)
	case "task_started", "turn_queued":
		return resourceAudience(ctx, db, "session", data.Turn.Session.String())
	case "conversation_changed", "tools_completed":
		var id protocol.SessionID
		if err = json.Unmarshal(data.Session, &id); err != nil {
			return eventAudience{}, err
		}
		return resourceAudience(ctx, db, "session", id.String())
	case "session_rewound":
		var session protocol.Session
		if err = json.Unmarshal(data.Session, &session); err != nil {
			return eventAudience{}, err
		}
		return resourceAudience(ctx, db, "session", session.ID.String())
	}
	return personalAudience(ctx, db)
}

type eventVisibility struct {
	ctx      context.Context
	db       *sql.Conn
	user     protocol.User
	owners   map[protocol.UserID]bool
	projects map[protocol.ProjectID]sql.NullBool
}

func (v *eventVisibility) owns(id *protocol.UserID) (bool, error) {
	if id == nil {
		return false, nil
	}
	if *id == v.user.ID && v.user.State == "active" {
		return true, nil
	}
	if owned, ok := v.owners[*id]; ok {
		return owned, nil
	}
	owner, err := ResolveUser(v.ctx, v.db, *id)
	if err != nil {
		return false, err
	}
	owned := owner.ID == v.user.ID
	if v.owners != nil {
		v.owners[*id] = owned
	}
	return owned, nil
}

func (v *eventVisibility) visible(a eventAudience) (bool, error) {
	switch a.Kind {
	case "host", "reset":
		return true, nil
	case "project":
		if a.Project == nil {
			return false, nil
		}
		owned, err := v.owns(a.Owner)
		if err != nil {
			return false, err
		}
		if !a.Shared && !owned {
			return false, nil
		}
		current, ok := v.projects[*a.Project]
		if !ok {
			err = v.db.QueryRowContext(v.ctx, "SELECT visibility='shared' OR coalesce(owner,(SELECT id FROM users WHERE local=1))=?2 FROM projects WHERE id=?1", a.Project.String(), v.user.ID.String()).Scan(&current.Bool)
			if err != nil && !errors.Is(err, sql.ErrNoRows) {
				return false, err
			}
			current.Valid = err == nil
			if v.projects != nil {
				v.projects[*a.Project] = current
			}
		}
		if !current.Valid {
			return owned, nil
		}
		return current.Bool, nil
	case "user":
		return v.owns(a.Owner)
	}
	return false, nil
}

func filterEvent(ctx context.Context, db *sql.Conn, event protocol.Event, a eventAudience, user protocol.User) (protocol.Event, error) {
	if a.Kind == "reset" {
		return protocol.Event{Kind: "projection_invalidated"}, nil
	}
	visibility := eventVisibility{ctx: ctx, db: db, user: user}
	if a.Kind == "members" {
		// Members share this event's delivery snapshot, never a later cursor's.
		visibility.owners = make(map[protocol.UserID]bool)
		visibility.projects = make(map[protocol.ProjectID]sql.NullBool)
		switch event.Kind {
		case "sessions_removed", "sessions_reordered":
			body, err := json.Marshal(event.Data)
			if err != nil {
				return protocol.Event{}, err
			}
			var ids []protocol.SessionID
			if err = json.Unmarshal(body, &ids); err != nil {
				return protocol.Event{}, err
			}
			filtered := make([]protocol.SessionID, 0, len(ids))
			for _, id := range ids {
				visible, err := visibility.visible(a.Members[id.String()])
				if err != nil {
					return protocol.Event{}, err
				}
				if visible {
					filtered = append(filtered, id)
				}
			}
			if len(filtered) > 0 {
				return protocol.Event{Kind: event.Kind, Data: filtered}, nil
			}
		case "notifications_read", "notifications_dismissed":
			body, err := json.Marshal(event.Data)
			if err != nil {
				return protocol.Event{}, err
			}
			var payload struct {
				IDs []protocol.NotificationID `json:"ids"`
			}
			if err = json.Unmarshal(body, &payload); err != nil {
				return protocol.Event{}, err
			}
			filtered := make([]protocol.NotificationID, 0, len(payload.IDs))
			for _, id := range payload.IDs {
				visible, err := visibility.visible(a.Members[id.String()])
				if err != nil {
					return protocol.Event{}, err
				}
				if visible {
					filtered = append(filtered, id)
				}
			}
			if len(filtered) > 0 {
				if event.Kind == "notifications_read" {
					return protocol.Event{Kind: event.Kind, Data: protocol.EventNotificationsReadPayload{IDs: filtered}}, nil
				}
				return protocol.Event{Kind: event.Kind, Data: protocol.EventNotificationsDismissedPayload{IDs: filtered}}, nil
			}
		}
		return protocol.Event{Kind: "cursor"}, nil
	}
	visible, err := visibility.visible(a)
	if err != nil {
		return protocol.Event{}, err
	}
	if !visible {
		return protocol.Event{Kind: "cursor"}, nil
	}
	return event, nil
}

// NextEvent pins only a cursor watermark across deliveries. The next row and
// its current audience are read together in the caller's fresh transaction.
func NextEvent(ctx context.Context, db *sql.Conn, node protocol.NodeID, after, through uint64) (uint64, *protocol.EventEnvelope, error) {
	if after >= through {
		err := db.QueryRowContext(ctx, "SELECT cursor FROM events WHERE cursor>?1 ORDER BY cursor DESC LIMIT 1", after).Scan(&through)
		if errors.Is(err, sql.ErrNoRows) {
			return after, nil, nil
		}
		if err != nil {
			return through, nil, err
		}
	}
	event, err := RefreshEventAccess(ctx, db, protocol.EventEnvelope{Node: node, Cursor: after + 1})
	if err != nil {
		return through, nil, err
	}
	return through, &event, nil
}

// RefreshEventAccess reads the canonical row by cursor and filters it for the current recipient.
func RefreshEventAccess(ctx context.Context, db *sql.Conn, event protocol.EventEnvelope) (protocol.EventEnvelope, error) {
	var body, scope []byte
	if err := db.QueryRowContext(ctx, "SELECT body,audience FROM events WHERE cursor=?1", event.Cursor).Scan(&body, &scope); err != nil {
		return event, err
	}
	if err := json.Unmarshal(body, &event.Event); err != nil {
		return event, err
	}
	var audience eventAudience
	if scope == nil {
		local, err := LocalUser(ctx, db)
		if err != nil {
			return event, err
		}
		audience = eventAudience{Kind: "user", Owner: &local.ID}
	} else if err := json.Unmarshal(scope, &audience); err != nil {
		return event, err
	}
	user, err := CurrentUser(ctx, db)
	if err != nil {
		return event, err
	}
	event.Event, err = filterEvent(ctx, db, event.Event, audience, user)
	return event, err
}
