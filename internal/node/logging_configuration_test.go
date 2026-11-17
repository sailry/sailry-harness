package node

import (
	"encoding/json"
	"log/slog"
	"path/filepath"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/logging"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestConfigurationRefusalLogging(t *testing.T) {
	closeLog, err := logging.Install(filepath.Join(t.TempDir(), "profile"), "host")
	if err != nil {
		t.Fatal(err)
	}
	defer closeLog()
	var severity slog.Level
	var fields logging.Fields
	logging.SetReport(func(level slog.Level, values logging.Fields) { severity, fields = level, values })
	defer logging.SetReport(nil)
	for _, test := range []struct {
		reason protocol.ConfigurationReason
		level  slog.Level
	}{
		{protocol.ConfigurationDefaultsMissing, slog.LevelWarn},
		{protocol.ConfigurationProviderDisabled, slog.LevelWarn},
		{protocol.ConfigurationProviderRemoved, slog.LevelWarn},
		{protocol.ConfigurationLoginRequired, slog.LevelWarn},
		{protocol.ConfigurationAuthenticationChanged, slog.LevelWarn},
		{protocol.ConfigurationAuthorizationExpired, slog.LevelWarn},
		{protocol.ConfigurationCredentialRevoked, slog.LevelWarn},
		{protocol.ConfigurationPluginDisabled, slog.LevelWarn},
		{protocol.ConfigurationProviderMissing, slog.LevelError},
		{protocol.ConfigurationCredentialMissing, slog.LevelError},
		{protocol.ConfigurationRuntimeCredentialMissing, slog.LevelError},
		{protocol.ConfigurationProviderCaptureMissing, slog.LevelError},
		{protocol.ConfigurationAssistantCaptureMissing, slog.LevelError},
		{protocol.ConfigurationPluginUnavailable, slog.LevelError},
		{"future_reason", slog.LevelError},
		{"", slog.LevelError},
	} {
		t.Run(string(test.reason), func(t *testing.T) {
			failure := protocol.NewConfigurationFault(test.reason, "SECRET credential and prompt")
			if test.reason == "" {
				failure.Details = nil
			}
			// Durable completion and remote faults have been decoded from JSON.
			body, err := json.Marshal(failure)
			if err != nil {
				t.Fatal(err)
			}
			if err := protocol.DecodeAs(protocol.PeerData, body, failure); err != nil {
				t.Fatal(err)
			}
			fields = nil
			span := logging.NewSpan("command", slog.String("operation", "submit_turn"))
			rejectCommand(span, failure, false)
			if severity != test.level || fields["code"] != "not_configured" {
				t.Fatalf("logged %v %v, want %v", severity, fields, test.level)
			}
			if test.reason.Known() && fields["configuration_reason"] != string(test.reason) {
				t.Fatalf("reason lost: %v", fields)
			}
			if !test.reason.Known() && fields["configuration_reason"] != nil {
				t.Fatalf("unknown reason exposed: %v", fields)
			}
			for _, value := range fields {
				if value == failure.Message {
					t.Fatal("fault message entered the log")
				}
			}
		})
	}
	// A known reason must not hide an internal error or a script diagnostic.
	internal := protocol.NewConfigurationFault(protocol.ConfigurationLoginRequired, "private")
	internal.Code = protocol.ErrorCodeInternal
	rejectCommand(logging.NewSpan("command"), internal, false)
	if severity != slog.LevelError {
		t.Fatal("internal error was downgraded")
	}
	diagnostic := &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Details: &protocol.FaultDetails{Kind: "system_diagnostic"}}
	rejectCommand(logging.NewSpan("command"), diagnostic, false)
	if severity != slog.LevelError {
		t.Fatal("system diagnostic was downgraded")
	}
}
