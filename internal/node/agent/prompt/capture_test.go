package prompt

import (
	"context"
	"errors"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func instructionDirectory(t *testing.T) string {
	t.Helper()
	root, err := filepath.EvalSymlinks(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	return root
}

func writeInstruction(t *testing.T, root, name string, content []byte) {
	t.Helper()
	file := filepath.Join(root, filepath.FromSlash(name))
	if err := os.MkdirAll(filepath.Dir(file), 0o700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(file, content, 0o600); err != nil {
		t.Fatal(err)
	}
}

func TestCaptureInstructionsBeyondToolPage(t *testing.T) {
	root := instructionDirectory(t)
	content := strings.Repeat("Keep the declared task scope.\n", 5000) + "Full instruction tail: 规则。\n"
	if len(content) <= protocol.MAX_FILE_BYTES {
		t.Fatal("fixture does not exceed the tool page limit")
	}
	writeInstruction(t, root, "AGENTS.md", []byte(content))
	writeInstruction(t, root, "instructions.md", []byte(content))
	guides, err := CaptureGuides(t.Context(), root, nil)
	if err != nil {
		t.Fatal(err)
	}
	guide := guides["sailry.context.guide:"+digest("AGENTS.md")]
	if len(guides) != 1 || guide.Path != "AGENTS.md" || guide.Revision != digest(content) || !strings.HasSuffix(guide.Content, content) {
		t.Fatal("large project instructions were omitted, shortened or assigned a partial revision")
	}
	personal, err := CapturePersonal(t.Context(), root)
	if err != nil || !strings.HasSuffix(personal, content) {
		t.Fatalf("large personal instructions were not captured whole: %v", err)
	}
}

func TestCaptureGuidePrecedenceAndScopes(t *testing.T) {
	root := instructionDirectory(t)
	for name, content := range map[string]string{
		"AGENTS.md":          "Root guide",
		"CLAUDE.md":          "Unused root fallback",
		"src/CLAUDE.md":      "Source guide",
		"src/GEMINI.md":      "Unused source fallback",
		"src/deep/AGENTS.md": "Deep guide",
		"other/AGENTS.md":    "Unrelated guide",
	} {
		writeInstruction(t, root, name, []byte(content))
	}
	guides, err := CaptureGuides(t.Context(), root, []GuideScope{{Path: "src/deep/file.go"}})
	if err != nil {
		t.Fatal(err)
	}
	if len(guides) != 3 {
		t.Fatalf("captured %d guides, want three applicable scopes", len(guides))
	}
	for _, name := range []string{"AGENTS.md", "src/CLAUDE.md", "src/deep/AGENTS.md"} {
		if guide := guides["sailry.context.guide:"+digest(name)]; guide.Path != name {
			t.Fatalf("missing applicable guide %s", name)
		}
	}
	writeInstruction(t, root, "AGENTS.md", []byte(" \n\t"))
	guides, err = CaptureGuides(t.Context(), root, nil)
	if err != nil || len(guides) != 0 {
		t.Fatalf("an empty higher-priority guide unexpectedly selected a fallback: %v", err)
	}
}

func TestCaptureInstructionReadBoundaries(t *testing.T) {
	for _, row := range []struct {
		name, content, want string
	}{
		{"invalid UTF-8", "bad\xfftext", "not UTF-8 text"},
		{"binary", "bad\x00text", "binary file"},
	} {
		t.Run(row.name, func(t *testing.T) {
			root := instructionDirectory(t)
			writeInstruction(t, root, "AGENTS.md", []byte(row.content))
			writeInstruction(t, root, "CLAUDE.md", []byte("Must not replace unreadable instructions"))
			_, err := CaptureGuides(t.Context(), root, nil)
			var fault *protocol.Fault
			if !errors.As(err, &fault) || fault.Code != protocol.ErrorCodeInvalidRequest || !strings.Contains(fault.Message, row.want) || !strings.Contains(fault.Message, "AGENTS.md") {
				t.Fatalf("instruction read error = %v", err)
			}
		})
	}
	t.Run("history capacity", func(t *testing.T) {
		root := instructionDirectory(t)
		writeInstruction(t, root, "AGENTS.md", nil)
		if err := os.Truncate(filepath.Join(root, "AGENTS.md"), protocol.MAX_FRAME_BYTES+1); err != nil {
			t.Fatal(err)
		}
		writeInstruction(t, root, "CLAUDE.md", []byte("Must not replace oversized instructions"))
		_, err := CaptureGuides(t.Context(), root, nil)
		var fault *protocol.Fault
		if !errors.As(err, &fault) || fault.Code != protocol.ErrorCodeInvalidRequest || !strings.Contains(fault.Message, "AGENTS.md") || !strings.Contains(fault.Message, "conversation history record capacity") {
			t.Fatalf("oversized instruction error = %v", err)
		}
	})
	t.Run("cancelled", func(t *testing.T) {
		root := instructionDirectory(t)
		ctx, cancel := context.WithCancel(t.Context())
		cancel()
		_, err := CaptureGuides(ctx, root, nil)
		var fault *protocol.Fault
		if !errors.As(err, &fault) || fault.Code != protocol.ErrorCodeCancelled {
			t.Fatalf("cancelled instruction read = %v", err)
		}
	})
	t.Run("confined", func(t *testing.T) {
		root, outside := instructionDirectory(t), instructionDirectory(t)
		writeInstruction(t, outside, "guide.md", []byte("Outside the worktree"))
		if err := os.Symlink(filepath.Join(outside, "guide.md"), filepath.Join(root, "AGENTS.md")); err != nil {
			t.Skipf("symbolic link fixture unavailable: %v", err)
		}
		if _, err := CaptureGuides(t.Context(), root, nil); err == nil {
			t.Fatal("instructions escaped the captured worktree")
		}
	})
	t.Run("invalid scope", func(t *testing.T) {
		if _, err := CaptureGuides(t.Context(), instructionDirectory(t), []GuideScope{{Path: "../outside"}}); err == nil {
			t.Fatal("parent scope was admitted")
		}
	})
	t.Run("absent", func(t *testing.T) {
		root := instructionDirectory(t)
		guides, err := CaptureGuides(t.Context(), root, nil)
		if err != nil || len(guides) != 0 {
			t.Fatalf("absent guides = %v, %v", guides, err)
		}
		personal, err := CapturePersonal(t.Context(), root)
		if err != nil || personal != "" {
			t.Fatalf("absent personal instructions = %q, %v", personal, err)
		}
	})
}
