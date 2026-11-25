package live

import (
	"slices"
	"testing"

	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func workspaceFailures(t *testing.T) {
	fault := func(code protocol.ErrorCode, message string) error {
		return &link.FaultError{Fault: protocol.Fault{Code: code, Message: message}}
	}
	cases := []struct {
		kind    string
		err     error
		outcome kit.Outcome
		key     string
	}{
		{fileSave, fault(protocol.ErrorCodePermissionDenied, ""), kit.Failed, "workspace_action_failed"},
		{fileSave, fault(protocol.ErrorCodeConflict, ""), kit.Conflict, "workspace_changed"},
		{fileSave, fault(protocol.ErrorCodeOutcomeUnknown, ""), kit.Uncertain, "workspace_action_unknown"},
	}
	// A save the file's change refused has no toast: the editor asks instead.
	for _, code := range []protocol.ErrorCode{protocol.ErrorCodeRevisionConflict, protocol.ErrorCodeNotFound} {
		if _, key := workspaceFailure(fileSave, fault(code, "")); key != "" {
			t.Errorf("refused save %v toasts %q", code, key)
		}
	}
	for _, lang := range []string{"en", "zh-CN"} {
		keys := i18n.Keys(lang)
		for _, test := range cases {
			outcome, key := workspaceFailure(test.kind, test.err)
			if outcome != test.outcome || key != test.key {
				t.Errorf("%s %v: %v %q, want %v %q", test.kind, test.err, outcome, key, test.outcome, test.key)
			}
			if !slices.Contains(keys, key) {
				t.Errorf("%s has no copy for %q", lang, key)
			}
		}
	}
}
