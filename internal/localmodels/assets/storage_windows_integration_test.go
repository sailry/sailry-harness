//go:build windows && integration

package assets

import (
	"crypto/sha256"
	"errors"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"

	"golang.org/x/sys/windows"
)

func TestWindowsAssetDirectoryAliases(t *testing.T) {
	for _, name := range []string{"case", "short"} {
		t.Run(name, func(t *testing.T) {
			base := filepath.Join(t.TempDir(), "Model Assets Fixture")
			if err := os.Mkdir(base, 0o700); err != nil {
				t.Fatal(err)
			}
			canonical, err := filepath.EvalSymlinks(base)
			if err != nil {
				t.Fatal(err)
			}
			alias := strings.ToLower(canonical)
			if name == "short" {
				input, err := windows.UTF16PtrFromString(canonical)
				if err != nil {
					t.Fatal(err)
				}
				size, err := windows.GetShortPathName(input, nil, 0)
				if err != nil {
					t.Fatal(err)
				}
				buffer := make([]uint16, size)
				written, err := windows.GetShortPathName(input, &buffer[0], size)
				if err != nil || written >= size {
					t.Fatalf("read short path: length=%d, capacity=%d, error=%v", written, size, err)
				}
				alias = windows.UTF16ToString(buffer[:written])
				if alias == canonical {
					t.Skip("fixture volume does not expose an 8.3 alias")
				}
			}
			if alias == canonical {
				t.Fatal("fixture does not change the directory spelling")
			}
			directory, canonical := filepath.Join(alias, "models"), filepath.Join(canonical, "models")
			const body = "pinned model fixture"
			file := File{Name: "model.bin", Size: uint64(len(body)), SHA256: fmt.Sprintf("%x", sha256.Sum256([]byte(body)))}
			files := []File{file}
			downloads := []Download{{File: file, URL: &url.URL{Scheme: "https", Host: "models.invalid", Path: "/model.bin"}}}
			requests := 0
			client := Client(failingTransport(func(*http.Request) (*http.Response, error) {
				requests++
				return &http.Response{StatusCode: http.StatusOK, ContentLength: int64(len(body)), Body: io.NopCloser(strings.NewReader(body)), Header: http.Header{}}, nil
			}))
			if err := Prepare(t.Context(), client, directory, downloads, func(Progress) {}); err != nil {
				t.Fatalf("prepare through alias %q: %v", directory, err)
			}
			expected := Candidate(directory, files)
			if expected == nil {
				t.Fatal("alias did not produce a validation candidate")
			}
			stamp, err := Mark(directory, files, "fixture", expected)
			if err != nil {
				t.Fatalf("mark through alias: %v", err)
			}
			for _, path := range []string{directory, canonical} {
				observed := StampOf(path, files, "fixture")
				if !Ready(path, files) || !stamp.Equal(observed) || !Recorded(path, "fixture", observed) {
					t.Fatalf("directory spelling changed the verified installation: %q", path)
				}
			}
			if err := Prepare(t.Context(), client, canonical, downloads, func(Progress) {}); err != nil || requests != 1 {
				t.Fatalf("reopen redownloaded verified files: requests=%d, error=%v", requests, err)
			}
		})
	}
}

func TestWindowsAssetDirectoryJunctions(t *testing.T) {
	base := t.TempDir()
	target, junction := filepath.Join(base, "target"), filepath.Join(base, "junction")
	for _, path := range []string{target, filepath.Join(target, "models")} {
		if err := os.MkdirAll(path, 0o700); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(filepath.Join(path, "model.bin"), []byte{1}, 0o600); err != nil {
			t.Fatal(err)
		}
	}
	if output, err := exec.CommandContext(t.Context(), "cmd.exe", "/d", "/c", "mklink", "/J", junction, target).CombinedOutput(); err != nil {
		t.Fatalf("create junction fixture: %s, %v", output, err)
	}
	t.Cleanup(func() {
		if err := os.Remove(junction); err != nil {
			t.Error(err)
		}
	})
	files := []File{{Name: "model.bin", Size: 1, SHA256: fmt.Sprintf("%x", sha256.Sum256([]byte{1}))}}
	for _, path := range []string{junction, filepath.Join(junction, "models")} {
		root, err := open(path, false)
		if root != nil {
			root.Close()
		}
		if !errors.Is(err, ErrStorage) || Ready(path, files) || Candidate(path, files) != nil {
			t.Fatalf("directory access followed a junction: %q, error=%v", path, err)
		}
		if err := WriteMarker(path, "fixture"); !errors.Is(err, ErrStorage) {
			t.Fatalf("marker write followed a junction: %q, error=%v", path, err)
		}
	}
	for _, path := range []string{target, filepath.Join(target, "models")} {
		if _, err := os.Stat(filepath.Join(path, "validated")); !errors.Is(err, os.ErrNotExist) {
			t.Fatalf("junction refusal changed its target: %q, error=%v", path, err)
		}
	}
}
