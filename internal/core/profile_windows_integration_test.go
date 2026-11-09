//go:build windows && integration

package core

import (
	"bytes"
	"errors"
	"os"
	"path/filepath"
	"testing"
	"unsafe"

	"golang.org/x/sys/windows"
)

func TestWindowsProfileSecurity(t *testing.T) {
	t.Parallel()
	t.Run("private profile and inherited files", func(t *testing.T) {
		path := filepath.Join(t.TempDir(), "profile")
		profile, err := AcquireProfile(path)
		if err != nil {
			t.Fatal(err)
		}
		defer profile.Close()
		key := filepath.Join(path, "link.key")
		if err := os.WriteFile(key, []byte("isolated identity fixture"), 0o600); err != nil {
			t.Fatal(err)
		}
		if err := validateExistingWindowsFile(key); err != nil {
			t.Fatal(err)
		}
		database := filepath.Join(path, "storage", "node.sqlite")
		if err := PrepareDatabase(database); err != nil {
			t.Fatal(err)
		}
		family := DatabaseFiles(database)
		for _, sidecar := range family[1:] {
			if err := os.WriteFile(sidecar, []byte("isolated journal fixture"), 0o600); err != nil {
				t.Fatal(err)
			}
		}
		if err := ValidateDatabase(database); err != nil {
			t.Fatal(err)
		}
		if err := profile.Close(); err != nil {
			t.Fatal(err)
		}
		reopened, err := AcquireProfile(path)
		if err != nil {
			t.Fatal(err)
		}
		defer reopened.Close()
	})
	for _, target := range []string{"directory", "identity", "lock", "database", "node.sqlite-wal", "node.sqlite-shm", "node.sqlite-journal"} {
		t.Run("reject shared "+target, func(t *testing.T) {
			path := filepath.Join(t.TempDir(), "profile")
			if target == "directory" {
				createWindowsProfileFixture(t, path, "D:P(A;OICI;FA;;;WD)")
			} else if err := PrivateDirectory(path); err != nil {
				t.Fatal(err)
			}
			fixture := path
			database := filepath.Join(path, "node.sqlite")
			var operation func() error
			operation = func() error {
				profile, err := AcquireProfile(path)
				if err == nil {
					_ = profile.Close()
				}
				return err
			}
			switch target {
			case "identity":
				fixture = filepath.Join(path, "link.key")
			case "lock":
				fixture = filepath.Join(path, "node.lock")
			case "database":
				if err := PrepareDatabase(database); err != nil {
					t.Fatal(err)
				}
				fixture = database
				operation = func() error { return PrepareDatabase(database) }
			case "node.sqlite-wal", "node.sqlite-shm", "node.sqlite-journal":
				fixture = filepath.Join(path, target)
				operation = func() error { return PrepareDatabase(database) }
			}
			marker := fixture
			if target == "directory" {
				marker = filepath.Join(path, "existing.bin")
			}
			const contents = "preserved fixture data"
			if err := os.WriteFile(marker, []byte(contents), 0o600); err != nil {
				t.Fatal(err)
			}
			if target != "directory" {
				shareWindowsProfileFixture(t, fixture)
			}
			before := windowsProfileDescriptor(t, fixture)
			if err := operation(); err == nil {
				t.Fatal("shared profile storage accepted")
			}
			if after := windowsProfileDescriptor(t, fixture); after != before {
				t.Fatal("refusal changed the access list")
			}
			data, err := os.ReadFile(marker)
			if err != nil || !bytes.Equal(data, []byte(contents)) {
				t.Fatalf("refusal changed fixture data: %v", err)
			}
			if target != "lock" && target != "database" {
				for _, absent := range []string{filepath.Join(path, "node.lock"), database} {
					if _, err := os.Stat(absent); !errors.Is(err, os.ErrNotExist) {
						t.Fatalf("refusal created %s: %v", absent, err)
					}
				}
			}
		})
	}
	t.Run("reject hard-linked database", func(t *testing.T) {
		path := filepath.Join(t.TempDir(), "storage")
		if err := PrivateDirectory(path); err != nil {
			t.Fatal(err)
		}
		database := filepath.Join(path, "node.sqlite")
		if err := PrepareDatabase(database); err != nil {
			t.Fatal(err)
		}
		alias := filepath.Join(path, "alias.turso")
		if err := os.Link(database, alias); err != nil {
			t.Fatalf("hard-link fixture unavailable: %v", err)
		}
		if err := PrepareDatabase(database); err == nil {
			t.Fatal("hard-linked database accepted")
		}
	})
	t.Run("reject null access list", func(t *testing.T) {
		path := filepath.Join(t.TempDir(), "unrestricted")
		createWindowsProfileFixture(t, path, "D:NO_ACCESS_CONTROL")
		if profile, err := AcquireProfile(path); err == nil {
			_ = profile.Close()
			t.Fatal("unrestricted profile accepted")
		}
	})
}

func createWindowsProfileFixture(t *testing.T, path, sddl string) {
	t.Helper()
	security, err := windows.SecurityDescriptorFromString(sddl)
	if err != nil {
		t.Fatal(err)
	}
	name, err := windowsName(path)
	if err != nil {
		t.Fatal(err)
	}
	attributes := windows.SecurityAttributes{Length: uint32(unsafe.Sizeof(windows.SecurityAttributes{})), SecurityDescriptor: security}
	if err := windows.CreateDirectory(name, &attributes); err != nil {
		t.Fatal(err)
	}
}

func shareWindowsProfileFixture(t *testing.T, path string) {
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

func windowsProfileDescriptor(t *testing.T, path string) string {
	t.Helper()
	security, err := windows.GetNamedSecurityInfo(path, windows.SE_FILE_OBJECT,
		windows.OWNER_SECURITY_INFORMATION|windows.DACL_SECURITY_INFORMATION)
	if err != nil {
		t.Fatal(err)
	}
	return security.String()
}
