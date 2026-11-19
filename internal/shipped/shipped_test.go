package shipped

import (
	"encoding/json"
	"errors"
	"io/fs"
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"syscall"
	"testing"
)

func TestStagedDirectoryFindsExecutableCheckout(t *testing.T) {
	t.Parallel()
	const name = "native-runtime.test"
	checkout := t.TempDir()
	staging := filepath.Join(checkout, ".runtime", "native", runtime.GOOS+"-"+runtime.GOARCH)
	if err := os.MkdirAll(staging, 0o700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(staging, name), []byte("runtime"), 0o600); err != nil {
		t.Fatal(err)
	}
	executableDirectory := filepath.Join(checkout, "bin", "dev")
	if got := StagedDirectory(name, t.TempDir(), executableDirectory); got != staging {
		t.Fatalf("executable checkout library = %q; want %q", got, staging)
	}
}

func TestSeal(t *testing.T) {
	for _, location := range []struct {
		goos, executable, directory string
		bundle                      bool
	}{
		{"darwin", "/Applications/Sailry.app/Contents/MacOS/Sailry", "/Applications/Sailry.app/Contents/Resources", true},
		{"darwin", "/opt/sailry/sailry-host", "/opt/sailry", false},
		{"darwin", "/opt/MacOS/sailry-host", "/opt/MacOS", false},
		{"linux", "/opt/Sailry.app/Contents/MacOS/Sailry", "/opt/Sailry.app/Contents/MacOS", false},
	} {
		if directory, bundle := locate(location.goos, filepath.FromSlash(location.executable)); directory != filepath.FromSlash(location.directory) || bundle != location.bundle {
			t.Errorf("locate(%s, %s) = %s, %v", location.goos, location.executable, directory, bundle)
		}
	}

	directory, err := filepath.EvalSymlinks(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	library := filepath.Join(directory, "libexample.dylib")
	if err := os.WriteFile(library, []byte("signed library"), 0o600); err != nil {
		t.Fatal(err)
	}
	record, err := Record(directory, "libexample.dylib")
	if err != nil || record.Size != 14 || len(record.SHA256) != 64 {
		t.Fatalf("record = %+v, %v", record, err)
	}
	write := func(body string) {
		t.Helper()
		if err := os.WriteFile(filepath.Join(directory, SealName), []byte(body), 0o600); err != nil {
			t.Fatal(err)
		}
	}
	encode := func(seal Seal) string {
		body, err := json.Marshal(seal)
		if err != nil {
			t.Fatal(err)
		}
		return string(body)
	}

	// Without a seal the upstream pins apply, in a bundle or not.
	if _, err := Read(directory, "darwin/arm64"); !errors.Is(err, fs.ErrNotExist) {
		t.Errorf("missing seal = %v", err)
	}
	if seal, err := sealed(directory, true, directory, "darwin/arm64"); seal != nil || err != nil {
		t.Errorf("bundle without a seal = %v, %v", seal, err)
	}

	valid := Seal{Version: 1, Target: "darwin/arm64", Files: []File{record}}
	linked := record
	linked.Name = "liblinked.dylib"
	for name, body := range map[string]string{
		"not JSON":       "{",
		"empty":          "{}",
		"version":        encode(Seal{Version: 2, Target: valid.Target, Files: valid.Files}),
		"no files":       encode(Seal{Version: 1, Target: valid.Target}),
		"unknown field":  strings.Replace(encode(valid), `"version"`, `"extra":1,"version"`, 1),
		"path":           encode(Seal{Version: 1, Target: valid.Target, Files: []File{{Name: "../libexample.dylib", Size: record.Size, SHA256: record.SHA256}}}),
		"duplicate":      encode(Seal{Version: 1, Target: valid.Target, Files: []File{record, record}}),
		"empty file":     encode(Seal{Version: 1, Target: valid.Target, Files: []File{{Name: record.Name, SHA256: record.SHA256}}}),
		"short digest":   encode(Seal{Version: 1, Target: valid.Target, Files: []File{{Name: record.Name, Size: record.Size, SHA256: record.SHA256[:62]}}}),
		"other platform": encode(Seal{Version: 1, Target: "linux/amd64", Files: valid.Files}),
		"oversized":      encode(valid) + strings.Repeat(" ", 64*1024),
	} {
		write(body)
		if seal, err := Read(directory, valid.Target); err == nil || errors.Is(err, fs.ErrNotExist) {
			t.Errorf("%s: seal = %v, %v", name, seal, err)
		}
		// An invalid seal in the running bundle never falls back to the
		// upstream pins.
		if seal, err := sealed(directory, true, directory, valid.Target); seal != nil || err == nil {
			t.Errorf("%s: bundle seal = %v, %v", name, seal, err)
		}
	}

	write(encode(Seal{Version: 1, Target: valid.Target, Files: []File{record, linked}}))
	seal, err := sealed(directory, true, filepath.Join(directory, "."), valid.Target)
	if err != nil || seal == nil {
		t.Fatalf("bundle seal = %v, %v", seal, err)
	}
	// The seal counts only for the running bundle's own directory.
	if seal, err := sealed(directory, false, directory, valid.Target); seal != nil || err != nil {
		t.Errorf("seal outside a bundle = %v, %v", seal, err)
	}
	if seal, err := sealed(filepath.Join(directory, "other"), true, directory, valid.Target); seal != nil || err != nil {
		t.Errorf("seal of another directory = %v, %v", seal, err)
	}

	if err := seal.Check(directory, "libexample.dylib"); err != nil {
		t.Errorf("sealed library: %v", err)
	}
	if err := seal.Check(directory, "libother.dylib"); err == nil || !strings.Contains(err.Error(), "not in the package seal") {
		t.Errorf("unlisted library: %v", err)
	}
	t.Run("symlink", func(t *testing.T) {
		if err := os.Symlink(library, filepath.Join(directory, linked.Name)); err != nil {
			const errorPrivilegeNotHeld = syscall.Errno(1314)
			if runtime.GOOS == "windows" && errors.Is(err, errorPrivilegeNotHeld) {
				t.Skipf("Windows symlink privilege is unavailable: %v", err)
			}
			t.Fatal(err)
		}
		if _, err := Record(directory, linked.Name); err == nil {
			t.Error("a symbolic link was recorded")
		}
		if err := seal.Check(directory, linked.Name); err == nil {
			t.Error("a symbolic link passed the seal")
		}
	})
	// Same size, different content; then a different size.
	for _, body := range []string{"signed librarY", "signed library and more"} {
		if err := os.WriteFile(library, []byte(body), 0o600); err != nil {
			t.Fatal(err)
		}
		if err := seal.Check(directory, "libexample.dylib"); err == nil || !strings.Contains(err.Error(), "does not match the package seal") {
			t.Errorf("changed library %q: %v", body, err)
		}
	}
	if err := os.Remove(library); err != nil {
		t.Fatal(err)
	}
	if err := seal.Check(directory, "libexample.dylib"); !errors.Is(err, fs.ErrNotExist) {
		t.Errorf("removed library: %v", err)
	}
}
