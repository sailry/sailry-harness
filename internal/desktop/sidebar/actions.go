package sidebar

import (
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
)

// Actions injects controller operations without giving views backend ownership.
// Completion callbacks run on the UI thread. A failed save keeps its draft.
type Actions struct {
	Rename        func(*viewmodel.Conversation, string, func(bool))
	Archive       func(*viewmodel.Conversation)
	Read          func(*viewmodel.Conversation, bool)
	Delete        func(*viewmodel.Conversation)
	Pin           func(*viewmodel.Conversation)
	MovePin       func(source, target string, after bool)
	NewDraft      func(*viewmodel.Project)
	RemoveProject func(*viewmodel.Project)
	SaveProject   func(Editor, string, func(bool))
	Browse        func(host string, selected func(string))
	Worktrees     func(*viewmodel.Project, string, string)
	ForkWorktree  func(*viewmodel.Conversation)
	Records       func(*viewmodel.Project)
	OpenWindow    func(session string)
	ForkHere      func(*viewmodel.Conversation)
	Export        func(*viewmodel.Conversation, conversation.ExportFormat)
	OpenWorkspace func(*viewmodel.Project, *viewmodel.Conversation)
	Overview      func(*viewmodel.Project)
	CLI           func(*viewmodel.Project)
}

func (s *State) removeLabel() string {
	if s.Actions != nil {
		return "project_remove"
	}
	return "project_remove_preview"
}

// OpenFolder starts a local project draft with a chosen directory.
func (s *State) OpenFolder(path string) { s.OpenEditor(nil); s.editor.Path = path }

func sessionPath(pr *viewmodel.Project, cv *viewmodel.Conversation) string {
	if cv.WorktreePath != "" {
		return cv.WorktreePath
	}
	if pr != nil {
		return pr.Path
	}
	return ""
}
