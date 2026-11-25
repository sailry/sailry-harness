package shell

import (
	"context"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// PermissionContribution declares the grants of one native subject on the
// managed Host. An enabled package declaring that subject takes precedence.
type PermissionContribution struct {
	Subject string
	Kinds   []protocol.PermissionsKind
	// Revision derives the availability revision from a read-only Host snapshot.
	// It is needed only when Available should follow changes while the page is open.
	Revision  func(*protocol.Snapshot) uint64
	Available func(context.Context, PermissionCommand) (bool, error)
	// Check returns all of the subject's grants in one read.
	Check func(context.Context, PermissionCommand) (PermissionReport, error)
	// Request is optional. The window calls it only on the local Host, after
	// Check reports that the selected grant can still be requested.
	Request func(context.Context, PermissionCommand, protocol.PermissionsKind) (PermissionReport, error)
}

// PermissionCommand executes an ordinary controller command on the selected Host.
type PermissionCommand func(context.Context, protocol.Command) (protocol.Output, error)

type PermissionReport struct {
	Grants     map[protocol.PermissionsKind]PermissionGrantState
	CanRequest bool
}

type PermissionGrantState string

const (
	PermissionUnknown     PermissionGrantState = "permission_unknown"
	PermissionGranted     PermissionGrantState = "permission_granted"
	PermissionRequired    PermissionGrantState = "permission_required"
	PermissionNotNeeded   PermissionGrantState = "permission_not_needed"
	PermissionUnavailable PermissionGrantState = "permission_unavailable"
)
