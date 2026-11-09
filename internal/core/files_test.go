package core

import (
	"context"
	"encoding/hex"
	"encoding/json"
	"errors"
	"math"
	"os"
	"path/filepath"
	"reflect"
	"runtime"
	"sort"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func testFiles(t *testing.T) (*Files, string) {
	t.Helper()
	directory, err := filepath.EvalSymlinks(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	files, err := OpenFiles(directory, RestrictedFiles)
	if err != nil {
		t.Fatal(err)
	}
	files.globalConfigs = nil
	files.defaultIgnore = ""
	t.Cleanup(func() { files.Close() })
	return files, directory
}

func putFileFixture(t *testing.T, directory, relative, text string) {
	t.Helper()
	name := filepath.Join(directory, filepath.FromSlash(relative))
	if err := os.MkdirAll(filepath.Dir(name), 0o700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(name, []byte(text), 0o600); err != nil {
		t.Fatal(err)
	}
}

func TestRootRejectsEscape(t *testing.T) {
	t.Parallel()
	t.Run("root acquisition", func(t *testing.T) {
		base, err := filepath.EvalSymlinks(t.TempDir())
		if err != nil {
			t.Fatal(err)
		}
		actual := filepath.Join(base, "actual")
		if err := os.MkdirAll(filepath.Join(actual, "child"), 0o700); err != nil {
			t.Fatal(err)
		}
		if err := os.Symlink(filepath.Join(actual, "child"), filepath.Join(base, "linked-root")); err != nil {
			t.Skipf("symlink fixture unavailable: %v", err)
		}
		if err := os.Symlink(actual, filepath.Join(base, "linked-parent")); err != nil {
			t.Fatal(err)
		}
		for _, directory := range []string{filepath.Join(base, "linked-root"), filepath.Join(base, "linked-parent", "child")} {
			files, err := OpenFiles(directory, RestrictedFiles)
			if err == nil {
				files.Close()
				t.Fatalf("linked root acquisition accepted: %s", directory)
			}
		}
	})
	t.Run("Windows wire names", func(t *testing.T) {
		for _, name := range []string{"NUL", "nul.txt", "CON", "CON .txt", "COM1", "LPT9.log", "COM¹", "LPT².txt", "CONOUT$", "file.", "file ", "a?b"} {
			if windowsFileComponent(name) {
				t.Fatalf("reserved Windows name accepted: %s", name)
			}
		}
		for _, name := range []string{"COM10", "console.rs", "文件.txt", "my file", ".gitignore", "conın$"} {
			if !windowsFileComponent(name) {
				t.Fatalf("ordinary Windows name rejected: %s", name)
			}
		}
	})
	t.Run("managed worktree names", func(t *testing.T) {
		for _, row := range []struct {
			path     string
			reserved bool
		}{
			{".sailry/worktrees/id/file", true},
			{".SAILRY/WORKTREES/id/file", true},
			{".ſailry/worktrees/id/file", false},
			{".sailry/worKtrees/id/file", false},
		} {
			_, err := fileComponents(row.path, false, RestrictedFiles, false)
			if (err != nil) != row.reserved {
				t.Fatalf("managed worktree path %q: %v", row.path, err)
			}
		}
	})
	t.Run("rename entries", func(t *testing.T) {
		files, directory := testFiles(t)
		putFileFixture(t, directory, "source/file", "content")
		if err := files.CreateDirectory("target"); err != nil {
			t.Fatal(err)
		}
		if err := files.Rename("source/file", "source/file"); err != nil {
			t.Fatalf("same-entry rename: %v", err)
		}
		if err := files.Rename("missing", "missing"); !isFileFault(err, protocol.ErrorCodeNotFound) {
			t.Fatalf("missing same-entry rename: %v", err)
		}
		if err := files.Rename("source/file", "target/file"); err != nil {
			t.Fatal(err)
		}
		if err := files.Rename("target", "renamed"); err != nil {
			t.Fatal(err)
		}
		if err := files.Rename("renamed", "renamed"); err != nil {
			t.Fatal(err)
		}
		putFileFixture(t, directory, "existing", "keep")
		if err := files.Rename("renamed/file", "existing"); !isFileFault(err, protocol.ErrorCodeConflict) {
			t.Fatalf("existing target: %v", err)
		}
		if data, err := os.ReadFile(filepath.Join(directory, "existing")); err != nil || string(data) != "keep" {
			t.Fatalf("rename changed existing target: %q, %v", data, err)
		}
	})
	t.Run("rename linked parents", func(t *testing.T) {
		files, directory := testFiles(t)
		putFileFixture(t, directory, "real/file", "keep")
		if err := os.Symlink("real", filepath.Join(directory, "alias")); err != nil {
			t.Skipf("symlink fixture unavailable: %v", err)
		}
		for _, pair := range [][2]string{{"alias/file", "other"}, {"real/file", "alias/other"}} {
			if err := files.Rename(pair[0], pair[1]); err == nil {
				t.Fatalf("linked rename parent accepted: %v", pair)
			}
		}
		if err := files.Rename("alias", "moved-link"); err != nil {
			t.Fatal(err)
		}
		if metadata, err := os.Lstat(filepath.Join(directory, "moved-link")); err != nil || metadata.Mode()&os.ModeSymlink == 0 {
			t.Fatalf("rename did not preserve link: %v, %v", metadata, err)
		}
	})
	for _, scenario := range []struct {
		name             string
		afterPublication bool
		code             protocol.ErrorCode
	}{
		{"rename parent before publication", false, protocol.ErrorCodeRevisionConflict},
		{"rename parent after publication", true, protocol.ErrorCodeOutcomeUnknown},
	} {
		t.Run(scenario.name, func(t *testing.T) {
			files, directory := testFiles(t)
			putFileFixture(t, directory, "source/file", "keep")
			if err := files.CreateDirectory("target"); err != nil {
				t.Fatal(err)
			}
			parent := "source"
			if scenario.afterPublication {
				parent = "target"
			}
			files.renameHook = func(published bool) {
				if published != scenario.afterPublication {
					return
				}
				if err := os.Rename(filepath.Join(directory, parent), filepath.Join(directory, "moved")); err != nil {
					if runtime.GOOS == "windows" && errors.Is(err, os.ErrPermission) {
						t.Skipf("retained directory cannot be replaced on this platform: %v", err)
					}
					t.Fatal(err)
				}
				if err := os.Mkdir(filepath.Join(directory, parent), 0o700); err != nil {
					t.Fatal(err)
				}
			}
			if err := files.Rename("source/file", "target/file"); !isFileFault(err, scenario.code) {
				t.Fatalf("parent replacement outcome: %v", err)
			}
			if data, err := os.ReadFile(filepath.Join(directory, "moved", "file")); err != nil || string(data) != "keep" {
				t.Fatalf("move result lost: %q, %v", data, err)
			}
			if _, err := os.Lstat(filepath.Join(directory, parent, "file")); !errors.Is(err, os.ErrNotExist) {
				t.Fatalf("replacement parent was mutated: %v", err)
			}
		})
	}
	t.Run("paths", func(t *testing.T) {
		files, directory := testFiles(t)
		outside := t.TempDir()
		putFileFixture(t, outside, "private", "outside")
		for _, name := range []string{"../private", filepath.Join(outside, "private"), "dir/../private", ".git/config", ".sailry/worktrees/id/file"} {
			if _, err := files.Save(context.Background(), name, "replacement", nil); err == nil {
				t.Fatalf("unconfined mutation accepted: %s", name)
			}
		}
		if err := os.Symlink(outside, filepath.Join(directory, "linked")); err != nil {
			t.Skipf("symlink fixture unavailable: %v", err)
		}
		for _, name := range []string{"linked/private", "linked/new/file"} {
			if _, err := files.Save(context.Background(), name, "replacement", nil); err == nil {
				t.Fatalf("linked parent accepted: %s", name)
			}
		}
		if original, err := os.ReadFile(filepath.Join(outside, "private")); err != nil || string(original) != "outside" {
			t.Fatalf("outside content changed: %q, %v", original, err)
		}
	})
	t.Run("create and revisions", func(t *testing.T) {
		files, directory := testFiles(t)
		if err := files.CreateDirectory("folder"); err != nil {
			t.Fatal(err)
		}
		if err := files.CreateDirectory("folder"); err != nil {
			t.Fatal(err)
		}
		if err := files.CreateDirectory("absent/child"); !isFileFault(err, protocol.ErrorCodeNotFound) {
			t.Fatalf("missing parent: %v", err)
		}
		created, err := files.Save(context.Background(), "资料/中文 文件.txt", "initial", nil)
		if err != nil {
			t.Fatal(err)
		}
		updated, err := files.Save(context.Background(), created.Path, "updated", &created.Revision)
		if err != nil || updated.Revision != contentRevision([]byte("updated")) {
			t.Fatalf("replacement: %+v, %v", updated, err)
		}
		if _, err := files.Save(context.Background(), created.Path, "stale", &created.Revision); !isFileFault(err, protocol.ErrorCodeRevisionConflict) {
			t.Fatalf("stale revision: %v", err)
		}
		stamp, err := files.SourceRevision(created.Path)
		if err != nil {
			t.Fatal(err)
		}
		if err := files.VerifySource(context.Background(), created.Path, updated.Revision, stamp); err != nil {
			t.Fatal(err)
		}
		putFileFixture(t, directory, created.Path, "external")
		if err := files.VerifySource(context.Background(), created.Path, updated.Revision, stamp); !isFileFault(err, protocol.ErrorCodeRevisionConflict) {
			t.Fatalf("changed source: %v", err)
		}
	})
	for _, scenario := range []struct {
		name             string
		afterPublication bool
		code             protocol.ErrorCode
	}{
		{"concurrent replacement", false, protocol.ErrorCodeRevisionConflict},
		{"later external write", true, protocol.ErrorCodeOutcomeUnknown},
	} {
		t.Run(scenario.name, func(t *testing.T) {
			files, directory := testFiles(t)
			putFileFixture(t, directory, "file", "original")
			expected := contentRevision([]byte("original"))
			target, err := files.PrepareFileTarget(context.Background(), "file", &expected, protocol.MAX_EDIT_BYTES)
			if err != nil {
				t.Fatal(err)
			}
			target.writeHook = func(published bool) {
				if published == scenario.afterPublication {
					putFileFixture(t, directory, "file", "external")
				}
			}
			_, err = target.Write(strings.NewReader("ours"), 4, contentRevision([]byte("ours")))
			if !isFileFault(err, scenario.code) {
				t.Fatalf("publication outcome: %v", err)
			}
			if actual, err := os.ReadFile(filepath.Join(directory, "file")); err != nil || string(actual) != "external" {
				t.Fatalf("external content lost: %q, %v", actual, err)
			}
			entries, err := os.ReadDir(directory)
			if err != nil || len(entries) != 1 {
				t.Fatalf("temporary file retained: %v, %v", entries, err)
			}
		})
	}
	for _, fixture := range []struct {
		name       string
		files      map[string]string
		scope      string
		want       []string
		prepare    func(*testing.T, string)
		wantNotice bool
	}{
		{"nested rules", map[string]string{".gitignore": "*.tmp\n/excluded/\n/root.rs\n", "src/.gitignore": "!keep.tmp\n/nested/skip.rs\n", "root.rs": "", "src/root.rs": "", "src/a.tmp": "", "src/keep.tmp": "", "src/nested/skip.rs": "", "src/nested/use.rs": "", "excluded/private.rs": ""}, "", []string{"src/keep.tmp", "src/nested/use.rs", "src/root.rs"}, nil, false},
		{"override beats deeper rules", map[string]string{".gitignore": "*.rs\n", ".ignore": "!keep.rs\n", "src/.gitignore": "keep.rs\n", "src/keep.rs": "", "src/skip.rs": ""}, "", []string{"src/keep.rs"}, nil, false},
		{"info exclude", map[string]string{".git/config": "[core]\nrepositoryformatversion=0\n", ".git/HEAD": "ref: refs/heads/main\n", ".git/info/exclude": "*.tmp\n", "kept.rs": "", "excluded.tmp": ""}, "", []string{"kept.rs"}, nil, false},
		{"configured excludes", map[string]string{".git/config": "[core]\nexcludesFile=rules\n", "rules": "*.tmp\n", "kept.rs": "", "excluded.tmp": ""}, "", []string{"kept.rs"}, nil, false},
		{"included config", map[string]string{".git/config": "[include]\npath=include\n", ".git/include": "[core]\nexcludesFile=rules\n", "rules": "*.tmp\n", "kept.rs": "", "excluded.tmp": ""}, "", []string{"kept.rs"}, nil, false},
		{"branch conditional config", map[string]string{".git/config": "[includeIf \"onbranch:main\"]\npath=include\n", ".git/HEAD": "ref: refs/heads/main\n", ".git/include": "[core]\nexcludesFile=rules\n", "rules": "*.tmp\n", "kept.rs": "", "excluded.tmp": ""}, "", []string{"kept.rs"}, nil, false},
		{"remote conditional config", map[string]string{".git/config": "[includeIf \"hasconfig:remote.*.url:https://example.test/**\"]\npath=include\n[remote \"origin\"]\nurl=https://example.test/repository\n", ".git/include": "[core]\nexcludesFile=rules\n", "rules": "*.tmp\n", "kept.rs": "", "excluded.tmp": ""}, "", []string{"kept.rs"}, nil, false},
		{"directory conditional config", map[string]string{".git/include": "[core]\nexcludesFile=rules\n", "rules": "*.tmp\n", "kept.rs": "", "excluded.tmp": ""}, "", []string{"kept.rs"}, func(t *testing.T, directory string) {
			putFileFixture(t, directory, ".git/config", "[includeIf \"gitdir:"+filepath.ToSlash(filepath.Join(directory, ".git"))+"/\"]\npath=include\n")
		}, false},
		{"explicit ignored scope", map[string]string{".gitignore": "/ignored/\n", "ignored/private.rs": ""}, "ignored", []string{"ignored/private.rs"}, nil, false},
		{"ancestor layers", map[string]string{"src/.gitignore": "/nested/skip.rs\n", "src/nested/skip.rs": "", "src/nested/use.rs": ""}, "src/nested", []string{"src/nested/use.rs"}, nil, false},
		{"invalid utf8 ignore", map[string]string{".gitignore": "*.rs\n\xff", "kept.rs": ""}, "", []string{"kept.rs"}, nil, true},
		{"oversized ignore", map[string]string{".gitignore": "*.rs\n" + strings.Repeat("#", maxFileIgnoreBytes), "kept.rs": ""}, "", []string{"kept.rs"}, nil, true},
		{"nonregular ignore", map[string]string{".gitignore/child": "", "kept.rs": ""}, "", []string{"kept.rs"}, nil, true},
		{"linked worktree", map[string]string{"visible.rs": "", "excluded.rs": "", "configured.rs": ""}, "", []string{"visible.rs"}, func(t *testing.T, directory string) {
			common, err := filepath.EvalSymlinks(t.TempDir())
			if err != nil {
				t.Fatal(err)
			}
			putFileFixture(t, common, "config", "[core]\nexcludesFile="+filepath.ToSlash(filepath.Join(common, "rules"))+"\n")
			putFileFixture(t, common, "rules", "configured.rs\n")
			putFileFixture(t, common, "info/exclude", "excluded.rs\n")
			putFileFixture(t, common, "worktrees/checkout/commondir", "../..\n")
			putFileFixture(t, directory, ".git", "gitdir: "+filepath.ToSlash(filepath.Join(common, "worktrees/checkout"))+"\n")
		}, false},
		{"linked ignore", map[string]string{"kept.rs": ""}, "", []string{"kept.rs"}, func(t *testing.T, directory string) {
			outside := t.TempDir()
			putFileFixture(t, outside, "rules", "*.rs\n")
			if err := os.Symlink(filepath.Join(outside, "rules"), filepath.Join(directory, ".gitignore")); err != nil {
				t.Skipf("symlink fixture unavailable: %v", err)
			}
		}, true},
	} {
		t.Run("walk/"+fixture.name, func(t *testing.T) {
			files, directory := testFiles(t)
			for name, text := range fixture.files {
				putFileFixture(t, directory, name, text)
			}
			if fixture.prepare != nil {
				fixture.prepare(t, directory)
			}
			globs, err := CompileFileGlobs([]string{"*.rs", "*.tmp"})
			if err != nil {
				t.Fatal(err)
			}
			listing, err := files.CollectFiles(context.Background(), fixture.scope, math.MaxUint64, globs)
			if err != nil {
				t.Fatal(err)
			}
			var paths []string
			for _, entry := range listing.Entries {
				paths = append(paths, entry.Path)
			}
			sort.Strings(paths)
			if !reflect.DeepEqual(paths, fixture.want) {
				t.Fatalf("paths: got %v, want %v", paths, fixture.want)
			}
			if fixture.wantNotice && len(listing.Report.Diagnostics) == 0 {
				t.Fatal("missing ignore diagnostic")
			}
		})
	}
}

func TestTextDecodingFixtures(t *testing.T) {
	t.Parallel()
	data, err := os.ReadFile("testdata/files/text.json")
	if err != nil {
		t.Fatal(err)
	}
	var fixtures []struct{ Name, Hex, Want, Error string }
	if err := json.Unmarshal(data, &fixtures); err != nil {
		t.Fatal(err)
	}
	for _, fixture := range fixtures {
		t.Run(fixture.Name, func(t *testing.T) {
			t.Parallel()
			input, err := hex.DecodeString(fixture.Hex)
			if err != nil {
				t.Fatal(err)
			}
			actual, err := DecodeText(input)
			var wantError error
			if fixture.Error == "binary" {
				wantError = ErrBinaryText
			} else if fixture.Error == "unsupported" {
				wantError = ErrUnsupportedText
			}
			if !errors.Is(err, wantError) || actual != fixture.Want {
				t.Fatalf("decode: %q, %v; want %q, %v", actual, err, fixture.Want, wantError)
			}
		})
	}
	for _, fixture := range []struct {
		name, text    string
		offset, limit uint64
		want          []protocol.FileLine
		next          *uint64
		truncated     bool
	}{
		{"endings", "first\r\nsecond 🙂\nlast", 1, 1, []protocol.FileLine{{Number: 2, Text: "second 🙂\n"}}, fileUint(2), false},
		{"complete", "first\r\nsecond 🙂\nlast", 0, 3, []protocol.FileLine{{Number: 1, Text: "first\r\n"}, {Number: 2, Text: "second 🙂\n"}, {Number: 3, Text: "last"}}, nil, false},
		{"large full hash", strings.Repeat("large line\r\n", 20000), 19998, 200, []protocol.FileLine{{Number: 19999, Text: "large line\r\n"}, {Number: 20000, Text: "large line\r\n"}}, nil, false},
		{"split utf8", strings.Repeat("x", 8191) + "🙂\r\nsecond\nlast", 0, 1, []protocol.FileLine{{Number: 1, Text: strings.Repeat("x", 8191) + "🙂\r\n"}}, fileUint(1), false},
		{"oversized line", strings.Repeat("x", protocol.MAX_FILE_BYTES-1) + "🙂", 0, 200, []protocol.FileLine{{Number: 1, Text: strings.Repeat("x", protocol.MAX_FILE_BYTES-1)}}, nil, true},
		{"oversized skipped", "first\n" + strings.Repeat("x", protocol.MAX_FILE_BYTES+1) + "\nlast", 2, 1, []protocol.FileLine{{Number: 3, Text: "last"}}, nil, false},
	} {
		t.Run("lines/"+fixture.name, func(t *testing.T) {
			files, directory := testFiles(t)
			putFileFixture(t, directory, "file", fixture.text)
			page, err := files.ReadLines(context.Background(), "file", fixture.offset, fixture.limit, nil)
			if err != nil {
				t.Fatal(err)
			}
			if !reflect.DeepEqual(page.Lines, fixture.want) || !reflect.DeepEqual(page.NextOffset, fixture.next) || page.Truncated != fixture.truncated || page.Revision != contentRevision([]byte(fixture.text)) || page.Size != uint64(len(fixture.text)) {
				t.Fatalf("unexpected line page: %+v", page)
			}
		})
	}
	t.Run("unreturned invalid text", func(t *testing.T) {
		files, directory := testFiles(t)
		for _, tail := range []string{"\xf0\x9f", "\xff", "\x00"} {
			putFileFixture(t, directory, "file", "valid first line\n"+tail)
			if _, err := files.ReadLines(context.Background(), "file", 0, 1, nil); !isFileFault(err, protocol.ErrorCodeInvalidRequest) {
				t.Fatalf("invalid tail accepted: %v", err)
			}
		}
		ctx, cancel := context.WithCancel(context.Background())
		cancel()
		if _, err := files.ReadLines(ctx, "file", 0, 1, nil); !isFileFault(err, protocol.ErrorCodeCancelled) {
			t.Fatalf("cancel: %v", err)
		}
	})
}

func fileUint(value uint64) *uint64 { return &value }
