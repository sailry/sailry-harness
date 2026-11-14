package store

import "github.com/sailry/sailry-harness/internal/protocol"

// withinSessionPermissions applies the same ceiling to reused children and
// sessions created by callbacks captured from an existing conversation.
func withinSessionPermissions(source, target protocol.SessionConfig) bool {
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
	return (source.Mode != protocol.WorkModePlan || target.Mode == protocol.WorkModePlan) && permission(target.Permission) <= permission(source.Permission)
}
