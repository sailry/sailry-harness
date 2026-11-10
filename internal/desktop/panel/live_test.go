package panel

import (
	"testing"

	"github.com/egoist/mygo/ui/editor"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
)

func TestLiveFileDrafts(t *testing.T) {
	t.Run("prompt pending and replaced form identities", func(t *testing.T) {
		pr := NewLiveProject("worktree", &viewmodel.Workspace{}, &Actions{})
		var completed func(bool)
		calls := 0
		pr.Prompt("first", []string{"settings_name"}, []string{"draft"}, func(values []string, done func(bool)) {
			calls++
			completed = done
			if values[0] != "draft" {
				t.Fatal("form did not capture draft")
			}
		})
		pr.submitPrompt()
		pr.submitPrompt()
		if calls != 1 {
			t.Fatal("pending form admitted duplicate request")
		}
		pr.Prompt("second", nil, nil, nil)
		completed(true)
		if !pr.prompt.open || pr.prompt.title != "second" {
			t.Fatal("old request completion closed replacement form")
		}
	})
	data := &viewmodel.Workspace{Files: &viewmodel.FileNode{Children: []*viewmodel.FileNode{{Name: "a.md", Path: "a.md"}}}}
	pr := NewLiveProject("worktree", data, &Actions{})
	pr.SetFile("a.md", "original\r\n\r\n")
	if text, dirty := pr.FileDraft("a.md"); text != "original\r\n\r\n" || dirty {
		t.Fatalf("initial draft %q dirty=%v", text, dirty)
	}
	pr.FileChanged("a.md", "edited")
	pr.SetFile("a.md", "external\n")
	if text, dirty := pr.FileDraft("a.md"); text != "edited\r\n\r\n" || !dirty {
		t.Fatalf("external refresh replaced draft %q dirty=%v", text, dirty)
	}
	pr.FileChanged("a.md", "edited again\n")
	pr.SavedFile("a.md", "edited\r\n\r\n")
	if text, dirty := pr.FileDraft("a.md"); text != "edited again\n\r\n\r\n" || !dirty {
		t.Fatalf("in-flight save replaced new edits %q dirty=%v", text, dirty)
	}
	pr.SavedFile("a.md", "edited again\n\r\n\r\n")
	if _, dirty := pr.FileDraft("a.md"); dirty {
		t.Fatal("successful matching save remained dirty")
	}
}
func TestLiveDiffHunkSelection(t *testing.T) {
	t.Run("captured git documents retain immutable native contents", func(t *testing.T) {
		data := &viewmodel.Workspace{}
		pr := NewLiveProject("worktree", data, &Actions{})
		files := []GitDocumentFile{{FileChange: viewmodel.FileChange{Path: "source.go", Hunks: []viewmodel.Hunk{{Lines: []viewmodel.DiffLine{{Kind: viewmodel.Removed, Old: 1, Text: "before Ω"}, {Kind: viewmodel.Added, New: 1, Text: "after λ"}}}}}, Truncated: true}}
		key := pr.SetGitCommit("0123456789", "01234567", "Message", files, true)
		if key != ":commit/0123456789" || pr.Git.Change() != key || len(pr.Git.Open()) != 1 || len(data.Documents) != 0 {
			t.Fatal("captured commit did not open in Git")
		}
		files[0].Hunks[0].Lines[0].Text = "mutated"
		if pr.Git.documents[key].files[0].Hunks[0].Lines[0].Text != "before Ω" || !pr.Git.documents[key].truncated {
			t.Fatal("captured commit aliases mutable caller data")
		}
		stash := pr.SetGitDiff("stash", "Stash", GitDocumentFile{Binary: true})
		if !pr.Git.documents[stash].files[0].Binary {
			t.Fatal("binary stash flag lost")
		}
		literal := "```diff\n# literal <tag>\n```\n"
		output := pr.SetGitText("output", "Git output", literal)
		if !pr.Git.documents[output].textOnly || pr.Git.documents[output].text != literal || len(data.Documents) != 0 {
			t.Fatal("Git output converted literal text")
		}
		if pr.gitDocumentLabel(key, "fallback") != "01234567" || !pr.gitDocumentKnown(key) {
			t.Fatal("Git document label lost")
		}
		pr.Git.Close(key)
		if pr.gitDocumentKnown(key) {
			t.Fatal("closed captured document remained cached")
		}
	})
	change := viewmodel.FileChange{Hunks: []viewmodel.Hunk{{Lines: []viewmodel.DiffLine{{Kind: viewmodel.Added, New: 4, Text: "first"}}}, {Lines: []viewmodel.DiffLine{{Kind: viewmodel.Added, New: 10, Text: "second"}}}}}
	if got := diffHunkIndex(change, editor.DiffHunk{NewStart: 9, NewEnd: 10}); got != 1 {
		t.Fatalf("second native hunk matched %d", got)
	}
	if got := diffHunkIndex(change, editor.DiffHunk{NewStart: 0, NewEnd: 11}); got != -1 {
		t.Fatalf("ambiguous native hunk matched %d", got)
	}
	if got := diffHunkIndex(change, editor.DiffHunk{NewStart: 5, NewEnd: 6}); got != -1 {
		t.Fatalf("unrelated native hunk matched %d", got)
	}
	change.Hunks = []viewmodel.Hunk{{Header: "@@ -10,2 +10 @@", Lines: []viewmodel.DiffLine{{Kind: viewmodel.Removed, Old: 10, Text: "deleted"}, {Kind: viewmodel.Context, Old: 11, New: 10, Text: "same"}}}}
	if got := diffHunkIndex(change, editor.DiffHunk{OldStart: 20, OldEnd: 21, NewStart: 9, NewEnd: 9}); got != 0 {
		t.Fatalf("deletion after prior native Keep matched %d", got)
	}
}
