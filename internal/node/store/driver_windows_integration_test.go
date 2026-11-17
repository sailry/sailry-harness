//go:build integration && windows

package store

import (
	"bytes"
	"context"
	"database/sql"
	"errors"
	"os"
	"path/filepath"
	"testing"

	"github.com/sailry/sailry-harness/internal/core"
	"golang.org/x/sys/windows"
)

func TestWindowsStoreSecurity(t *testing.T) {
	ctx := context.Background()
	t.Run("private database and new sidecars", func(t *testing.T) {
		path := windowsStoreFixture(t)
		handle, err := OpenHandle(ctx, path)
		if err != nil {
			t.Fatal(err)
		}
		defer handle.Close()
		if err := handle.Write(ctx, func(connection *sql.Conn) error {
			_, err := connection.ExecContext(ctx, "INSERT INTO private_fixture(body) VALUES('second record')")
			return err
		}); err != nil {
			t.Fatal(err)
		}
		if err := core.ValidateDatabase(path); err != nil {
			t.Fatalf("new database family is not private: %v", err)
		}
	})
	for index := range core.DatabaseFiles("node.turso") {
		name := core.DatabaseFiles("node.turso")[index]
		if index == 0 {
			name = "database"
		}
		for _, scenario := range []struct {
			name    string
			foreign bool
		}{
			{"restrict shared", false},
			{"reject foreign", true},
		} {
			t.Run(scenario.name+" "+name, func(t *testing.T) {
				if scenario.foreign && !windows.GetCurrentProcessToken().IsElevated() {
					t.Skip("an elevated token is required to create an administrator-owned fixture")
				}
				path := windowsStoreFixture(t)
				fixture := core.DatabaseFiles(path)[index]
				if index != 0 {
					if err := os.WriteFile(fixture, []byte("preserved sidecar fixture"), 0o600); err != nil {
						t.Fatal(err)
					}
				}
				shareWindowsStoreFixture(t, fixture)
				if scenario.foreign {
					foreignWindowsStoreFixture(t, fixture)
				}
				beforeData, err := os.ReadFile(fixture)
				if err != nil {
					t.Fatal(err)
				}
				beforeACL := windowsStoreDescriptor(t, fixture)
				beforeDatabase, err := os.ReadFile(path)
				if err != nil {
					t.Fatal(err)
				}
				if scenario.foreign {
					handle, err := OpenHandle(ctx, path)
					if err == nil {
						_ = handle.Close()
						t.Fatal("foreign-owned database storage was accepted")
					}
					var invalid *core.InvalidProfileError
					if !errors.As(err, &invalid) || invalid.Path != fixture {
						t.Fatalf("privacy refusal did not identify the foreign-owned file: %v", err)
					}
					if afterACL := windowsStoreDescriptor(t, fixture); afterACL != beforeACL {
						t.Fatal("refusal changed existing ownership or access")
					}
				} else {
					// Exercise Store's privacy boundary before the database engine
					// can interpret or checkpoint the synthetic sidecar contents.
					if err := createDatabaseFile(path); err != nil {
						t.Fatalf("owned shared storage was not restricted: %v", err)
					}
					user, err := windows.GetCurrentProcessToken().GetTokenUser()
					if err != nil {
						t.Fatal(err)
					}
					sid := user.User.Sid.String()
					private, err := windows.SecurityDescriptorFromString("O:" + sid + "D:P(A;;FA;;;" + sid + ")")
					if err != nil {
						t.Fatal(err)
					}
					if afterACL := windowsStoreDescriptor(t, fixture); afterACL != private.String() {
						t.Fatalf("owned storage did not receive private access: %s", afterACL)
					}
				}
				afterData, err := os.ReadFile(fixture)
				if err != nil || !bytes.Equal(beforeData, afterData) {
					t.Fatalf("privacy check changed the existing file: %v", err)
				}
				afterDatabase, err := os.ReadFile(path)
				if err != nil || !bytes.Equal(beforeDatabase, afterDatabase) {
					t.Fatalf("privacy check changed the database: %v", err)
				}
			})
		}
	}
}

func foreignWindowsStoreFixture(t *testing.T, path string) {
	t.Helper()
	user, err := windows.GetCurrentProcessToken().GetTokenUser()
	if err != nil {
		t.Fatal(err)
	}
	owner, err := windows.CreateWellKnownSid(windows.WinBuiltinAdministratorsSid)
	if err != nil {
		t.Fatal(err)
	}
	if err := windows.SetNamedSecurityInfo(path, windows.SE_FILE_OBJECT,
		windows.OWNER_SECURITY_INFORMATION, owner, nil, nil, nil); err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() {
		if err := windows.SetNamedSecurityInfo(path, windows.SE_FILE_OBJECT,
			windows.OWNER_SECURITY_INFORMATION, user.User.Sid, nil, nil, nil); err != nil {
			t.Error(err)
		}
	})
}

func windowsStoreFixture(t *testing.T) string {
	t.Helper()
	directory := filepath.Join(t.TempDir(), "storage")
	if err := core.PrivateDirectory(directory); err != nil {
		t.Fatal(err)
	}
	path := filepath.Join(directory, "node.turso")
	handle, err := OpenHandle(context.Background(), path)
	if err != nil {
		t.Fatal(err)
	}
	if err := handle.Write(context.Background(), func(connection *sql.Conn) error {
		_, err := connection.ExecContext(context.Background(), "CREATE TABLE private_fixture(body TEXT NOT NULL); INSERT INTO private_fixture VALUES('preserved record')")
		return err
	}); err != nil {
		_ = handle.Close()
		t.Fatal(err)
	}
	if err := handle.Close(); err != nil {
		t.Fatal(err)
	}
	return path
}

func shareWindowsStoreFixture(t *testing.T, path string) {
	t.Helper()
	security, err := windows.SecurityDescriptorFromString("D:P(A;;FA;;;WD)")
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

func windowsStoreDescriptor(t *testing.T, path string) string {
	t.Helper()
	security, err := windows.GetNamedSecurityInfo(path, windows.SE_FILE_OBJECT,
		windows.OWNER_SECURITY_INFORMATION|windows.DACL_SECURITY_INFORMATION)
	if err != nil {
		t.Fatal(err)
	}
	return security.String()
}
