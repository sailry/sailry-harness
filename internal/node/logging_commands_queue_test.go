package node

import (
	"log/slog"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/logging"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestQueueRejectionLogging(t *testing.T) {
	closeLog, err := logging.Install(filepath.Join(t.TempDir(), "profile"), "desktop")
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(closeLog)
	for _, tc := range []struct {
		name, command string
		fault         protocol.Fault
		level, reason string
	}{
		{"stopping", "guide_queued_turn", protocol.Fault{Code: protocol.ErrorCodeConflict, Message: "guidance recipient is stopping", ReportRef: "5c003d59-71da-40f1-9fea-5f44d4397a35"}, "WARN", "recipient_stopping"},
		{"scope", "guide_queued_turn", protocol.Fault{Code: protocol.ErrorCodeConflict, Message: "guidance belongs to another execution scope"}, "ERROR", "guidance_scope_mismatch"},
		{"permission", "guide_queued_turn", protocol.Fault{Code: protocol.ErrorCodePermissionDenied, Message: "guidance recipient is stopping"}, "ERROR", ""},
		{"diagnostic", "guide_queued_turn", protocol.Fault{Code: protocol.ErrorCodeConflict, Message: "guidance recipient is stopping", Details: &protocol.FaultDetails{Kind: "system_diagnostic"}}, "ERROR", ""},
		{"unknown", "guide_queued_turn", protocol.Fault{Code: protocol.ErrorCodeConflict, Message: "private new failure"}, "ERROR", ""},
		{"unrelated command", "stop_turn", protocol.Fault{Code: protocol.ErrorCodeConflict, Message: "guidance recipient is stopping"}, "ERROR", ""},
	} {
		t.Run(tc.name, func(t *testing.T) {
			request := protocol.NewRequestID().String()
			span := logging.NewSpan("command", slog.String("request", request), slog.String("operation", tc.command))
			rejectCommand(span, tc.command, &tc.fault, false)
			path, err := logging.CurrentFile()
			if err != nil {
				t.Fatal(err)
			}
			data, err := os.ReadFile(path)
			if err != nil {
				t.Fatal(err)
			}
			for line := range strings.SplitSeq(string(data), "\n") {
				if !strings.Contains(line, "request="+request+" ") || !strings.Contains(line, "phase=failed ") {
					continue
				}
				if !strings.Contains(line, " "+tc.level+" desktop ") || strings.Contains(line, tc.fault.Message) || !strings.Contains(line, "code="+string(tc.fault.Code)+" ") ||
					(tc.reason != "") != strings.Contains(line, " reason_code=") || tc.reason != "" && !strings.Contains(line+" ", " reason_code="+tc.reason+" ") {
					t.Fatalf("incorrect rejection record: %s", line)
				}
				return
			}
			t.Fatal("missing local command rejection record")
		})
	}
}
