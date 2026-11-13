package databases

import (
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestSQLiteStorageStatements(t *testing.T) {
	root := t.TempDir()
	path := filepath.Join(root, "external.sqlite")
	sqliteQuery(t, path, "CREATE TABLE items (id INTEGER)", 1)
	storage := filepath.Join(root, "profile")
	if err := os.MkdirAll(filepath.Join(storage, "nested"), 0o700); err != nil {
		t.Fatal(err)
	}
	paths := map[string]string{
		"direct": filepath.Join(storage, "copy.sqlite"),
	}
	if runtime.GOOS != "windows" {
		alias := filepath.Join(root, "alias")
		if err := os.Symlink(filepath.Join(storage, "nested"), alias); err != nil {
			t.Fatal(err)
		}
		paths["symlink and parent"] = alias + "/../copy.sqlite"
		dangling := filepath.Join(root, "dangling")
		if err := os.Symlink(paths["direct"], dangling); err != nil {
			t.Fatal(err)
		}
		paths["dangling symlink"] = dangling
	}
	for _, statement := range []string{"VACUUM INTO '%s'", "ATTACH '%s' AS other"} {
		t.Run(statement, func(t *testing.T) {
			for name, target := range paths {
				t.Run(name, func(t *testing.T) {
					sql := strings.Replace(statement, "%s", strings.ReplaceAll(target, "'", "''"), 1)
					operation := sqliteOperation(path, false, &Query{SQL: sql, RowLimit: 1, TimeoutMS: 1000})
					operation.Storage = storage
					if _, err := Execute(t.Context(), operation); err == nil {
						t.Error("SQL accessed Node storage")
					}
					if _, err := os.Stat(target); !os.IsNotExist(err) {
						t.Errorf("SQL created a Node profile file: %v", err)
					}
				})
			}
		})
	}
	for name, sql := range map[string]string{
		"vacuum":    "VACUUM",
		"export":    "VACUUM INTO '" + strings.ReplaceAll(filepath.Join(root, "export.sqlite"), "'", "''") + "'",
		"memory":    "ATTACH ':memory:' AS other",
		"temporary": "ATTACH '' AS other",
	} {
		t.Run(name, func(t *testing.T) {
			operation := sqliteOperation(path, false, &Query{SQL: sql, RowLimit: 1, TimeoutMS: 1000})
			operation.Storage = storage
			if _, err := Execute(t.Context(), operation); err != nil {
				t.Fatalf("allowed operation: %v", err)
			}
		})
	}
	t.Run("computed attachment cannot bypass the check", func(t *testing.T) {
		sql := "ATTACH ('" + strings.ReplaceAll(paths["direct"], "'", "''") + "' || '') AS other"
		operation := sqliteOperation(path, false, &Query{SQL: sql, RowLimit: 1, TimeoutMS: 1000})
		operation.Storage = storage
		_, err := Execute(t.Context(), operation)
		requireFault(t, err, protocol.ErrorCodePermissionDenied)
		if _, err := os.Stat(paths["direct"]); !os.IsNotExist(err) {
			t.Fatalf("computed attachment created a Node profile file: %v", err)
		}
	})
}
