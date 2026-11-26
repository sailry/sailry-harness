package updater

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestWindowsPackageReconcilesOnlyOwnedFiles(t *testing.T) {
	for _, collision := range []bool{false, true} {
		name := "success"
		if collision {
			name = "conflict"
		}
		t.Run(name, func(t *testing.T) {
			base := t.TempDir()
			src, installed, backup := filepath.Join(base, "new"), filepath.Join(base, "installed"), filepath.Join(base, "backup")
			write := func(dir, path, value string) {
				t.Helper()
				path = filepath.Join(dir, path)
				if err := os.MkdirAll(filepath.Dir(path), 0o700); err != nil {
					t.Fatal(err)
				}
				if err := os.WriteFile(path, []byte(value), 0o600); err != nil {
					t.Fatal(err)
				}
			}
			check := func(dir, path, want string) {
				t.Helper()
				data, err := os.ReadFile(filepath.Join(dir, path))
				if err != nil || string(data) != want {
					t.Fatalf("%s = %q, %v", path, data, err)
				}
			}
			write(installed, "build.json", `{"files":["build.json","app.exe","resources/retired","resources/current"]}`)
			write(installed, "app.exe", "old")
			write(installed, "resources/retired", "retired")
			write(installed, "resources/current", "old resource")
			write(installed, "resources/mine", "user")
			write(installed, "Uninstall.exe", "uninstaller")
			write(installed, ".mygo-files", "app.exe\r\nresources\r\nnative-plugin.dll\r\nold-plugin.dll\r\n")
			write(installed, "native-plugin.dll", "old plugin")
			write(installed, "old-plugin.dll", "retired plugin")
			write(src, ".mygo-files", "app.exe\nresources\nnative-plugin.dll\n")
			write(src, "native-plugin.dll", "updated plugin")
			write(src, "app.exe", "new")
			write(src, "resources/current", "new resource")
			write(src, "resources/new", "added")
			if collision {
				write(installed, "resources/new", "user collision")
			}
			err := applyWindowsFiles(src, installed, backup)
			check(installed, "Uninstall.exe", "uninstaller")
			check(installed, "resources/mine", "user")
			if collision {
				if err == nil || !strings.Contains(err.Error(), "conflicts") {
					t.Fatalf("conflicting file: %v", err)
				}
				check(installed, "app.exe", "old")
				check(installed, "resources/new", "user collision")
				return
			}
			if err != nil {
				t.Fatal(err)
			}
			check(installed, "app.exe", "new")
			check(installed, "resources/new", "added")
			check(backup, "app.exe", "old")
			check(backup, "resources/retired", "retired")
			check(installed, "native-plugin.dll", "updated plugin")
			check(backup, "native-plugin.dll", "old plugin")
			check(backup, "old-plugin.dll", "retired plugin")
			if _, err := os.Stat(filepath.Join(installed, "old-plugin.dll")); !os.IsNotExist(err) {
				t.Fatal("retired plugin still installed")
			}
			if _, err := os.Stat(filepath.Join(installed, "resources/retired")); !os.IsNotExist(err) {
				t.Fatal("retired file still installed")
			}
		})
	}
}
