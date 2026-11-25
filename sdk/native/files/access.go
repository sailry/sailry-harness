package files

import (
	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/protocol"
)

const ManualFiles = core.ManualFiles

func ManagedWorkspace(root, profile string) bool { return core.ManagedWorkspace(root, profile) }

func ConnectionWorkspace(profile string, resource protocol.ConnectionResource) (string, error) {
	return core.ConnectionWorkspace(profile, resource)
}

func FileAccessForRequest(request protocol.Request, restricted bool) FileAccess {
	return core.FileAccessForRequest(request, restricted)
}
