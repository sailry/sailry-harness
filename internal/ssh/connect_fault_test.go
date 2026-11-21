package ssh

import (
	"context"
	"crypto/ed25519"
	"errors"
	"strings"
	"testing"
	"testing/synctest"
	"time"

	"github.com/sailry/sailry-harness/internal/protocol"
	cryptossh "golang.org/x/crypto/ssh"
)

func TestConnectFailureReasons(t *testing.T) {
	const secret = "fixture-private-credential"
	profile := protocol.SshProfile{Username: "fixture", Host: "127.0.0.1", Authentication: protocol.SshAuthenticationPassword}
	credential := protocol.SshCredential{Kind: "password", Data: protocol.SshCredentialPasswordPayload{Password: protocol.NewSecret(secret)}}
	check := func(t *testing.T, err error, want protocol.SSHFailureReason) {
		t.Helper()
		var fault *protocol.Fault
		if !errors.As(err, &fault) || fault.Code != protocol.ErrorCodeUnavailable || fault.SSHReason != want || strings.Contains(fault.Message, secret) {
			t.Fatalf("connection fault = %v (%+v), want %s", err, fault, want)
		}
	}
	for _, test := range []struct {
		name string
		ctx  func() (context.Context, context.CancelFunc)
		want protocol.SSHFailureReason
	}{
		{"dial timeout", func() (context.Context, context.CancelFunc) {
			return context.WithDeadline(t.Context(), time.Unix(0, 0))
		}, protocol.SSHDialTimeout},
		{"dial cancelled", func() (context.Context, context.CancelFunc) {
			ctx, cancel := context.WithCancel(t.Context())
			cancel()
			return ctx, cancel
		}, protocol.SSHDialCancelled},
	} {
		t.Run(test.name, func(t *testing.T) {
			ctx, cancel := test.ctx()
			defer cancel()
			client, err := Connect(ctx, profile, credential)
			if client != nil {
				_ = client.Close()
				t.Fatal("connection unexpectedly opened")
			}
			check(t, err, test.want)
		})
	}
	t.Run("silent handshake reaches the connection deadline", func(t *testing.T) {
		synctest.Test(t, func(t *testing.T) {
			near, far := memoryPipe()
			defer far.Close()
			start := time.Now()
			ctx, cancel := context.WithTimeout(t.Context(), connectTimeout)
			defer cancel()
			_, err := connectClient(ctx, near, "fixture", profile, cryptossh.Password(secret))
			check(t, err, protocol.SSHHandshakeTimeout)
			if got := time.Since(start); got != 30*time.Second {
				t.Fatalf("handshake waited %v", got)
			}
		})
	})
	signer, err := cryptossh.NewSignerFromKey(ed25519.NewKeyFromSeed(make([]byte, ed25519.SeedSize)))
	if err != nil {
		t.Fatal(err)
	}
	trust := &protocol.SshHostKey{Algorithm: signer.PublicKey().Type(), Fingerprint: cryptossh.FingerprintSHA256(signer.PublicKey())}
	for _, test := range []struct {
		name string
		key  *protocol.SshHostKey
		wait bool
		want protocol.SSHFailureReason
	}{
		{"unknown host key", nil, false, ""},
		{"wrong credential", trust, false, protocol.SSHAuthFailed},
		{"authentication deadline", trust, true, protocol.SSHAuthTimeout},
	} {
		t.Run(test.name, func(t *testing.T) {
			synctest.Test(t, func(t *testing.T) {
				near, far := memoryPipe()
				release := make(chan struct{})
				defer close(release)
				server := &cryptossh.ServerConfig{PasswordCallback: func(cryptossh.ConnMetadata, []byte) (*cryptossh.Permissions, error) {
					if test.wait {
						<-release
					}
					return nil, errors.New(secret)
				}}
				server.AddHostKey(signer)
				go func() {
					defer far.Close()
					_, _, _, _ = cryptossh.NewServerConn(far, server)
				}()
				profile.HostKey = test.key
				ctx, cancel := context.WithTimeout(t.Context(), time.Second)
				defer cancel()
				_, err := connectClient(ctx, near, "fixture", profile, cryptossh.Password(secret))
				if test.key == nil {
					var required *HostKeyRequired
					if !errors.As(err, &required) || required.Changed || required.Key != *trust {
						t.Fatalf("host key was not offered for confirmation: %v", err)
					}
					return
				}
				check(t, err, test.want)
			})
		})
	}
}
