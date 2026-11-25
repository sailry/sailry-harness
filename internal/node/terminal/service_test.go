package terminal

import (
	"errors"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestWorkspaceTerminalRequiresSandbox(t *testing.T) {
	service := New(t.Context(), protocol.NodeID{1}, nil)
	t.Cleanup(func() { _ = service.Close() })
	launch := Launch{
		ID: protocol.TerminalID{1}, Root: t.TempDir(), Access: protocol.ProcessAccessWorkspace,
		Command: []string{"fixture-not-started"}, Viewport: protocol.TerminalViewport{Columns: 80, Rows: 24},
	}
	if info, err := service.Start(t.Context(), launch); info.ID != (protocol.TerminalID{}) || !isTerminalFault(err, protocol.ErrorCodeNotConfigured) {
		t.Fatalf("workspace launch without a sandbox = %+v, %v", info, err)
	}
	denied := fault(protocol.ErrorCodeNotConfigured, "fixture sandbox is unavailable")
	launch.Sandbox = func(root string, command, environment []string) ([]string, []string, func(), error) {
		return nil, nil, nil, denied
	}
	if info, err := service.Start(t.Context(), launch); info.ID != (protocol.TerminalID{}) || !errors.Is(err, denied) {
		t.Fatalf("workspace launch refused by its sandbox = %+v, %v", info, err)
	}
	if _, err := service.Info(launch.ID); !isTerminalFault(err, protocol.ErrorCodeNotFound) {
		t.Fatalf("refused workspace launch appeared in the inventory: %v", err)
	}
}

func isTerminalFault(err error, code protocol.ErrorCode) bool {
	var failure *protocol.Fault
	return errors.As(err, &failure) && failure.Code == code
}
