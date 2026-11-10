package ssh

import (
	"context"
	"errors"
	"net"
	"runtime"
	"strconv"
	"syscall"
	"time"

	"github.com/jonboulle/clockwork"
	"github.com/sailry/sailry-harness/internal/protocol"
	cryptossh "golang.org/x/crypto/ssh"
)

const connectTimeout = 30 * time.Second

var retryDelays = [...]time.Duration{250 * time.Millisecond, 500 * time.Millisecond, time.Second, 2 * time.Second}

// recoverUnreachable repeats attempt only while it reports EHOSTUNREACH on
// macOS. Verified macOS local-network cache rebuilding can reject several TCP
// attempts even with access allowed; TN3179 recommends connection recovery for
// socket APIs. The caller keeps its original deadline and cancellation.
func recoverUnreachable(ctx context.Context, clock clockwork.Clock, goos string, attempt func() error) error {
	err := attempt()
	for _, delay := range retryDelays {
		if goos != "darwin" || !errors.Is(err, syscall.EHOSTUNREACH) {
			return err
		}
		select {
		case <-ctx.Done():
			return err
		case <-clock.After(delay):
		}
		err = attempt()
	}
	return err
}

// ConnectWithRecovery wraps Connect with the bounded pre-authentication
// recovery. Connect hides its TCP error, so after a dial failure on macOS one
// classification dial decides whether the host was unreachable. Handshake,
// authentication, SFTP and remote effects are never repeated.
func ConnectWithRecovery(ctx context.Context, clock clockwork.Clock, profile protocol.SshProfile, credential protocol.SshCredential) (*cryptossh.Client, error) {
	scoped, cancel := context.WithTimeout(ctx, connectTimeout)
	defer cancel()
	var client *cryptossh.Client
	var failure error
	_ = recoverUnreachable(scoped, clock, runtime.GOOS, func() error {
		client, failure = Connect(scoped, profile, credential)
		if failure == nil || runtime.GOOS != "darwin" || !dialFailed(failure) {
			return failure
		}
		probe, err := (&net.Dialer{}).DialContext(scoped, "tcp", net.JoinHostPort(profile.Host, strconv.Itoa(int(profile.Port))))
		if err == nil {
			_ = probe.Close()
			return failure
		}
		if errors.Is(err, syscall.EHOSTUNREACH) {
			return syscall.EHOSTUNREACH
		}
		return failure
	})
	return client, failure
}

// dialFailed recognizes the one Connect fault raised before any SSH byte is sent.
func dialFailed(err error) bool {
	var fault *protocol.Fault
	return errors.As(err, &fault) && fault.Code == protocol.ErrorCodeUnavailable && fault.Message == "SSH connection could not be established"
}
