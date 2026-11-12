package live

import (
	"runtime"

	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// pluginDiagnostic is the report of a package's metadata read or view
// initialization that failed, which the Node records as a system notice
// (plugins/metadata/loading.rs, plugins/panel.rs report_failure). A failure
// without a protocol code is internal; a package without a version is not
// named, as the Node accepts no empty version.
func pluginDiagnostic(source protocol.NotificationSystemSystemSource, code protocol.ErrorCode, name string, version *string, operation string, scope protocol.PluginContext) protocol.NotificationSystemSystemDiagnostic {
	phase := protocol.NotificationSystemDiagnosticPhaseInitialization
	if source == protocol.NotificationSystemSystemSourcePluginMetadata {
		phase = protocol.NotificationSystemDiagnosticPhaseMetadata
	}
	if code == "" {
		code = protocol.ErrorCodeInternal
	}
	report := protocol.NotificationSystemSystemDiagnostic{
		Incident: protocol.NewRequestID(), Source: source, Phase: phase,
		Reason: protocol.NotificationSystemDiagnosticReasonFailure, Code: code,
		Worktree: scope.Worktree, Session: scope.Session, Operation: &operation,
		Version: link.ProductVersion, Platform: runtime.GOOS + "-" + runtime.GOARCH,
	}
	if version != nil && *version != "" {
		report.Plugin = &protocol.NotificationSystemDiagnosticPlugin{Name: name, Version: *version}
	}
	return report
}

// reportDiagnostic sends a report to the Node the failure belongs to. Its
// outcome is not shown: the notice is the Node's, and a Node that is away
// has none to raise.
func (r *Runtime) reportDiagnostic(report protocol.NotificationSystemSystemDiagnostic) {
	if report.Code == protocol.ErrorCodeCancelled || !r.controller.Connected() {
		return
	}
	r.async(func() {
		_, _ = r.controller.Do(r.ctx, protocol.Command{Kind: "report_diagnostic", Data: report})
	})
}
