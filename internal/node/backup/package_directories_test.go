package backup

import (
	"archive/tar"
	"bytes"
	"compress/gzip"
	"crypto/sha256"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestRestorePackageWithEmptyDirectories(t *testing.T) {
	t.Parallel()
	ctx := t.Context()
	profile, source := t.TempDir(), t.TempDir()
	const name = "empty-directory-fixture"
	if err := os.WriteFile(filepath.Join(source, "plugin.json"), []byte(`{"$schema":"`+plugins.ManifestSchema+`","name":"`+name+`","version":"1.0.0"}`), 0o600); err != nil {
		t.Fatal(err)
	}
	for _, child := range []string{"a/b", "a/c"} {
		if err := os.MkdirAll(filepath.Join(source, child), 0o700); err != nil {
			t.Fatal(err)
		}
	}
	installed, err := plugins.NewManager(profile).Install(ctx, source, "", name)
	if err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(profile, identityName), bytes.Repeat([]byte{1}, 32), 0o600); err != nil {
		t.Fatal(err)
	}
	root := "plugins/packages/" + installed.Summary.Digest
	snapshot := fixtureSnapshot(t, profile, map[string][]byte{"storage/node.turso": []byte("node"), "storage/knowledge.turso": []byte("knowledge")}, []string{identityName, root})
	var plain bytes.Buffer
	if err := snapshot.archive(ctx, &plain); err != nil {
		t.Fatal(err)
	}
	staged, err := Stage(ctx, &plain, t.TempDir(), nil, "fixture", 0, "")
	if err != nil {
		t.Fatal(err)
	}
	defer staged.Close()
	target := filepath.Join(t.TempDir(), "restored")
	if err := Finish(ctx, staged, target, nil); err != nil {
		t.Fatal(err)
	}
	for _, child := range []string{"a/b", "a/c"} {
		info, err := os.Lstat(filepath.Join(target, root, child))
		if err != nil || !info.IsDir() || runtime.GOOS != "windows" && info.Mode().Perm() != 0o700 {
			t.Fatalf("restored directory %s: %v, %v", child, info, err)
		}
	}
	verified, err := plugins.NewManager(target).Install(ctx, source, "", name)
	if err != nil || verified.Summary.Digest != installed.Summary.Digest {
		t.Fatalf("restored immutable package failed verification: %s, %v", verified.Summary.Digest, err)
	}
}

func TestUnpackPackageDirectoryEntries(t *testing.T) {
	t.Parallel()
	ctx := t.Context()
	sum := fmt.Sprintf("%x", sha256.Sum256(nil))
	dir := "plugins/packages/" + sum + "/empty"
	manifest := Manifest{Version: 1, OriginalProfile: t.TempDir(), EmptyPackageDirs: []string{dir},
		Files: []File{{Path: identityName, SHA256: sum, Mode: 0o600}, {Path: "storage/node.turso", SHA256: sum, Mode: 0o600}}}
	body, err := json.Marshal(manifest)
	if err != nil {
		t.Fatal(err)
	}
	for _, test := range []struct {
		name string
		dirs []string
		mode int64
	}{
		{"missing", nil, 0o700},
		{"unexpected", []string{dir + "-other"}, 0o700},
		{"duplicate", []string{dir, dir}, 0o700},
		{"public mode", []string{dir}, 0o755},
	} {
		t.Run(test.name, func(t *testing.T) {
			var plain bytes.Buffer
			compressed := gzip.NewWriter(&plain)
			archive := tar.NewWriter(compressed)
			if err := appendEntry(ctx, archive, manifestName, uint64(len(body)), 0o600, bytes.NewReader(body)); err != nil {
				t.Fatal(err)
			}
			for _, file := range manifest.Files {
				if err := appendEntry(ctx, archive, file.Path, 0, file.Mode, bytes.NewReader(nil)); err != nil {
					t.Fatal(err)
				}
			}
			for _, path := range test.dirs {
				if err := archive.WriteHeader(&tar.Header{Typeflag: tar.TypeDir, Name: path, Mode: test.mode}); err != nil {
					t.Fatal(err)
				}
			}
			if err := errors.Join(archive.Close(), compressed.Close()); err != nil {
				t.Fatal(err)
			}
			if _, err := Unpack(ctx, &plain, t.TempDir()); code(err) != protocol.ErrorCodeInvalidRequest {
				t.Fatalf("accepted inconsistent package directory entry: %v", err)
			}
		})
	}
}

func TestBackupPackageDirectoryBoundaries(t *testing.T) {
	t.Parallel()
	digest := strings.Repeat("a", 64)
	root := "plugins/packages/" + digest
	for _, test := range []struct {
		name string
		dirs []string
		file string
		link string
	}{
		{name: "outside packages", dirs: []string{"storage/empty"}},
		{name: "package root", dirs: []string{root}},
		{name: "not a digest", dirs: []string{"plugins/packages/not-a-digest/a"}},
		{name: "traversal", dirs: []string{root + "/../a"}},
		{name: "duplicate", dirs: []string{root + "/a", root + "/a"}},
		{name: "nested directories", dirs: []string{root + "/a/b", root + "/a"}},
		{name: "file below empty directory", dirs: []string{root + "/a"}, file: root + "/a/b"},
		{name: "directory in place of file", dirs: []string{root + "/a"}, file: root + "/a"},
		{name: "directory below link", dirs: []string{root + "/a/b"}, link: root + "/a"},
	} {
		t.Run(test.name, func(t *testing.T) {
			manifest := Manifest{Version: 1, OriginalProfile: t.TempDir(), EmptyPackageDirs: test.dirs,
				Files: []File{{Path: identityName, SHA256: digest}, {Path: "storage/node.turso", SHA256: digest}}}
			if test.file != "" {
				manifest.Files = append(manifest.Files, File{Path: test.file, SHA256: digest})
			}
			if test.link != "" {
				manifest.Links = []Link{{Path: test.link, Target: "elsewhere"}}
			}
			if err := manifest.Validate(); code(err) != protocol.ErrorCodeInvalidRequest {
				t.Fatalf("accepted invalid package directory: %v", err)
			}
		})
	}
}
