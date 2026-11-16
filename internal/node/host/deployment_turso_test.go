package host

import (
	"context"
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
	"github.com/sailry/sailry-harness/internal/shipped"
)

func TestSSHLocalArtifactsRequireTursoSeal(t *testing.T) {
	t.Parallel()
	for _, test := range []struct {
		name   string
		change func(*testing.T, string, *shipped.Seal)
	}{
		{"missing Turso", func(t *testing.T, root string, seal *shipped.Seal) {
			if err := os.Remove(filepath.Join(root, "libturso_sync_sdk_kit.so")); err != nil {
				t.Fatal(err)
			}
		}},
		{"empty Turso", func(t *testing.T, root string, seal *shipped.Seal) {
			if err := os.WriteFile(filepath.Join(root, "libturso_sync_sdk_kit.so"), nil, 0o600); err != nil {
				t.Fatal(err)
			}
		}},
		{"missing seal", func(t *testing.T, root string, seal *shipped.Seal) {
			if err := os.Remove(filepath.Join(root, shipped.SealName)); err != nil {
				t.Fatal(err)
			}
		}},
		{"wrong target", func(t *testing.T, root string, seal *shipped.Seal) {
			seal.Target = "windows/amd64"
			writeHostSeal(t, root, *seal)
		}},
		{"unsealed Turso", func(t *testing.T, root string, seal *shipped.Seal) {
			seal.Files = seal.Files[:1]
			writeHostSeal(t, root, *seal)
		}},
		{"changed Turso", func(t *testing.T, root string, seal *shipped.Seal) {
			if err := os.WriteFile(filepath.Join(root, "libturso_sync_sdk_kit.so"), []byte("changed engine"), 0o600); err != nil {
				t.Fatal(err)
			}
		}},
		{"changed transport", func(t *testing.T, root string, seal *shipped.Seal) {
			if err := os.WriteFile(filepath.Join(root, "libsailry_iroh.so"), []byte("changed transport"), 0o600); err != nil {
				t.Fatal(err)
			}
		}},
	} {
		t.Run(test.name, func(t *testing.T) {
			root := t.TempDir()
			for name, body := range map[string]string{"sailry-host": "Host", "libsailry_iroh.so": "transport", "libturso_sync_sdk_kit.so": "engine"} {
				if err := os.WriteFile(filepath.Join(root, name), []byte(body), 0o600); err != nil {
					t.Fatal(err)
				}
			}
			seal := shipped.Seal{Version: 1, Target: "linux/amd64"}
			for _, name := range []string{"libsailry_iroh.so", "libturso_sync_sdk_kit.so"} {
				entry, err := shipped.Record(root, name)
				if err != nil {
					t.Fatal(err)
				}
				seal.Files = append(seal.Files, entry)
			}
			writeHostSeal(t, root, seal)
			test.change(t, root, &seal)
			// Invalid artifacts must fail before any SSH staging or remote write.
			err := NewInstallations(DeploymentOptions{}).installLocal(context.Background(), nil, protocol.NodeID{1}, protocol.RequestID{2}, root, "linux-amd64", "libsailry_iroh.so", DeploymentOptions{}, nil, nil)
			var fault *protocol.Fault
			if !errors.As(err, &fault) || fault.Code != protocol.ErrorCodeUnavailable {
				t.Fatalf("invalid native package did not fail before SSH: %v", err)
			}
		})
	}
}

func writeHostSeal(t *testing.T, root string, seal shipped.Seal) {
	t.Helper()
	body, err := json.Marshal(seal)
	if err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(root, shipped.SealName), body, 0o600); err != nil {
		t.Fatal(err)
	}
}
