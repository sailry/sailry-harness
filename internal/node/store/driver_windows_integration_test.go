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
	for _, suffix := range []string{"", "-wal", "-shm", "-journal"} {
		name := suffix
		if name == "" {
			name = "database"
		}
		t.Run("reject shared "+name, func(t *testing.T) {
			path := windowsStoreFixture(t)
			fixture := path + suffix
			if suffix != "" {
				if err := os.WriteFile(fixture, []byte("preserved sidecar fixture"), 0o600); err != nil {
					t.Fatal(err)
				}
			}
			shareWindowsStoreFixture(t, fixture)
			beforeData, err := os.ReadFile(fixture)
			if err != nil {
				t.Fatal(err)
			}
			beforeACL := windowsStoreDescriptor(t, fixture)
			beforeDatabase, err := os.ReadFile(path)
			if err != nil {
				t.Fatal(err)
			}
			handle, err := OpenHandle(ctx, path)
			if err == nil {
				_ = handle.Close()
				t.Fatal("shared database storage was accepted")
			}
			var invalid *core.InvalidProfileError
			if !errors.As(err, &invalid) || invalid.Path != fixture {
				t.Fatalf("privacy refusal did not identify the shared file: %v", err)
			}
			afterData, err := os.ReadFile(fixture)
			if err != nil || !bytes.Equal(beforeData, afterData) {
				t.Fatalf("refusal changed the shared file: %v", err)
			}
			if afterACL := windowsStoreDescriptor(t, fixture); afterACL != beforeACL {
				t.Fatal("refusal changed the existing access list")
			}
			afterDatabase, err := os.ReadFile(path)
			if err != nil || !bytes.Equal(beforeDatabase, afterDatabase) {
				t.Fatalf("refusal changed the database: %v", err)
			}
		})
	}
}

func windowsStoreFixture(t *testing.T) string {
	t.Helper()
	directory := filepath.Join(t.TempDir(), "storage")
	if err := core.PrivateDirectory(directory); err != nil {
		t.Fatal(err)
	}
	path := filepath.Join(directory, "node.sqlite")
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
