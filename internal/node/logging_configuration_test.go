package node

import (
	"encoding/json"
	"errors"
	"log/slog"
	"path/filepath"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/logging"
	"github.com/sailry/sailry-harness/internal/node/plugins"
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
			rejectCommand(span, "submit_turn", failure, false)
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
	rejectCommand(logging.NewSpan("command"), "submit_turn", internal, false)
	if severity != slog.LevelError {
		t.Fatal("internal error was downgraded")
	}
	const entry, detail = "dev.sailry.platform/host/main.js", "private script detail"
	bundle := plugins.ScriptBundle{Entry: entry, Files: map[string]string{
		entry: `export function read() { throw new TypeError("` + detail + `"); }`,
	}}
	_, err = plugins.RunCallback(t.Context(), bundle, "read", nil, plugins.ScriptEnvironment{Context: protocol.DefaultPluginContext()}, true, nil)
	var diagnostic *protocol.Fault
	if !errors.As(err, &diagnostic) || diagnostic.Code != protocol.ErrorCodeInvalidRequest {
		t.Fatalf("callback fault = %v", err)
	}
	report, ok := faultDiagnostic(diagnostic)
	if !ok || report.Source != protocol.NotificationSystemSystemSourcePluginCallback || report.Phase != protocol.NotificationSystemDiagnosticPhaseCallback ||
		report.Exception == nil || *report.Exception != protocol.NotificationSystemScriptExceptionTypeError {
		t.Fatalf("callback diagnostic = %+v", report)
	}
	rejectCommand(logging.NewSpan("command", slog.String("operation", "read_plugin_callback")), "read_plugin_callback", diagnostic, false)
	if severity != slog.LevelError || fields["code"] != "invalid_request" || fields["operation"] != "read_plugin_callback" {
		t.Fatalf("callback fault logged as %v %v", severity, fields)
	}
	for _, value := range fields {
		if text, ok := value.(string); ok && strings.Contains(text, detail) {
			t.Fatal("script exception text entered the log")
		}
	}
}
