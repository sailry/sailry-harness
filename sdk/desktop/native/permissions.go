package native

import (
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type PermissionContribution = shell.PermissionContribution
type PermissionCommand = shell.PermissionCommand
type PermissionReport = shell.PermissionReport
type PermissionGrantState = shell.PermissionGrantState
type PermissionKind = protocol.PermissionsKind
type PermissionSnapshot = protocol.Snapshot

const (
	PermissionKindFullDisk        = protocol.PermissionsKindFullDisk
	PermissionKindScreenRecording = protocol.PermissionsKindScreenRecording
	PermissionKindAccessibility   = protocol.PermissionsKindAccessibility
	PermissionKindMicrophone      = protocol.PermissionsKindMicrophone

	PermissionUnknown     = shell.PermissionUnknown
	PermissionGranted     = shell.PermissionGranted
	PermissionRequired    = shell.PermissionRequired
	PermissionNotNeeded   = shell.PermissionNotNeeded
	PermissionUnavailable = shell.PermissionUnavailable
)
