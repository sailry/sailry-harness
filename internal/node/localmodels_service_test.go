package node

import (
	"context"
	"database/sql"
	"log/slog"
	"path/filepath"
	"testing"

	"github.com/sailry/sailry-harness/internal/localmodels"
	"github.com/sailry/sailry-harness/internal/node/logging"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestLocalModelReadTerminalObservation(t *testing.T) {
	path := filepath.Join(t.TempDir(), "profile")
	closeLog, err := logging.Install(path, "host")
	if err != nil {
		t.Fatal(err)
	}
	defer closeLog()
	profile, err := OpenProfile(t.Context(), path)
	if err != nil {
		t.Fatal(err)
	}
	defer profile.Close()
	if err := profile.Store.Node.Write(t.Context(), func(db *sql.Conn) error {
		_, err := db.ExecContext(t.Context(), "UPDATE local_model_settings SET body = ?1 WHERE singleton = 1", []byte("invalid settings record"))
		return err
	}); err != nil {
		t.Fatal(err)
	}
	var reports []logging.Fields
	logging.SetReport(func(level slog.Level, fields logging.Fields) {
		if level == slog.LevelError && fields["request"] != nil {
			reports = append(reports, fields)
		}
	})
	defer logging.SetReport(nil)
	registry := localmodels.New(localmodels.Options{})
	defer registry.Shutdown()
	node := &Node{profile: profile, localModels: &localModelControls{registry: registry}, ctx: t.Context(), logs: true}
	defer node.work.Wait()
	request := protocol.NewRequest(node.ID(), protocol.Command{Kind: "read_local_model_settings"})
	result := dispatchFixture(t, node, node.ID(), request)
	if result.Fault == nil || result.Fault.Code != protocol.ErrorCodeInternal || !logging.ValidReportRef(result.Fault.ReportRef) {
		t.Fatalf("unexpected completion fault: %v", result.Fault)
	}
	if len(reports) != 1 || reports[0]["request"] != request.ID.String() || reports[0]["operation"] != request.Command.Kind ||
		reports[0]["code"] != string(result.Fault.Code) || reports[0]["report_ref"] != result.Fault.ReportRef || reports[0]["command_stage"] != "completion" {
		t.Fatalf("terminal reports = %v", reports)
	}
	ctx, cancel := context.WithCancel(t.Context())
	cancel()
	request.ID = protocol.NewRequestID()
	if _, err := node.Dispatch(ctx, node.ID(), request); err == nil || nodeFault(err).(*protocol.Fault).Code != protocol.ErrorCodeCancelled {
		t.Fatalf("cancelled read: %v", err)
	}
	if len(reports) != 1 {
		t.Fatalf("cancelled read added failure reports: %v", reports)
	}
}
