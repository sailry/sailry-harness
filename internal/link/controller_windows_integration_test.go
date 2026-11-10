//go:build windows && integration

package link

import (
	"bytes"
	"errors"
	"os"
	"os/exec"
	"path/filepath"
	"testing"
	"unsafe"

	"github.com/sailry/sailry-harness/internal/protocol"
	"golang.org/x/sys/windows"
)

func TestWindowsControllerSecurity(t *testing.T) {
	t.Parallel()
	t.Run("private_unicode_reload_and_lock", func(t *testing.T) {
		path := filepath.Join(t.TempDir(), "用户 🙂")
		identity, store, err := openControllerStore(path)
		if err != nil {
			t.Fatal(err)
		}
		peer := protocol.NodeID{1}
		if err := store.SetTrust(t.Context(), peer, true); err != nil {
			t.Fatal(err)
		}
		for _, name := range []string{"node.lock", "link.key", "peers.json"} {
			if err := validateExistingControllerFile(filepath.Join(path, name)); err != nil {
				t.Fatal(err)
			}
		}
		lock := filepath.Join(path, "node.lock")
		if err := os.Remove(lock); err == nil {
			t.Fatal("live controller lock was removed")
		}
		if err := os.Rename(lock, filepath.Join(path, "replacement.lock")); err == nil {
			t.Fatal("live controller lock was renamed")
		}
		if _, duplicate, err := openControllerStore(path); err == nil {
			_ = duplicate.Close()
			t.Fatal("controller profile acquired twice")
		}
		if err := store.Close(); err != nil {
			t.Fatal(err)
		}
		reloaded, reopened, err := openControllerStore(path)
		if err != nil {
			t.Fatal(err)
		}
		defer reopened.Close()
		peers, err := reopened.Peers(t.Context())
		if err != nil || reloaded.ID() != identity.ID() || len(peers) != 1 || peers[0] != peer {
			t.Fatalf("controller reload: %v", err)
		}
	})
	for _, name := range []string{"link.key", "node.lock", "peers.json"} {
		t.Run("shared_"+name, func(t *testing.T) {
			path := filepath.Join(t.TempDir(), "profile")
			if err := privateControllerDirectory(path); err != nil {
				t.Fatal(err)
			}
			target := filepath.Join(path, name)
			const content = "preserved fixture data"
			if err := os.WriteFile(target, []byte(content), 0o600); err != nil {
				t.Fatal(err)
			}
			shareWindowsControllerFixture(t, target)
			before := windowsControllerDescriptor(t, target)
			if _, store, err := openControllerStore(path); err == nil {
				_ = store.Close()
				t.Fatal("shared controller file was accepted")
			}
			if data, err := os.ReadFile(target); err != nil || !bytes.Equal(data, []byte(content)) ||
				windowsControllerDescriptor(t, target) != before {
				t.Fatal("controller refusal changed existing storage")
			}
			if name == "link.key" {
				if _, err := os.Lstat(filepath.Join(path, "node.lock")); !errors.Is(err, os.ErrNotExist) {
					t.Fatalf("identity rejection created a lock: %v", err)
				}
			}
		})
		t.Run("hard_linked_"+name, func(t *testing.T) {
			path := filepath.Join(t.TempDir(), "profile")
			_, store, err := openControllerStore(path)
			if err != nil {
				t.Fatal(err)
			}
			if err := store.Close(); err != nil {
				t.Fatal(err)
			}
			if err := os.Link(filepath.Join(path, name), filepath.Join(path, "alias")); err != nil {
				t.Fatal(err)
			}
			if _, store, err := openControllerStore(path); err == nil {
				_ = store.Close()
				t.Fatal("hard-linked controller file was accepted")
			}
		})
	}
	t.Run("unsafe_replacement_target", func(t *testing.T) {
		path := filepath.Join(t.TempDir(), "profile")
		_, store, err := openControllerStore(path)
		if err != nil {
			t.Fatal(err)
		}
		defer store.Close()
		target := filepath.Join(path, "peers.json")
		shareWindowsControllerFixture(t, target)
		before := windowsControllerDescriptor(t, target)
		data, err := os.ReadFile(target)
		if err != nil {
			t.Fatal(err)
		}
		if err := store.SetTrust(t.Context(), protocol.NodeID{1}, true); err == nil {
			t.Fatal("shared replacement target was overwritten")
		}
		after, err := os.ReadFile(target)
		if err != nil || !bytes.Equal(data, after) || windowsControllerDescriptor(t, target) != before {
			t.Fatal("failed replacement changed existing storage")
		}
		if peers, err := store.Peers(t.Context()); err != nil || len(peers) != 0 {
			t.Fatalf("failed replacement changed memory: %v", err)
		}
	})
	for _, sddl := range []string{"D:P(A;OICI;FA;;;WD)", "D:NO_ACCESS_CONTROL"} {
		t.Run("unsafe_directory_"+sddl, func(t *testing.T) {
			path := filepath.Join(t.TempDir(), "profile")
			security, err := windows.SecurityDescriptorFromString(sddl)
			if err != nil {
				t.Fatal(err)
			}
			name, err := controllerWindowsName(path)
			if err != nil {
				t.Fatal(err)
			}
			attributes := windows.SecurityAttributes{
				Length: uint32(unsafe.Sizeof(windows.SecurityAttributes{})), SecurityDescriptor: security,
			}
			if err := windows.CreateDirectory(name, &attributes); err != nil {
				t.Fatal(err)
			}
			before := windowsControllerDescriptor(t, path)
			if _, store, err := openControllerStore(path); err == nil {
				_ = store.Close()
				t.Fatal("unsafe controller directory was accepted")
			}
			if windowsControllerDescriptor(t, path) != before {
				t.Fatal("refusal changed directory permissions")
			}
		})
	}
	t.Run("junction_directory", func(t *testing.T) {
		directory := t.TempDir()
		target, junction := filepath.Join(directory, "target"), filepath.Join(directory, "junction")
		if err := privateControllerDirectory(target); err != nil {
			t.Fatal(err)
		}
		if output, err := exec.CommandContext(t.Context(), "cmd.exe", "/d", "/c", "mklink", "/J", junction, target).CombinedOutput(); err != nil {
			t.Fatalf("create junction fixture: %s, %v", output, err)
		}
		if _, store, err := openControllerStore(junction); err == nil {
			_ = store.Close()
			t.Fatal("controller junction was accepted")
		}
		if _, err := os.Stat(filepath.Join(target, "node.lock")); !errors.Is(err, os.ErrNotExist) {
			t.Fatalf("junction rejection created a lock: %v", err)
		}
		if err := os.Remove(junction); err != nil {
			t.Fatal(err)
		}
	})
}

func shareWindowsControllerFixture(t *testing.T, path string) {
	t.Helper()
	security, err := windows.SecurityDescriptorFromString("D:P(A;OICI;FA;;;WD)")
	if err != nil {
		t.Fatal(err)
	}
	dacl, _, err := security.DACL()
	if err != nil {
		t.Fatal(err)
	}
	if err := windows.SetNamedSecurityInfo(path, windows.SE_FILE_OBJECT,
		windows.DACL_SECURITY_INFORMATION|windows.PROTECTED_DACL_SECURITY_INFORMATION, nil, nil, dacl, nil); err != nil {
		t.Fatal(err)
	}
}

func windowsControllerDescriptor(t *testing.T, path string) string {
	t.Helper()
	security, err := windows.GetNamedSecurityInfo(path, windows.SE_FILE_OBJECT,
		windows.OWNER_SECURITY_INFORMATION|windows.DACL_SECURITY_INFORMATION)
	if err != nil {
		t.Fatal(err)
	}
	return security.String()
}
