package operations

import (
	"slices"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// A flow step has no approval of its own, so the operations a flow may declare
// are exactly the read-only ones, the package's own values and a declared
// request. Progress is the model's own report.
func TestFlowOperationsNeedNoApproval(t *testing.T) {
	policies, err := operationPolicies()
	if err != nil {
		t.Fatal(err)
	}
	own := []protocol.ToolOperation{
		protocol.ToolOperationSetValue, protocol.ToolOperationDeleteValue, protocol.ToolOperationStorageTransaction, protocol.ToolOperationHttp,
	}
	for operation, spec := range policies {
		want := spec.ReadOnly && operation != protocol.ToolOperationProgress || slices.Contains(own, operation)
		if got := plugins.FlowOperation(operation); got != want {
			t.Errorf("%s: a flow may declare it = %v, read-only = %v", operation, got, spec.ReadOnly)
		}
	}
	for _, operation := range own {
		if !plugins.FlowOperation(operation) {
			t.Errorf("%s: a flow cannot declare it", operation)
		}
	}
}
