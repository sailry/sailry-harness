package store

import (
	"context"
	"database/sql"
	"errors"
	"slices"

	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// connectionIdentity returns a comparable kind and identifier. Resource data
// is a value in constructed commands and a pointer after decoding.
func connectionIdentity(resource protocol.ConnectionResource) (string, string, bool) {
	switch resource.Kind {
	case "database":
		if id, valid := dispatchValue[protocol.DatabaseID](resource.Data); valid {
			return resource.Kind, id.String(), true
		}
	case "ssh":
		if id, valid := dispatchValue[protocol.SshID](resource.Data); valid {
			return resource.Kind, id.String(), true
		}
	}
	return "", "", false
}

func sameConnection(left, right protocol.ConnectionResource) bool {
	leftKind, leftID, leftValid := connectionIdentity(left)
	rightKind, rightID, rightValid := connectionIdentity(right)
	return leftValid && rightValid && leftKind == rightKind && leftID == rightID
}

func containsConnection(resources []protocol.ConnectionResource, target protocol.ConnectionResource) bool {
	return slices.ContainsFunc(resources, func(candidate protocol.ConnectionResource) bool { return sameConnection(candidate, target) })
}

func databaseResource(id protocol.DatabaseID) protocol.ConnectionResource {
	return protocol.ConnectionResource{Kind: "database", Data: id}
}

func sshResource(id protocol.SshID) protocol.ConnectionResource {
	return protocol.ConnectionResource{Kind: "ssh", Data: id}
}

// connectionSharing returns the saved sharing rule of a resource and whether
// the resource exists.
func connectionSharing(ctx context.Context, db *sql.Conn, resource protocol.ConnectionResource) (*protocol.ConnectionSharing, bool, error) {
	switch resource.Kind {
	case "database":
		if id, valid := dispatchValue[protocol.DatabaseID](resource.Data); valid {
			profile, err := ReadDatabase(ctx, db, id)
			if err != nil || profile == nil {
				return nil, false, err
			}
			return profile.Sharing, true, nil
		}
	case "ssh":
		if id, valid := dispatchValue[protocol.SshID](resource.Data); valid {
			profile, err := readBody[protocol.SshProfile](ctx, db, "SELECT body FROM ssh_profiles WHERE id=?1", id.String())
			if err != nil || profile == nil {
				return nil, false, err
			}
			return profile.Sharing, true, nil
		}
	}
	return nil, false, invalid("invalid connection resource")
}

// ConnectionPermits is the execution Node's sharing decision. A bound
// conversation reaches only its own resource; a project conversation reaches
// resources shared globally or with its project; any other conversation
// reaches none.
func ConnectionPermits(bound *protocol.ConnectionResource, project *protocol.ProjectID, target protocol.ConnectionResource, sharing *protocol.ConnectionSharing) bool {
	if bound != nil {
		return sameConnection(*bound, target)
	}
	if project == nil || sharing == nil {
		return false
	}
	switch sharing.Kind {
	case "global":
		return true
	case "projects":
		projects, _ := dispatchValue[[]protocol.ProjectID](sharing.Data)
		return slices.Contains(projects, *project)
	}
	return false
}

// connectionWorkspace returns the one scratch worktree every conversation
// bound to a saved connection shares, registering it on first use.
func connectionWorkspace(ctx context.Context, db *sql.Conn, profile string, resource protocol.ConnectionResource) (protocol.WorktreeID, error) {
	_, exists, err := connectionSharing(ctx, db, resource)
	if err != nil {
		return protocol.WorktreeID{}, err
	}
	if !exists {
		return protocol.WorktreeID{}, fault(protocol.ErrorCodeNotFound, "connection is unavailable")
	}
	if profile == "" {
		return protocol.WorktreeID{}, fault(protocol.ErrorCodeNotConfigured, "connection workspace storage is unavailable")
	}
	root, err := core.ConnectionWorkspace(profile, resource)
	if err != nil {
		return protocol.WorktreeID{}, err
	}
	var existing string
	err = db.QueryRowContext(ctx, "SELECT id FROM worktrees WHERE project IS NULL AND path=?1", root).Scan(&existing)
	if err == nil {
		return protocol.ParseWorktreeID(existing)
	}
	if !errors.Is(err, sql.ErrNoRows) {
		return protocol.WorktreeID{}, err
	}
	id := protocol.NewWorktreeID()
	_, err = db.ExecContext(ctx, "INSERT INTO worktrees(id,project,path,main) VALUES(?1,NULL,?2,0)", id.String(), root)
	return id, err
}
