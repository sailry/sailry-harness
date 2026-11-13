package node

import (
	"context"
	"errors"
	"math"
	"strings"
	"testing"
	"time"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// testSSHResourceRules checks the pure admission and interruption rules of the
// SSH resource commands. It is a helper for an existing grouped Node test to
// call as a subtest; it starts no Node, connection or timer.
func testSSHResourceRules(t *testing.T) {
	t.Helper()
	for _, kind := range []string{"browse_ssh_directory", "download_ssh_file", "stage_ssh_upload", "finish_ssh_upload", "modify_ssh_file", "run_ssh", "transfer_ssh", "cancel_ssh"} {
		if !sshResourceCommand(kind) {
			t.Fatalf("%s is not an SSH resource command", kind)
		}
	}
	for _, kind := range []string{"check_ssh", "install_host", "save_ssh", "open_ssh_terminal", "list_ssh"} {
		if sshResourceCommand(kind) {
			t.Fatalf("%s must stay with its existing owner", kind)
		}
	}
	for _, test := range []struct {
		milliseconds uint64
		want         time.Duration
		valid        bool
	}{
		{0, 0, false},
		{1, time.Millisecond, true},
		{5000, 5 * time.Second, true},
		{math.MaxInt64 / uint64(time.Millisecond), time.Duration(math.MaxInt64/int64(time.Millisecond)) * time.Millisecond, true},
		{math.MaxInt64/uint64(time.Millisecond) + 1, 0, false},
		{math.MaxUint64, 0, false},
	} {
		if got, valid := sshTimeout(test.milliseconds); got != test.want || valid != test.valid {
			t.Fatalf("sshTimeout(%d) = %v, %v", test.milliseconds, got, valid)
		}
	}

	failure := &protocol.Fault{Code: protocol.ErrorCodeNotFound, Message: "definite"}
	live := context.Background()
	stopped, stop := context.WithCancel(live)
	stop()
	expired, release := context.WithDeadline(live, time.Unix(0, 0))
	defer release()
	connectionFailure := &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "SSH connection could not be established"}
	for _, test := range []struct {
		name string
		ctx  context.Context
		code protocol.ErrorCode
	}{
		{"connection refused", live, protocol.ErrorCodeUnavailable},
		{"connection deadline", expired, protocol.ErrorCodeUnavailable},
		{"connection cancelled", stopped, protocol.ErrorCodeCancelled},
	} {
		var fault *protocol.Fault
		if err := sshConnectionFailure(test.ctx, connectionFailure); !errors.As(err, &fault) || fault.Code != test.code {
			t.Fatalf("%s = %v", test.name, err)
		}
	}
	for _, test := range []struct {
		name          string
		parent, timed context.Context
		want          string
	}{
		{"own failure", live, live, "definite"},
		{"cancelled", live, stopped, "cancelled"},
		{"timed out", live, expired, "timed out"},
		{"Node closed", stopped, stopped, "closed"},
	} {
		err := sshInterrupted(test.parent, test.timed, failure, "cancelled", "closed", "timed out")
		var fault *protocol.Fault
		if !errors.As(err, &fault) || !strings.Contains(fault.Message, test.want) {
			t.Fatalf("%s = %v", test.name, err)
		}
		if uncertain := fault.Code == protocol.ErrorCodeOutcomeUnknown; uncertain != (test.timed.Err() != nil) {
			t.Fatalf("%s code = %q", test.name, fault.Code)
		}
	}
}
