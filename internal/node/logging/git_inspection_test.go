package logging

import "testing"

func TestRemoteGitInspectionDiagnostic(t *testing.T) {
	t.Parallel()
	const ref = "e11525db-c4b2-45ff-9b82-ff23c41f1b15"
	fields := Fields{
		"area": "desktop", "operation": "inspect_git", "phase": Failed, "code": "unavailable",
		"stage": "parse_status", "reason": "unsupported_path_encoding", "report_ref": ref,
		"detail": "/private/worktree", "stderr": "fatal: private repository", "path": "/private/path",
	}
	remote := Remote(fields)
	if remote["stage"] != "parse_status" || remote["reason"] != "unsupported_path_encoding" || remote["report_ref"] != ref ||
		remote["detail"] != nil || remote["stderr"] != nil || remote["path"] != nil {
		t.Fatalf("remote Git inspection fields = %v", remote)
	}
	for _, invalid := range []struct {
		key   string
		value any
	}{
		{"area", "command"}, {"operation", "list_worktrees"}, {"phase", Completed}, {"code", "cancelled"},
		{"stage", "preflight"}, {"stage", "run_status"}, {"stage", "/private/path"}, {"stage", nil},
		{"reason", "private_repository_name"}, {"reason", "/private/path"}, {"reason", uint64(1)}, {"reason", nil},
	} {
		original := fields[invalid.key]
		fields[invalid.key] = invalid.value
		if remote := Remote(fields); remote["stage"] != nil || remote["reason"] != nil {
			t.Errorf("remote exported a Git diagnostic with %s = %v: %v", invalid.key, invalid.value, remote)
		}
		fields[invalid.key] = original
	}
}
