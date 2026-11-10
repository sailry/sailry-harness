package live

import (
	"errors"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/updater"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// checkSystemHelpers covers the About, diagnostics and selection mappings.
func checkSystemHelpers(t *testing.T) {
	var u settings.Updater
	updateView(&u, updater.State{Kind: updater.Downloading, Version: "0.2.0", Progress: updater.Progress{Copied: 5, Total: 10}}, nil)
	if u.State != settings.UpdateDownloading || u.Version != "0.2.0" || u.Copied != 5 || u.Total != 10 || u.Failure != nil {
		t.Fatalf("download view = %+v", u)
	}
	failure := &updater.Failure{Key: "updates_signature_invalid", Detail: "bad signature"}
	updateView(&u, updater.State{Kind: updater.Idle}, failure)
	first := u.Failure
	updateView(&u, updater.State{Kind: updater.Idle}, &updater.Failure{Key: failure.Key, Detail: failure.Detail})
	if u.State != settings.UpdateIdle || u.Copied != 0 || first == nil || u.Failure != first || *first != (settings.UpdateFailure{Key: failure.Key, Detail: failure.Detail}) {
		t.Fatalf("failure view = %+v", u)
	}
	for kind, want := range map[updater.Kind]settings.UpdateState{updater.Checking: settings.UpdateChecking, updater.Available: settings.UpdateAvailable, updater.Ready: settings.UpdateReady,
		updater.Installing: settings.UpdateInstalling, updater.Restarting: settings.UpdateRestarting, updater.Stopped: settings.UpdateStopped} {
		if updateView(&u, updater.State{Kind: kind}, nil); u.State != want || u.Failure != nil {
			t.Fatalf("state %d = %+v", kind, u)
		}
	}

	for _, test := range []struct {
		notice updater.Notice
		kind   string
		action bool
	}{
		{updater.Notice{Key: "updates_ready", Version: "0.2.0"}, "success", true},
		{updater.Notice{Key: "updates_current"}, "success", false},
		{updater.Notice{Key: "updates_network_failed", Error: true, Detail: "refused"}, "error", true},
		{updater.Notice{Key: "updates_install_uncertain", Error: true, Detail: "kept"}, "warning", true},
	} {
		toast := updateToast(test.notice, "title", "action", 1, func() {})
		if toast.Type != test.kind || (toast.OnAction != nil) != test.action || test.notice.Error && toast.Timeout != -1 {
			t.Fatalf("toast for %+v = %+v", test.notice, toast)
		}
	}

	for _, test := range []struct {
		err         error
		diagnostics bool
		outcome     kit.Outcome
		key         string
	}{
		{nil, false, kit.Done, "telemetry_reset"},
		{nil, true, kit.Done, "telemetry_diagnostics_sent"},
		{&protocol.Fault{Code: protocol.ErrorCodeRevisionConflict}, false, kit.Conflict, "telemetry_conflict"},
		{&protocol.Fault{Code: protocol.ErrorCodeOutcomeUnknown}, false, kit.Uncertain, "telemetry_unknown"},
		{errors.New("closed"), false, kit.Failed, "telemetry_failed"},
		{&protocol.Fault{Code: protocol.ErrorCodeUnavailable}, true, kit.Failed, "telemetry_diagnostics_failed"},
	} {
		if outcome, key := telemetryOutcome(test.err, test.diagnostics); outcome != test.outcome || key != test.key {
			t.Fatalf("outcome for %v = %v %q", test.err, outcome, key)
		}
	}

	for text, want := range map[string]string{"selected": "", " \n": "selection_empty", "": "selection_empty", "a\x00b": "selection_large", strings.Repeat("a", 65537): "selection_large"} {
		if got, key := boundedSelection(text); key != want || (want == "") != (got == text && got != "") {
			t.Fatalf("selection %q = %q %q", text[:min(len(text), 8)], got[:min(len(got), 8)], key)
		}
	}
}
