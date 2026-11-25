//go:build !windows

package preferences

import (
	"os"
	"syscall"
	"testing"
)

func TestWriteFailureNoSpace(t *testing.T) {
	err := &writeFailure{stage: "sync", err: &os.PathError{Op: "sync", Path: "/private/preferences.json", Err: syscall.ENOSPC}}
	if stage, reason := WriteFailure(err); stage != "sync" || reason != "no_space" {
		t.Fatalf("out-of-space diagnostic = %q/%q", stage, reason)
	}
}
