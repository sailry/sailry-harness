package operations

import (
	"encoding/json"
	"errors"
	"slices"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

var connectionOperations = []protocol.ToolOperation{
	protocol.ToolOperationListDatabaseConnections, protocol.ToolOperationSaveDatabaseConnection, protocol.ToolOperationBrowseDatabase,
	protocol.ToolOperationQueryDatabase, protocol.ToolOperationExecuteDatabase,
	protocol.ToolOperationListSshConnections, protocol.ToolOperationSaveSshConnection, protocol.ToolOperationRunSsh, protocol.ToolOperationTransferSsh,
}

func TestConnectionProtocol(t *testing.T) {
	t.Parallel()
	t.Run("classification", func(t *testing.T) {
		readOnly := []protocol.ToolOperation{protocol.ToolOperationListDatabaseConnections, protocol.ToolOperationListSshConnections,
			protocol.ToolOperationBrowseDatabase, protocol.ToolOperationQueryDatabase}
		for _, operation := range connectionOperations {
			if !ConnectionOperation(operation) {
				t.Errorf("%s is not a connection operation", operation)
			}
			read, concurrent := Policy(operation)
			// Read-only connection operations may run together; writes stay ordered.
			if read != slices.Contains(readOnly, operation) || concurrent != read {
				t.Errorf("%s policy = read-only %v, concurrent %v", operation, read, concurrent)
			}
		}
		if ConnectionOperation(protocol.ToolOperationWriteFile) {
			t.Error("a file operation was classified as a connection operation")
		}
	})

	t.Run("arguments", func(t *testing.T) {
		_, err := ConnectionCall(protocol.ToolOperationWriteFile, json.RawMessage(`{}`))
		var fault *protocol.Fault
		if !errors.As(err, &fault) || fault.Code != protocol.ErrorCodeInvalidRequest || !strings.HasPrefix(fault.Message, "native operation is unavailable") {
			t.Errorf("other operations: error = %v", err)
		}
	})
}
