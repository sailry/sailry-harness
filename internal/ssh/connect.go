package ssh

import (
	"context"
	"crypto/subtle"
	"errors"
	"io"
	"net"
	"os"
	"slices"
	"strconv"
	"strings"
	"time"

	"github.com/sailry/sailry-harness/internal/protocol"
	cryptossh "golang.org/x/crypto/ssh"
	"golang.org/x/crypto/ssh/agent"
)

type HostKeyRequired struct {
	Key     protocol.SshHostKey
	Changed bool
}

func (*HostKeyRequired) Error() string { return "SSH host key requires explicit confirmation" }

func (required *HostKeyRequired) Outcome() protocol.SshOutcome {
	return protocol.SshOutcome{Kind: "host_key_required", Data: protocol.SshOutcomeHostKeyRequiredPayload{Key: required.Key, Changed: required.Changed}}
}

func VerifyHostKey(trusted *protocol.SshHostKey, key cryptossh.PublicKey) error {
	actual := protocol.SshHostKey{Algorithm: key.Type(), Fingerprint: cryptossh.FingerprintSHA256(key)}
	if trusted != nil && trusted.Algorithm == actual.Algorithm &&
		subtle.ConstantTimeCompare([]byte(trusted.Fingerprint), []byte(actual.Fingerprint)) == 1 {
		return nil
	}
	return &HostKeyRequired{Key: actual, Changed: trusted != nil}
}

// keepaliveInterval and keepaliveMisses are OpenSSH's ServerAliveInterval and
// ServerAliveCountMax: a server that answers no keepalive for their product
// is gone, and its connection is closed so that its users fail instead of
// waiting on a dead route.
const (
	keepaliveInterval = 15 * time.Second
	keepaliveMisses   = 3
)

// hostKeyAlgorithms prefers the trusted key's algorithms, as OpenSSH orders
// HostKeyAlgorithms from known_hosts: a server that later adds a key of a
// type the library prefers still presents the key that was trusted, instead
// of reading as changed. A nil result keeps the library's order.
func hostKeyAlgorithms(trusted *protocol.SshHostKey) []string {
	if trusted == nil {
		return nil
	}
	preferred := []string{trusted.Algorithm}
	switch trusted.Algorithm {
	case cryptossh.KeyAlgoRSA:
		preferred = []string{cryptossh.KeyAlgoRSASHA512, cryptossh.KeyAlgoRSASHA256, cryptossh.KeyAlgoRSA}
	case cryptossh.CertAlgoRSAv01:
		preferred = []string{cryptossh.CertAlgoRSASHA512v01, cryptossh.CertAlgoRSASHA256v01, cryptossh.CertAlgoRSAv01}
	}
	supported := cryptossh.SupportedAlgorithms().HostKeys
	preferred = slices.DeleteFunc(preferred, func(algorithm string) bool { return !slices.Contains(supported, algorithm) })
	if len(preferred) == 0 {
		return nil
	}
	for _, algorithm := range supported {
		if !slices.Contains(preferred, algorithm) {
			preferred = append(preferred, algorithm)
		}
	}
	return preferred
}

// clientConfig is the handshake's configuration for a profile.
func clientConfig(profile protocol.SshProfile, auth cryptossh.AuthMethod) *cryptossh.ClientConfig {
	return &cryptossh.ClientConfig{
		User:              profile.Username,
		Auth:              []cryptossh.AuthMethod{auth},
		HostKeyAlgorithms: hostKeyAlgorithms(profile.HostKey),
		HostKeyCallback: func(_ string, _ net.Addr, key cryptossh.PublicKey) error {
			return VerifyHostKey(profile.HostKey, key)
		},
	}
}

// keepAlive asks the server for a reply every interval until the connection
// ends, and closes it when one does not come within misses intervals.
func keepAlive(client *cryptossh.Client, interval time.Duration, misses int) {
	ended := make(chan struct{})
	go func() { _ = client.Wait(); close(ended) }()
	wait := time.NewTimer(interval)
	defer wait.Stop()
	for {
		select {
		case <-ended:
			return
		case <-wait.C:
		}
		answered := make(chan struct{})
		go func() {
			// Any reply, a refusal included, shows the server is there.
			_, _, _ = client.SendRequest("keepalive@openssh.com", true, nil)
			close(answered)
		}()
		wait.Reset(time.Duration(misses) * interval)
		select {
		case <-ended:
			return
		case <-answered:
		case <-wait.C:
			_ = client.Close()
			return
		}
		wait.Reset(interval)
	}
}

func Connect(ctx context.Context, profile protocol.SshProfile, credential protocol.SshCredential) (*cryptossh.Client, error) {
	scoped, cancel := context.WithTimeout(ctx, 30*time.Second)
	defer cancel()
	auth, release, err := authentication(scoped, profile.Authentication, credential)
	if err != nil {
		return nil, err
	}
	defer release()
	address := net.JoinHostPort(profile.Host, strconv.Itoa(int(profile.Port)))
	connection, err := (&net.Dialer{}).DialContext(scoped, "tcp", address)
	if err != nil {
		return nil, unavailable("SSH connection could not be established")
	}
	stop := context.AfterFunc(scoped, func() { _ = connection.Close() })
	transport, channels, requests, err := cryptossh.NewClientConn(connection, address, clientConfig(profile, auth))
	if !stop() {
		if transport != nil {
			_ = transport.Close()
		}
		_ = connection.Close()
		return nil, unavailable("SSH connection was interrupted")
	}
	if err != nil {
		_ = connection.Close()
		var required *HostKeyRequired
		if errors.As(err, &required) {
			return nil, required
		}
		return nil, unavailable("SSH authentication or handshake failed")
	}
	client := cryptossh.NewClient(transport, channels, requests)
	go keepAlive(client, keepaliveInterval, keepaliveMisses)
	return client, nil
}

func authentication(ctx context.Context, kind protocol.SshAuthentication, credential protocol.SshCredential) (cryptossh.AuthMethod, func(), error) {
	release := func() {}
	if credential.Kind != string(kind) {
		return nil, release, invalid("SSH credential authentication does not match")
	}
	var key string
	var passphrase *protocol.Secret
	switch input := credential.Data.(type) {
	case *protocol.SshCredentialPasswordPayload:
		return cryptossh.Password(input.Password.Expose()), release, nil
	case protocol.SshCredentialPasswordPayload:
		return cryptossh.Password(input.Password.Expose()), release, nil
	case *protocol.SshCredentialPrivateKeyPayload:
		key, passphrase = input.Key.Expose(), input.Passphrase
	case protocol.SshCredentialPrivateKeyPayload:
		key, passphrase = input.Key.Expose(), input.Passphrase
	case *protocol.SshCredentialKeyPathPayload:
		value, err := privateKeyFile(input.Path.Expose())
		if err != nil {
			return nil, release, err
		}
		key, passphrase = value, input.Passphrase
	case protocol.SshCredentialKeyPathPayload:
		value, err := privateKeyFile(input.Path.Expose())
		if err != nil {
			return nil, release, err
		}
		key, passphrase = value, input.Passphrase
	}
	if credential.Kind == "agent" {
		path := os.Getenv("SSH_AUTH_SOCK")
		if path == "" {
			return nil, release, unavailable("SSH agent is unavailable")
		}
		connection, err := (&net.Dialer{}).DialContext(ctx, "unix", path)
		if err != nil {
			return nil, release, unavailable("SSH agent is unavailable")
		}
		stop := context.AfterFunc(ctx, func() { _ = connection.Close() })
		return cryptossh.PublicKeysCallback(agent.NewClient(connection).Signers), func() { stop(); _ = connection.Close() }, nil
	}
	var signer cryptossh.Signer
	var err error
	if passphrase == nil {
		signer, err = cryptossh.ParsePrivateKey([]byte(key))
	} else {
		signer, err = cryptossh.ParsePrivateKeyWithPassphrase([]byte(key), []byte(passphrase.Expose()))
	}
	if err != nil {
		return nil, release, invalid("SSH private key or passphrase is invalid")
	}
	return cryptossh.PublicKeys(signer), release, nil
}

func privateKeyFile(path string) (string, error) {
	if strings.HasPrefix(path, "~/") {
		home, err := os.UserHomeDir()
		if err != nil {
			return "", unavailable("SSH key file is unavailable")
		}
		path = home + strings.TrimPrefix(path, "~")
	}
	metadata, err := os.Stat(path)
	if err != nil || !metadata.Mode().IsRegular() {
		return "", unavailable("SSH key file is unavailable")
	}
	file, err := os.Open(path)
	if err != nil {
		return "", unavailable("SSH key file is unavailable")
	}
	defer file.Close()
	body, err := io.ReadAll(io.LimitReader(file, 65537))
	if err != nil || len(body) > 65536 {
		return "", unavailable("SSH key file is unavailable")
	}
	return string(body), nil
}

func unavailable(message string) error {
	return &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: message}
}

func invalid(message string) error {
	return &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: message}
}
