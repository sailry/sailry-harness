package node

import (
	"log/slog"
	"path/filepath"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/logging"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestOnlyCodeReaderWaitIsExpectedBusy(t *testing.T) {
	closeLog, err := logging.Install(filepath.Join(t.TempDir(), "profile"), "host")
	if err != nil {
		t.Fatal(err)
	}
	defer closeLog()
	var level slog.Level
	var fields logging.Fields
	logging.SetReport(func(l slog.Level, f logging.Fields) { level, fields = l, f })
	defer logging.SetReport(nil)
	const wait = "code query timed out waiting for a reader"
	node := &Node{profile: &Profile{}, logs: true}
	plugin := protocol.DefaultPluginContext()
	plugin.Package.Name = "fixture.plugin"
	for _, tc := range []struct {
		command string
		fault   protocol.Fault
		warn    bool
	}{
		{"search_code", protocol.Fault{Code: protocol.ErrorCodeBusy, Message: wait}, true},
		{"outline_code", protocol.Fault{Code: protocol.ErrorCodeBusy, Message: wait}, true},
		{"graph_code", protocol.Fault{Code: protocol.ErrorCodeBusy, Message: wait}, true},
		{"outline_code", protocol.Fault{Code: protocol.ErrorCodeBusy, Message: "code query deadline exceeded"}, false},
		{"search_code", protocol.Fault{Code: protocol.ErrorCodeBusy, Message: "code query capacity is exhausted"}, false},
		{"search_code", protocol.Fault{Code: protocol.ErrorCodeBusy, Message: wait, Details: &protocol.FaultDetails{Kind: "system_diagnostic"}}, false},
		{"graph_code", protocol.Fault{Code: protocol.ErrorCodeInternal, Message: wait}, false},
		{"read_file", protocol.Fault{Code: protocol.ErrorCodeBusy, Message: wait}, false},
		{"read_plugin_memory", protocol.Fault{Code: protocol.ErrorCodeInternal, Message: "private plugin fault text"}, false},
	} {
		fields = nil
		request := protocol.Request{Command: protocol.Command{Kind: tc.command}, Plugin: &plugin}
		rejectCommand(node.commandSpan(node.ID(), request), tc.command, &tc.fault, false)
		if fields["code"] != string(tc.fault.Code) || fields["phase"] != logging.Failed ||
			tc.warn && (level != slog.LevelWarn || fields["reason_code"] != "code_reader_wait") ||
			!tc.warn && (level != slog.LevelError || fields["reason_code"] != nil) {
			t.Errorf("%s: %s, got %v %v", tc.command, tc.fault.Message, level, fields)
		}
		remote := logging.Remote(fields)
		if fields["source_kind"] != tc.command || fields["command"] != "plugin_command" || fields["detail"] != nil ||
			remote["source_kind"] != nil || remote["operation"] != "plugin_command" || remote["package"] != "third_party" {
			t.Errorf("%s: unexpected local or remote diagnostic: %v / %v", tc.command, fields, remote)
		}
	}
}
