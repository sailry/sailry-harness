package desktop

import (
	"context"

	"github.com/sailry/sailry-harness/internal/protocol"
	"github.com/sailry/sailry-harness/sdk/desktop/native"
)

var helperGrants = [...]struct {
	kind       native.PermissionKind
	permission protocol.ComputerPermission
	authorized func(protocol.ComputerPermissions) *bool
}{
	{native.PermissionKindAccessibility, protocol.ComputerPermissionAccessibility, func(p protocol.ComputerPermissions) *bool { return p.Accessibility }},
	{native.PermissionKindScreenRecording, protocol.ComputerPermissionScreenCapture, func(p protocol.ComputerPermissions) *bool { return p.ScreenCapture }},
}

func permissionContribution() native.PermissionContribution {
	kinds := make([]native.PermissionKind, 0, len(helperGrants))
	for _, grant := range helperGrants {
		kinds = append(kinds, grant.kind)
	}
	return native.PermissionContribution{
		Subject: "dev.sailry.computer",
		Kinds:   kinds,
		Revision: func(snapshot *native.PermissionSnapshot) uint64 {
			return snapshot.ComputerSettingsRevision
		},
		Available: func(ctx context.Context, execute native.PermissionCommand) (bool, error) {
			value, err := permissionPayload[protocol.ComputerSettings](ctx, execute, protocol.Command{Kind: "read_computer_settings"}, "computer_settings")
			return value.Enabled, err
		},
		Check: func(ctx context.Context, execute native.PermissionCommand) (native.PermissionReport, error) {
			return helperPermissionReport(ctx, execute, protocol.Command{Kind: "read_computer_permissions"})
		},
		Request: func(ctx context.Context, execute native.PermissionCommand, kind native.PermissionKind) (native.PermissionReport, error) {
			for _, grant := range helperGrants {
				if grant.kind == kind {
					return helperPermissionReport(ctx, execute, protocol.Command{
						Kind: "request_computer_permission",
						Data: protocol.CommandRequestComputerPermissionPayload{Permission: grant.permission},
					})
				}
			}
			return native.PermissionReport{}, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "unknown computer helper permission"}
		},
	}
}

func helperPermissionReport(ctx context.Context, execute native.PermissionCommand, command protocol.Command) (native.PermissionReport, error) {
	value, err := permissionPayload[protocol.ComputerPermissions](ctx, execute, command, "computer_permissions")
	if err != nil {
		return native.PermissionReport{}, err
	}
	mac := value.Platform == "macos"
	grants := make(map[native.PermissionKind]native.PermissionGrantState, len(helperGrants))
	for _, grant := range helperGrants {
		authorized := grant.authorized(value)
		switch {
		case !mac:
			grants[grant.kind] = native.PermissionNotNeeded
		case authorized == nil:
			grants[grant.kind] = native.PermissionUnknown
		case *authorized:
			grants[grant.kind] = native.PermissionGranted
		default:
			grants[grant.kind] = native.PermissionRequired
		}
	}
	return native.PermissionReport{Grants: grants, CanRequest: mac && value.Local}, nil
}

func permissionPayload[T any](ctx context.Context, execute native.PermissionCommand, command protocol.Command, kind string) (T, error) {
	var zero T
	out, err := execute(ctx, command)
	if err != nil {
		return zero, err
	}
	if out.Kind == kind {
		switch value := out.Data.(type) {
		case T:
			return value, nil
		case *T:
			if value != nil {
				return *value, nil
			}
		}
	}
	return zero, &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: "invalid computer permission response"}
}
