package main

import (
	"context"
	"crypto/ed25519"
	"crypto/sha256"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/libp2p/go-libp2p/core/crypto"
	"github.com/libp2p/go-libp2p/core/peer"
	ma "github.com/multiformats/go-multiaddr"

	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/link/peerrelay"
)

func TestApplyRelayBinaryRollback(t *testing.T) {
	for _, fail := range []bool{false, true} {
		t.Run(fmt.Sprint(fail), func(t *testing.T) {
			root := t.TempDir()
			installed := filepath.Join(root, "sailry-relay")
			stage := t.TempDir()
			candidate := filepath.Join(stage, "sailry-relay")
			for path, value := range map[string]string{installed: "original", candidate: "new"} {
				if err := os.WriteFile(path, []byte(value), 0o755); err != nil {
					t.Fatal(err)
				}
			}
			check := func(path, want string) {
				t.Helper()
				if body, err := os.ReadFile(path); err != nil || string(body) != want {
					t.Fatalf("%s = %q, %v; want %q", path, body, err, want)
				}
			}
			ctx, cancel := context.WithCancel(t.Context())
			defer cancel()
			recovered := false
			err := applyRelayBinary(ctx, installed, candidate, func(context.Context) error {
				check(installed, "new")
				check(installed+".previous", "original")
				if fail {
					cancel()
					return ctx.Err()
				}
				return nil
			}, func(recovery context.Context) error {
				recovered = true
				check(installed, "original")
				return recovery.Err()
			})
			if fail {
				if !errors.Is(err, context.Canceled) || !strings.Contains(err.Error(), "restored") || !recovered {
					t.Fatalf("rollback after cancellation: %v (recovered %t)", err, recovered)
				}
				check(installed, "original")
				if _, err := os.Lstat(installed + ".previous"); !errors.Is(err, os.ErrNotExist) {
					t.Fatalf("rollback did not consume the backup: %v", err)
				}
			} else {
				if err != nil || recovered {
					t.Fatalf("successful replacement: %v (recovered %t)", err, recovered)
				}
				check(installed+".previous", "original")
			}
		})
	}
}

func TestApplyRelayBinaryReplacementFailureKeepsIndependentBackup(t *testing.T) {
	installed := filepath.Join(t.TempDir(), "sailry-relay")
	missing := filepath.Join(t.TempDir(), "sailry-relay")
	if err := os.WriteFile(installed, []byte("original"), 0o751); err != nil {
		t.Fatal(err)
	}
	noServiceChange := func(context.Context) error {
		t.Fatal("replacement failure reached the service lifecycle")
		return nil
	}
	if err := applyRelayBinary(t.Context(), installed, missing, noServiceChange, noServiceChange); !errors.Is(err, os.ErrNotExist) {
		t.Fatalf("unexpected replacement error: %v", err)
	}
	current, err := os.Stat(installed)
	if err != nil {
		t.Fatal(err)
	}
	backup, err := os.Stat(installed + ".previous")
	if err != nil {
		t.Fatal(err)
	}
	if os.SameFile(current, backup) || current.Mode().Perm() != backup.Mode().Perm() {
		t.Fatalf("installed and backup must be independent files with matching permissions: %v and %v", current.Mode(), backup.Mode())
	}
}

func TestCopyLocalRelayBinaryChecksDigest(t *testing.T) {
	root := t.TempDir()
	source, staged := filepath.Join(root, "source"), filepath.Join(root, "stage")
	body := []byte("binary")
	if err := os.WriteFile(source, body, 0o600); err != nil {
		t.Fatal(err)
	}
	if err := copyLocalRelayBinary(source, staged, strings.Repeat("0", 64)); err == nil {
		t.Fatal("accepted a mismatched checksum")
	}
	if _, err := os.Stat(staged); !errors.Is(err, os.ErrNotExist) {
		t.Fatalf("checksum failure retained a candidate: %v", err)
	}
	if err := copyLocalRelayBinary(source, staged, fmt.Sprintf("%x", sha256.Sum256(body))); err != nil {
		t.Fatal(err)
	}
	if copied, err := os.ReadFile(staged); err != nil || string(copied) != string(body) {
		t.Fatalf("staged binary = %q, %v", copied, err)
	}
}

func TestCopyLocalRelayBinaryRejectsSymlink(t *testing.T) {
	root := t.TempDir()
	source, link, staged := filepath.Join(root, "source"), filepath.Join(root, "link"), filepath.Join(root, "stage")
	body := []byte("binary")
	if err := os.WriteFile(source, body, 0o600); err != nil {
		t.Fatal(err)
	}
	symlinkRelayTestInput(t, source, link)
	if err := copyLocalRelayBinary(link, staged, fmt.Sprintf("%x", sha256.Sum256(body))); err == nil || !strings.Contains(err.Error(), "symbolic links are not allowed") {
		t.Fatalf("local relay candidate symlink: %v", err)
	}
	if _, err := os.Lstat(staged); !errors.Is(err, os.ErrNotExist) {
		t.Fatalf("symlink input retained a candidate: %v", err)
	}
}

func TestManagedRelayReadinessAuthenticatesPeer(t *testing.T) {
	directory := filepath.Join(t.TempDir(), "state")
	if err := core.PrivateDirectory(directory); err != nil {
		t.Fatal(err)
	}
	identity, err := link.OpenIdentity(directory)
	if err != nil {
		t.Fatal(err)
	}
	seed := identity.SecretKey()
	key, err := crypto.UnmarshalEd25519PrivateKey(ed25519.NewKeyFromSeed(seed[:]))
	if err != nil {
		t.Fatal(err)
	}
	service, err := peerrelay.New(peerrelay.Config{PrivateKey: key, Public: true, ListenAddr: ma.StringCast("/ip4/127.0.0.1/tcp/0")})
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = service.Close() })
	address, err := peer.AddrInfoFromP2pAddr(service.Addr())
	if err != nil || len(address.Addrs) != 1 {
		t.Fatalf("relay address: %v, %v", address, err)
	}
	options := startOptions{directory: directory, config: peerrelay.Config{ListenAddr: address.Addrs[0]}}
	if err := waitRelayReady(t.Context(), options, address.ID); err != nil {
		t.Fatal(err)
	}
	if err := waitRelayReady(t.Context(), options, peer.ID("different key")); err == nil || !strings.Contains(err.Error(), "PeerID changed") {
		t.Fatalf("readiness accepted an unexpected identity: %v", err)
	}
}
