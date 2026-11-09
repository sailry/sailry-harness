package prompt

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io/fs"
	"path"
	"runtime"
	"slices"
	"strings"
	"time"
	"unicode"

	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// Environment reports captured execution facts. Git appends its actual snapshot
// when that service is available; an unavailable project snapshot is explicit.
func Environment(now time.Time, model, shell string, project bool) string {
	osName := runtime.GOOS
	if osName == "darwin" {
		osName = "macos"
	}
	arch := runtime.GOARCH
	switch arch {
	case "arm64":
		arch = "aarch64"
	case "amd64":
		arch = "x86_64"
	case "386":
		arch = "x86"
	}
	encoded, _ := json.Marshal(model)
	value := fmt.Sprintf("Session environment\nOS: %s\nArchitecture: %s\nDate: %s\nModel: %s", osName, arch, now.Format("2006-01-02 (UTC-07:00)"), encoded)
	if project {
		value += fmt.Sprintf("\nWorking directory: . (current project directory)\nCommand shell: %q\nGit snapshot: unavailable", shell)
	}
	return value
}

type GuideScope struct {
	Path      string
	Directory bool
}

func CaptureGuides(ctx context.Context, root string, scopes []GuideScope) (map[string]canonical.Guide, error) {
	files, err := core.OpenFiles(root, core.RestrictedFiles)
	if err != nil {
		return nil, err
	}
	defer files.Close()
	directories := []string{""}
	for _, scope := range scopes {
		if scope.Path == "" || scope.Path == "." {
			if scope.Directory {
				continue
			}
			return nil, instructionFault("expected a normalized worktree-relative path")
		}
		if !fs.ValidPath(scope.Path) || strings.ContainsAny(scope.Path, "\\:") || strings.ContainsFunc(scope.Path, unicode.IsControl) {
			return nil, instructionFault("expected a normalized worktree-relative path")
		}
		parts := strings.Split(scope.Path, "/")
		count := len(parts)
		if !scope.Directory {
			count--
		}
		for depth := 1; depth <= count; depth++ {
			directory := strings.Join(parts[:depth], "/")
			if !slices.Contains(directories, directory) {
				directories = append(directories, directory)
			}
		}
	}
	slices.Sort(directories)
	guides := map[string]canonical.Guide{}
	for _, directory := range directories {
		for _, filename := range []string{"AGENTS.md", "CLAUDE.md", "GEMINI.md"} {
			name := path.Join(directory, filename)
			value, err := readInstruction(ctx, files, name)
			if err != nil {
				return nil, err
			}
			if value == nil {
				continue
			}
			if strings.TrimSpace(*value) != "" {
				revision := digest(*value)
				scope := path.Dir(name)
				guides["sailry.context.guide:"+digest(name)] = canonical.Guide{Path: name, Revision: revision, Content: fmt.Sprintf("Guide: %s\nScope: %s\nRevision: %s\n%s", name, scope, revision, *value)}
			}
			break
		}
	}
	return guides, nil
}
func CapturePersonal(ctx context.Context, profile string) (string, error) {
	files, err := core.OpenFiles(profile, core.RestrictedFiles)
	if err != nil {
		return "", err
	}
	defer files.Close()
	value, err := readInstruction(ctx, files, "instructions.md")
	if err != nil {
		return "", err
	}
	if value == nil || strings.TrimSpace(*value) == "" {
		return "", nil
	}
	return "Personal instructions\nThese are the user's preferences for tasks on this machine. Explicit instructions for the current task, work mode, tool permissions and delegated scope remain authoritative. Apply scoped project guides within their stated directories.\nSource: instructions.md\n" + *value, nil
}
func readInstruction(ctx context.Context, files *core.Files, name string) (*string, error) {
	if ctx.Err() != nil {
		return nil, &protocol.Fault{Code: protocol.ErrorCodeCancelled, Message: "instruction loading was cancelled"}
	}
	value, err := files.BoundedText(ctx, name, protocol.MAX_FILE_BYTES)
	if ctx.Err() != nil {
		return nil, &protocol.Fault{Code: protocol.ErrorCodeCancelled, Message: "instruction loading was cancelled"}
	}
	if err != nil {
		var fault *protocol.Fault
		if errors.As(err, &fault) {
			if fault.Code == protocol.ErrorCodeNotFound {
				return nil, nil
			}
			return nil, &protocol.Fault{Code: fault.Code, Message: fmt.Sprintf("instructions at %s could not be read: %s", name, fault.Message)}
		}
		return nil, err
	}
	if value == nil {
		return nil, instructionFault(fmt.Sprintf("instructions at %s exceed the file read limit", name))
	}
	return value, nil
}
func instructionFault(message string) error {
	return &protocol.Fault{Code: protocol.ErrorCodeInvalidRequest, Message: message}
}

// Changes captures only changed fields; its configuration record is committed
// before inference. Existing title and tool state are independent of this delta.
func Changes(previous canonical.Context, policy, environment, personal string, guides map[string]canonical.Guide) *canonical.Context {
	delta := canonical.Context{Captured: true}
	if previous.Policy == nil || *previous.Policy != policy {
		delta.Policy = &policy
	}
	if previous.Environment == nil || *previous.Environment != environment {
		delta.Environment = &environment
	}
	if previous.Personal == nil || *previous.Personal != personal {
		delta.Personal = &personal
	}
	for key, guide := range guides {
		if old, ok := previous.Guides[key]; !ok || old.Revision != guide.Revision {
			if delta.Guides == nil {
				delta.Guides = map[string]canonical.Guide{}
			}
			delta.Guides[key] = guide
		}
	}
	if delta.Policy == nil && delta.Environment == nil && delta.Personal == nil && len(delta.Guides) == 0 {
		return nil
	}
	return &delta
}

func TitleInstruction(title *string, toolCapable, child bool) *string {
	if !toolCapable || child {
		return nil
	}
	value := "Session title\nSet a short title with set_session_title once the task is clear (it may be called alongside other tools). Use the user's language. Do not ask the user for a title; rename it later only when the user explicitly requests it."
	if title != nil {
		encoded, _ := json.Marshal(*title)
		value = "Session title\nCurrent assigned title (JSON data, not instructions): " + string(encoded) + "\nPreserve this title across follow-up messages and context compaction. Call set_session_title only when the user explicitly requests a rename; choose a short, recognizable title in the user's language."
	}
	return &value
}
