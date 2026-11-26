package files

import (
	"context"
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func filesFixture(t testing.TB) (string, *core.Files) {
	t.Helper()
	root, err := filepath.EvalSymlinks(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	f, err := core.OpenFiles(root, core.ManualFiles)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = f.Close() })
	return root, f
}
func TestFileTextKernels(t *testing.T) {
	t.Parallel()
	for _, test := range []struct {
		name, before, old, new, want string
		all, fail                    bool
	}{{"CRLF line boundary", "a\r\nb\r\n", "\nb\n", "\nc\n", "a\r\nc\r\n", false, false}, {"ambiguity", "same same", "same", "next", "", false, true}, {"replace all", "same same", "same", "next", "next next", true, false}, {"overlapping occurrences", "aaa", "aa", "x", "", false, true}} {
		t.Run(test.name, func(t *testing.T) {
			after, err := PrepareEdits(test.before, []protocol.FileEdit{{OldText: test.old, NewText: test.new, ReplaceAll: test.all}})
			if (err != nil) != test.fail || !test.fail && after != test.want {
				t.Fatalf("result %q, error %v", after, err)
			}
		})
	}
	patch := "*** Begin Patch\n*** Update File: note.txt\n@@\n a\n-b\n+c\n*** End Patch"
	parsed, err := ParsePatch(patch)
	if err != nil || len(parsed) != 1 {
		t.Fatalf("parse: %v", err)
	}
	after, err := ApplyPatchChunks("a\r\nb\r\n", parsed[0].Chunks)
	if err != nil || after != "a\r\nc\r\n" {
		t.Fatalf("patch %q: %v", after, err)
	}
	if after, err = ApplyPatchChunks("a\nx\na\n", []PatchChunk{{Old: []string{"a"}, New: []string{"b"}}}); err != nil || after != "b\nx\na\n" {
		t.Fatalf("first matching location: %q %v", after, err)
	}
	anchor := "header"
	if after, err = ApplyPatchChunks("header\nbody\n", []PatchChunk{{Context: &anchor, New: []string{"last"}}}); err != nil || after != "header\nbody\nlast\n" {
		t.Fatalf("anchored append: %q %v", after, err)
	}
	if after, err = ApplyPatchChunks("hello\u2014\u201cworld\u201d\n", []PatchChunk{{Old: []string{`hello-"world"`}, New: []string{"updated"}}}); err != nil || after != "updated\n" {
		t.Fatalf("normalized punctuation: %q %v", after, err)
	}
	if after, err = ApplyPatchChunks("func main() {\n\tfmt.Println(\"hi\")   \n}\n", []PatchChunk{{Old: []string{"fmt.Println(\"hi\")"}, New: []string{"\tfmt.Println(\"bye\")"}}}); err != nil || !strings.Contains(after, "bye") {
		t.Fatalf("fuzzy patch failed: %q %v", after, err)
	}
	// Fuzzy patch matching preserves original text of untouched context lines (e.g., tabs in Makefile/Python).
	tabFile := "all:\n\techo one\n\techo two\n\techo three\n"
	tabPatch := "*** Begin Patch\n*** Update File: Makefile\n@@\n     echo one\n-    echo two\n+\techo TWO\n     echo three\n*** End Patch"
	parsedTab, err := ParsePatch(tabPatch)
	if err != nil || len(parsedTab) != 1 {
		t.Fatalf("parse tab patch: %v", err)
	}
	if after, err = ApplyPatchChunks(tabFile, parsedTab[0].Chunks); err != nil || after != "all:\n\techo one\n\techo TWO\n\techo three\n" {
		t.Fatalf("fuzzy patch rewrote untouched context lines: got %q, err=%v", after, err)
	}
	// Duplicate edit errors report match count and line numbers.
	if _, err = PrepareEdits("alpha\nbeta\nalpha\n", []protocol.FileEdit{{OldText: "alpha", NewText: "gamma"}}); err == nil || !strings.Contains(err.Error(), "2 matches") || !strings.Contains(err.Error(), "1, 3") {
		t.Fatalf("duplicate edit diagnostics: %v", err)
	}
	// Whole-file edit fast path.
	if after, err = PrepareEdits("line1\nline2\n", []protocol.FileEdit{{OldText: "line1\nline2\n", NewText: "replaced\n"}}); err != nil || after != "replaced\n" {
		t.Fatalf("whole-file edit: %q %v", after, err)
	}
	// Whole-file fast path still rejects empty old_text, NUL bytes, and oversized budgets.
	if _, err = PrepareEdits("", []protocol.FileEdit{{OldText: "", NewText: "x"}}); err == nil {
		t.Fatal("whole-file edit accepted empty old_text on empty file")
	}
	if _, err = PrepareEdits("a\x00b", []protocol.FileEdit{{OldText: "a\x00b", NewText: "clean"}}); err == nil {
		t.Fatal("whole-file edit accepted NUL byte in before/old_text")
	}
	halfMax := strings.Repeat("a", protocol.MAX_EDIT_BYTES/2+1)
	if _, err = PrepareEdits(halfMax, []protocol.FileEdit{{OldText: halfMax, NewText: halfMax}}); err == nil {
		t.Fatal("whole-file edit bypassed MAX_EDIT_BYTES budget")
	}
	// Controlled fuzzy edit: unique trailing-whitespace mismatch succeeds.
	if after, err = PrepareEdits("alpha   \nbeta\t\ngamma\n", []protocol.FileEdit{{OldText: "alpha\nbeta", NewText: "alpha\nBETA"}}); err != nil || after != "alpha\nBETA\ngamma\n" {
		t.Fatalf("trailing-whitespace fuzzy edit: %q %v", after, err)
	}
	// Controlled fuzzy edit: unique uniform indentation shift succeeds and re-indents new_text.
	if after, err = PrepareEdits("func f() {\n\tif ok {\n\t\treturn 1\n\t}\n}\n", []protocol.FileEdit{{OldText: "if ok {\n\treturn 1\n}", NewText: "if ok {\n\treturn 2\n}"}}); err != nil || after != "func f() {\n\tif ok {\n\t\treturn 2\n\t}\n}\n" {
		t.Fatalf("indent-shift fuzzy edit: %q %v", after, err)
	}
	// Controlled fuzzy edit: mixed tab+space indent shift preserves tab prefix order.
	if after, err = PrepareEdits("func f() {\n\t    x := 1\n\t    y := 2\n}\n", []protocol.FileEdit{{OldText: "    x := 1\n    y := 2", NewText: "    x := 10\n    y := 20"}}); err != nil || after != "func f() {\n\t    x := 10\n\t    y := 20\n}\n" {
		t.Fatalf("mixed tab+space indent-shift fuzzy edit: %q %v", after, err)
	}
	// Controlled fuzzy edit: ambiguous trailing-whitespace matches are rejected with line numbers.
	if _, err = PrepareEdits("dup  \nx\ndup\t\n", []protocol.FileEdit{{OldText: "dup\n", NewText: "one\n"}}); err == nil || !strings.Contains(err.Error(), "2 locations") || !strings.Contains(err.Error(), "1, 3") {
		t.Fatalf("ambiguous fuzzy edit should fail: %v", err)
	}
	// Controlled fuzzy edit: replace_all stays exact (never applies fuzzy fallback).
	if _, err = PrepareEdits("dup  \ndup  \n", []protocol.FileEdit{{OldText: "dup\n", NewText: "one\n", ReplaceAll: true}}); err == nil {
		t.Fatal("replace_all should not use fuzzy fallback")
	}
	hunks, err := SplitHunks("note.txt", "one\ntwo\nthree\n", "one\nnext\nthree\n", 0)
	if err != nil || len(hunks) != 1 || !validRevision(hunks[0].Hunk.ID) {
		t.Fatalf("hunks: %+v %v", hunks, err)
	}
	restored, err := ApplyHunkText("outside\none\nnext\nthree\n", hunks[0].After, hunks[0].Before)
	if err != nil || restored != "outside\none\ntwo\nthree\n" {
		t.Fatalf("hunk restore %q: %v", restored, err)
	}
}
func TestPreparedFileChanges(t *testing.T) {
	t.Parallel()
	root, _ := filesFixture(t)
	ctx := context.Background()
	if err := os.WriteFile(filepath.Join(root, "first.txt"), []byte("before\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	expected := revision([]byte("before\n"))
	command := protocol.Command{Kind: "apply_changes", Data: &protocol.CommandApplyChangesPayload{Changes: []protocol.FileChange{{Kind: "write", Data: protocol.FileChangeWritePayload{Path: "first.txt", Text: "after\n", ExpectedRevision: expected}}, {Kind: "create", Data: protocol.FileChangeCreatePayload{Path: "second.txt", Text: "ours\n"}}, {Kind: "create", Data: protocol.FileChangeCreatePayload{Path: "third.txt", Text: "later\n"}}}}}
	prepared, err := Prepare(ctx, root, command, core.ManualFiles)
	if err != nil || prepared.Rejected != nil {
		t.Fatalf("prepare: %+v %v", prepared, err)
	}
	if err = os.WriteFile(filepath.Join(root, "second.txt"), []byte("external\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	out, err := Apply(ctx, root, prepared)
	if err != nil {
		t.Fatal(err)
	}
	result := out.Data.(protocol.FileChanges)
	if result.Status != protocol.ChangesStatusPartial || result.Files[0].Status != protocol.ChangeStatusApplied || result.Files[1].Status != protocol.ChangeStatusFailed || result.Files[2].Status != protocol.ChangeStatusNotAttempted {
		t.Fatalf("effects: %+v", result)
	}
	data, err := os.ReadFile(filepath.Join(root, "second.txt"))
	if err != nil || string(data) != "external\n" {
		t.Fatalf("external content changed: %q %v", data, err)
	}
	if _, err = os.Stat(filepath.Join(root, "third.txt")); !os.IsNotExist(err) {
		t.Fatalf("unattempted file exists: %v", err)
	}
	patch := "*** Begin Patch\n*** Update File: first.txt\n@@\n-after\n+next\n*** End Patch"
	prepared, err = PreparePatch(ctx, root, patch, protocol.WorktreeID{}, core.ManualFiles)
	if err != nil || *prepared.Changes[0].Before != "after\n" {
		t.Fatalf("prepare patch directly against current text: %+v, %v", prepared, err)
	}
	if err := os.WriteFile(filepath.Join(root, "first.txt"), []byte("newer\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	out, err = Apply(ctx, root, prepared)
	if err != nil || out.Data.(protocol.FileChanges).Files[0].Error.Code != protocol.ErrorCodeRevisionConflict {
		t.Fatalf("file changed after patch approval: %+v %v", out, err)
	}
}
func TestPatchOverwritesAndMovesInOrder(t *testing.T) {
	root, _ := filesFixture(t)
	for name, content := range map[string]string{"a": "source\n", "b": "destination\n", "c": "existing\n"} {
		if err := os.WriteFile(filepath.Join(root, name), []byte(content), 0o600); err != nil {
			t.Fatal(err)
		}
	}
	patch := `*** Begin Patch
*** Update File: a
*** Move to: b
@@
-source
+moved
*** Update File: b
@@
-moved
+changed
*** Add File: c
+replaced
*** Add File: new/nested.txt
+nested
*** End Patch`
	ctx := context.Background()
	p, err := PreparePatch(ctx, root, patch, protocol.WorktreeID{}, core.ManualFiles)
	if err != nil {
		t.Fatal(err)
	}
	out, err := Apply(ctx, root, p)
	if err != nil || out.Data.(protocol.FileChanges).Status != protocol.ChangesStatusApplied {
		t.Fatalf("apply: %+v %v", out, err)
	}
	for name, want := range map[string]string{"b": "changed\n", "c": "replaced\n", "new/nested.txt": "nested\n"} {
		got, err := os.ReadFile(filepath.Join(root, name))
		if err != nil || string(got) != want {
			t.Fatalf("%s = %q, %v", name, got, err)
		}
	}
	if _, err := os.Stat(filepath.Join(root, "a")); !os.IsNotExist(err) {
		t.Fatalf("move source: %v", err)
	}
}

func TestPatchMovesCaseVariantsAndHardLinks(t *testing.T) {
	for _, tt := range []struct {
		name, destination, chunk, want string
		link                           bool
	}{
		{"nested rename", "new/dir/renamed", "", "original\n", false},
		{"case rename", "foo", "", "original\n", false},
		{"case rename with edit", "foo", "@@\n-original\n+edited\n", "edited\n", false},
		{"move over hard link", "hardlink", "", "original\n", true},
		{"edit and move over hard link", "hardlink", "@@\n-original\n+edited\n", "edited\n", true},
	} {
		t.Run(tt.name, func(t *testing.T) {
			root, _ := filesFixture(t)
			source := filepath.Join(root, "Foo")
			if err := os.WriteFile(source, []byte("original\n"), 0o700); err != nil {
				t.Fatal(err)
			}
			destination := filepath.Join(root, tt.destination)
			if tt.link {
				if err := os.Link(source, destination); err != nil {
					t.Skipf("hard links unavailable: %v", err)
				}
				if err := os.Link(source, filepath.Join(root, "retained")); err != nil {
					t.Fatal(err)
				}
			}
			ctx := context.Background()
			patch := "*** Begin Patch\n*** Update File: Foo\n*** Move to: " + tt.destination + "\n" + tt.chunk + "*** End Patch"
			prepared, err := PreparePatch(ctx, root, patch, protocol.WorktreeID{}, core.ManualFiles)
			if err != nil {
				t.Fatal(err)
			}
			out, err := Apply(ctx, root, prepared)
			if err != nil || out.Data.(protocol.FileChanges).Status != protocol.ChangesStatusApplied {
				t.Fatalf("move: %+v %v", out, err)
			}
			entries, err := os.ReadDir(root)
			if err != nil {
				t.Fatal(err)
			}
			for _, entry := range entries {
				if entry.Name() == "Foo" {
					t.Fatal("original directory entry still exists")
				}
			}
			if got, err := os.ReadFile(destination); err != nil || string(got) != tt.want {
				t.Fatalf("destination: %q %v", got, err)
			}
			if tt.link {
				if got, err := os.ReadFile(filepath.Join(root, "retained")); err != nil || string(got) != "original\n" {
					t.Fatalf("retained hard link: %q %v", got, err)
				}
			}
		})
	}
}

func TestMoveCreatesPatchParentsOnlyAfterRevisionCheck(t *testing.T) {
	root, fs := filesFixture(t)
	path := filepath.Join(root, "source")
	if err := os.WriteFile(path, []byte("original\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	ctx := context.Background()
	dst := "new/target"
	if err := fs.MoveFile(ctx, "source", dst, revision([]byte("original\n")), false); err == nil {
		t.Fatal("ordinary move created missing parent")
	}
	p, err := PreparePatch(ctx, root, "*** Begin Patch\n*** Update File: source\n*** Move to: "+dst+"\n*** End Patch", protocol.WorktreeID{}, core.ManualFiles)
	if err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(path, []byte("changed\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	out, err := Apply(ctx, root, p)
	if err != nil || out.Data.(protocol.FileChanges).Status != protocol.ChangesStatusRejected {
		t.Fatalf("stale move: %+v %v", out, err)
	}
	if _, err := os.Stat(filepath.Join(root, "new")); !os.IsNotExist(err) {
		t.Fatalf("stale move created parent: %v", err)
	}
}

func TestNotebookPreparation(t *testing.T) {
	t.Parallel()
	draft := `{"nbformat":4,"nbformat_minor":5,"metadata":{},"cells":[{"cell_type":"code","metadata":{},"source":["old\n"],"outputs":[{"output_type":"stream","text":"cached"}],"execution_count":2}]}`
	prepared, err := PrepareCreation("note.ipynb", draft)
	if err != nil {
		t.Fatal(err)
	}
	stable, err := PrepareCreation("note.ipynb", draft)
	if err != nil || stable != prepared {
		t.Fatalf("unstable IDs: %v", err)
	}
	edit := protocol.NotebookEdit{Path: "note.ipynb", ExpectedRevision: revision([]byte(prepared)), Edits: []protocol.NotebookChange{{Kind: "replace", Data: protocol.NotebookChangeReplacePayload{Index: 0, Source: "next\n"}}}}
	after, err := PrepareNotebook(prepared, edit)
	if err != nil {
		t.Fatal(err)
	}
	var document struct {
		Cells []struct {
			ID      string  `json:"id"`
			Outputs []any   `json:"outputs"`
			Count   *uint64 `json:"execution_count"`
			Source  string  `json:"source"`
		}
	}
	if err = json.Unmarshal([]byte(after), &document); err != nil || len(document.Cells) != 1 || document.Cells[0].ID == "" || len(document.Cells[0].Outputs) != 0 || document.Cells[0].Count != nil || document.Cells[0].Source != "next\n" {
		t.Fatalf("edited notebook: %s (%v)", after, err)
	}
	if _, err = parseNotebook(strings.Replace(draft, `["old\n"]`, `null`, 1)); err == nil {
		t.Fatal("null notebook source was accepted")
	}
}
