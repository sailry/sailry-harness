package store

import (
	"context"
	"database/sql"
	"errors"
	"math"
	"slices"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// ReadCodeIndexSettings returns the saved enablement, enabled by default.
func ReadCodeIndexSettings(ctx context.Context, db *sql.Conn) (protocol.CodeIndexSettings, error) {
	value, err := readBody[protocol.CodeIndexSettings](ctx, db, "SELECT body FROM code_index_settings WHERE singleton=1")
	if err != nil {
		return protocol.CodeIndexSettings{}, err
	}
	if value == nil {
		return protocol.CodeIndexSettings{Enabled: true}, nil
	}
	return *value, nil
}

// ViewCodeIndexSettings exposes only selections the requesting administrator
// can manage. Every user can still read whether the code tools are available.
func ViewCodeIndexSettings(ctx context.Context, db *sql.Conn) (protocol.CodeIndexSettings, error) {
	value, err := ReadCodeIndexSettings(ctx, db)
	if err != nil {
		return value, err
	}
	value.PersistentProjects, err = visibleCodeIndexProjects(ctx, db, value.PersistentProjects)
	return value, err
}

func visibleCodeIndexProjects(ctx context.Context, db *sql.Conn, projects *[]protocol.ProjectID) (*[]protocol.ProjectID, error) {
	user, err := CurrentUser(ctx, db)
	if err != nil || !user.Administrator {
		return nil, err
	}
	if projects == nil {
		return &[]protocol.ProjectID{}, nil
	}
	visible := make([]protocol.ProjectID, 0, len(*projects))
	for _, project := range *projects {
		_, manageable, err := codeIndexProjectAccess(ctx, db, project, user)
		if err != nil {
			return nil, err
		}
		if manageable {
			visible = append(visible, project)
		}
	}
	return &visible, nil
}

// SaveCodeIndexSettings replaces the settings when both the edited value and
// the stored one carry the expected revision.
func SaveCodeIndexSettings(ctx context.Context, db *sql.Conn, input protocol.CodeIndexSettings, expected uint64) (protocol.CodeIndexSettings, error) {
	current, err := ReadCodeIndexSettings(ctx, db)
	if err != nil {
		return protocol.CodeIndexSettings{}, err
	}
	if input.Revision != expected || current.Revision != expected {
		return protocol.CodeIndexSettings{}, fault(protocol.ErrorCodeRevisionConflict, "code index settings changed before saving")
	}
	if expected >= math.MaxInt64 {
		return protocol.CodeIndexSettings{}, invalid("code index revision is exhausted")
	}
	visible := input.PersistentProjects
	if visible == nil {
		input.PersistentProjects = current.PersistentProjects
		visible, err = visibleCodeIndexProjects(ctx, db, input.PersistentProjects)
	} else {
		input.PersistentProjects, visible, err = selectedCodeIndexProjects(ctx, db, *visible, current.PersistentProjects)
	}
	if err != nil {
		return protocol.CodeIndexSettings{}, err
	}
	input.Revision = expected + 1
	body, err := encode(input)
	if err != nil {
		return protocol.CodeIndexSettings{}, err
	}
	_, err = db.ExecContext(ctx, "INSERT INTO code_index_settings(singleton,body) VALUES(1,?1) ON CONFLICT(singleton) DO UPDATE SET body=excluded.body", body)
	input.PersistentProjects = visible
	return input, err
}

const maxPersistentCodeIndexProjects = 128

func selectedCodeIndexProjects(ctx context.Context, db *sql.Conn, requested []protocol.ProjectID, current *[]protocol.ProjectID) (total, visible *[]protocol.ProjectID, err error) {
	if len(requested) > maxPersistentCodeIndexProjects {
		return nil, nil, invalid("too many projects selected for persistent code indexing")
	}
	user, err := CurrentUser(ctx, db)
	if err != nil {
		return nil, nil, err
	}
	if !user.Administrator {
		return nil, nil, fault(protocol.ErrorCodePermissionDenied, "persistent code indexing requires a machine administrator")
	}
	selected := make([]protocol.ProjectID, 0, len(requested))
	seen := make(map[protocol.ProjectID]bool, len(requested))
	for _, project := range requested {
		if seen[project] {
			return nil, nil, invalid("persistent code index projects must be unique")
		}
		seen[project] = true
		previous := current != nil && slices.Contains(*current, project)
		registered, manageable, err := codeIndexProjectAccess(ctx, db, project, user)
		if err != nil {
			return nil, nil, err
		}
		if !registered && previous {
			continue // A project removed since the settings were read is no longer a selection.
		}
		if !manageable {
			return nil, nil, invalid("persistent code indexing requires a registered, manageable project")
		}
		selected = append(selected, project)
	}
	all := slices.Clone(selected)
	if current != nil {
		for _, project := range *current {
			if seen[project] {
				continue
			}
			registered, manageable, err := codeIndexProjectAccess(ctx, db, project, user)
			if err != nil {
				return nil, nil, err
			}
			if registered && !manageable {
				all = append(all, project)
			}
		}
	}
	if len(all) > maxPersistentCodeIndexProjects {
		return nil, nil, invalid("too many projects selected for persistent code indexing")
	}
	return &all, &selected, nil
}

func codeIndexProjectAccess(ctx context.Context, db *sql.Conn, project protocol.ProjectID, user protocol.User) (registered, manageable bool, err error) {
	err = db.QueryRowContext(ctx, `SELECT coalesce(owner,(SELECT id FROM users WHERE local=1))=?2 OR visibility='shared' FROM projects WHERE id=?1 AND id NOT IN (SELECT project FROM removed_projects)`, project.String(), user.ID.String()).Scan(&manageable)
	if errors.Is(err, sql.ErrNoRows) {
		return false, false, nil
	}
	return err == nil, manageable, err
}
