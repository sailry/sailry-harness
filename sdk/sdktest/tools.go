package sdktest

import (
	"context"
	"database/sql"
	"encoding/json"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/node/admission"
	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/node/agent/operations"
	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type ToolResponse = canonical.ToolResponse

type Declaration = operations.Declaration

type Spec = operations.Spec

type ConnectionArguments = operations.ConnectionArguments

const (
	ToolSuccess     = canonical.ToolSuccess
	ToolErrorStatus = canonical.ToolErrorStatus
	ManifestSchema  = plugins.ManifestSchema
)

func Alias(pkg, name string) string { return operations.Alias(pkg, name) }

func Policy(operation protocol.ToolOperation) (bool, bool) { return operations.Policy(operation) }

func ConnectionOperation(operation protocol.ToolOperation) bool {
	return operations.ConnectionOperation(operation)
}

func ConnectionCall(operation protocol.ToolOperation, arguments json.RawMessage) (ConnectionArguments, error) {
	return operations.ConnectionCall(operation, arguments)
}

func RetiredBundled() []string { return plugins.RetiredBundled() }

func PackageReference(info protocol.PluginInfo) protocol.PluginReference {
	return plugins.PackageReference(info)
}

func Admit(ctx context.Context, db *sql.Conn, caller protocol.NodeID, request protocol.Request, encoded []byte) (*link.Response, error) {
	return admission.Admit(ctx, db, caller, request, encoded)
}
