package logging

import "testing"

func TestRemotePreferencesWriteDiagnostic(t *testing.T) {
	t.Parallel()
	for _, stage := range []string{"create_directory", "create_temp", "write", "sync", "close", "replace"} {
		for _, reason := range []string{"permission_denied", "not_found", "already_exists", "no_space", "file_locked", "other"} {
			fields := Fields{
				"area": "desktop", "operation": "preferences_save_failed", "phase": Failed, "code": "internal",
				"stage": stage, "reason": reason, "detail": "private error text", "path": "/private/preferences.json", "errno": uint64(32),
			}
			if safe := Remote(fields); safe["stage"] != stage || safe["reason"] != reason || len(safe) != 6 {
				t.Fatalf("remote preference write fields = %v", safe)
			}
		}
	}
	fields := Fields{"area": "desktop", "operation": "preferences_save_failed", "phase": Failed, "code": "internal", "stage": "replace", "reason": "file_locked"}
	for _, invalid := range []struct {
		key   string
		value any
	}{
		{"area", "command"}, {"operation", "preferences_read_failed"}, {"phase", Completed}, {"code", "cancelled"},
		{"stage", "preflight"}, {"stage", "/private/path"}, {"stage", uint64(1)}, {"stage", nil},
		{"reason", "private_error_text"}, {"reason", "/private/path"}, {"reason", true}, {"reason", nil},
	} {
		original := fields[invalid.key]
		fields[invalid.key] = invalid.value
		if safe := Remote(fields); safe["stage"] != nil || safe["reason"] != nil {
			t.Errorf("invalid %s=%v escaped as %v", invalid.key, invalid.value, safe)
		}
		fields[invalid.key] = original
	}
}
