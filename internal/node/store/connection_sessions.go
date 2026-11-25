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
			profile, err := readUserConfiguration[protocol.SshProfile](ctx, db, "ssh_profiles", id.String())
			if err != nil || profile == nil {
				return nil, false, err
			}
			return profile.Sharing, true, nil
		}
	}
	return nil, false, invalid("invalid connection resource")
}

func sameConnectionSharing(left, right *protocol.ConnectionSharing) bool {
	if left == nil || right == nil {
		return left == right
	}
	if left.Kind != right.Kind {
		return false
	}
	switch left.Kind {
	case "global":
		return true
	case "projects":
		leftProjects, leftValid := dispatchValue[[]protocol.ProjectID](left.Data)
		rightProjects, rightValid := dispatchValue[[]protocol.ProjectID](right.Data)
		return leftValid && rightValid && slices.Equal(leftProjects, rightProjects)
	default:
		return false
	}
}

func sshResourceOnly(config protocol.SessionConfig) bool {
	return config.Assistant == nil && config.Resource != nil && config.Resource.Kind == "ssh"
}

// ConnectionToolAllowed limits the connection catalog of a native SSH session
// to operations on its bound profile. Other sessions keep their existing tools.
func ConnectionToolAllowed(config protocol.SessionConfig, operation protocol.ToolOperation) bool {
	return !sshResourceOnly(config) || operation == protocol.ToolOperationRunSsh || operation == protocol.ToolOperationTransferSsh
}

// ConnectionPermits is the execution Node's sharing decision. Native resource
// sessions stay on their bound connection. Only a legacy SSH assistant treats
// its binding as a default. Unbound conversations use explicit sharing.
func ConnectionPermits(config protocol.SessionConfig, project *protocol.ProjectID, target protocol.ConnectionResource, sharing *protocol.ConnectionSharing) bool {
	if bound := config.Resource; bound != nil {
		kind, id, valid := connectionIdentity(target)
		primaryKind, primaryID, primaryValid := connectionIdentity(*bound)
		return valid && primaryValid && primaryKind == kind && (primaryID == id || primaryKind == "ssh" && config.Assistant != nil)
	}
	return protocol.ConnectionSharingPermits(project, sharing)
}

func connectionCreationAllowed(config protocol.SessionConfig, kind string) bool {
	return config.Resource == nil || config.Assistant != nil && config.Resource.Kind == "ssh" && kind == "ssh"
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
	user, err := CurrentUser(ctx, db)
	if err != nil {
		return protocol.WorktreeID{}, err
	}
	id := protocol.NewWorktreeID()
	_, err = db.ExecContext(ctx, "INSERT INTO worktrees(id,project,path,main,owner) VALUES(?1,NULL,?2,0,?3)", id.String(), root, user.ID.String())
	return id, err
}
