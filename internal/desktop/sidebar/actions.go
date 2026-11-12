package sidebar

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/picker"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
)

// Actions injects controller operations without giving views backend ownership.
// Completion callbacks run on the UI thread. A failed save keeps its draft.
type Actions struct {
	Rename    func(*viewmodel.Conversation, string, func(bool))
	Archive   func(*viewmodel.Conversation)
	Unarchive func(*viewmodel.Conversation)
	Read      func(*viewmodel.Conversation, bool)
	Delete    func(*viewmodel.Conversation)
	Pin       func(*viewmodel.Conversation)
	MovePin   func(source, target string, after bool)
	// MoveSession saves a project's manual session order with source
	// before or after target.
	MoveSession   func(source, target *viewmodel.Conversation, after bool)
	NewDraft      func(*viewmodel.Project)
	RemoveProject func(*viewmodel.Project)
	SaveProject   func(Editor, string, func(bool))
	// Files returns the file system of a host for the directory picker, false
	// while the host cannot be reached.
	Files         func(host string) (picker.Source, bool)
	Worktrees     func(*viewmodel.Project, string, string)
	ForkWorktree  func(*viewmodel.Conversation)
	Records       func(*viewmodel.Project)
	OpenWindow    func(session string)
	ForkHere      func(*viewmodel.Conversation)
	Export        func(*viewmodel.Conversation, conversation.ExportFormat)
	OpenWorkspace func(*viewmodel.Project, *viewmodel.Conversation)
	Overview      func(*viewmodel.Project)
	CLI           func(*viewmodel.Project)
	NewTerminal   func(*viewmodel.Project)
	// ProjectMenu adds the items packages contribute to a project's menu.
	ProjectMenu func(*ui.Context, *ui.Menu, *viewmodel.Project)
}

// removeLabel is the removal's confirming action: the preview names its
// record, the demo as a live window does.
func (s *State) removeLabel(c *ui.Context) string {
	if s.Actions != nil || kit.Demo(c) {
		return "project_remove"
	}
	return "project_remove_preview"
}

// appearanceIcon is the icon an appearance names, the folder for none.
func appearanceIcon(icon string) string {
	if icon == "" {
		return "folder"
	}
	return icon
}

func sessionPath(pr *viewmodel.Project, cv *viewmodel.Conversation) string {
	if cv.WorktreePath != "" {
		return cv.WorktreePath
	}
	if pr != nil {
		return pr.Path
	}
	return ""
}
