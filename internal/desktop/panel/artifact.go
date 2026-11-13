package panel

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/fileview"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
)

// Artifact is a tab previewing a file of the conversation, a card's, by the
// file's ID.
const Artifact Kind = "artifact"

type ArtifactCallbacks struct {
	OpenExternal func(conversation.File)
	Image        func(kit.ImageAction, conversation.Image)
	Render       func(*ui.Context, conversation.File) bool
}

func (s *State) SetArtifactCallbacks(callbacks ArtifactCallbacks) { s.artifactCallbacks = callbacks }
func (s *State) UpdateArtifact(file conversation.File) {
	if s.artifacts == nil {
		s.artifacts = map[string]conversation.File{}
	}
	s.artifacts[file.ID] = file
}

// ShowArtifact opens f's preview as a tab, or focuses it.
func (s *State) ShowArtifact(f conversation.File) {
	if s.artifacts == nil {
		s.artifacts = map[string]conversation.File{}
	}
	s.artifacts[f.ID] = f
	s.Show(Tab{Artifact, f.ID})
}

// artifact is a file's preview, the application's one (fileview): the path
// bar with Copy and Open, then the file or the state of a preview that
// cannot show.
func (s *State) artifact(c *ui.Context, id string) {
	f, ok := s.artifacts[id]
	if !ok {
		empty(c, icons.File, "artifact_preview_failed")
		return
	}
	cb := fileview.Callbacks{Render: s.artifactCallbacks.Render, Image: func(action kit.ImageAction, image conversation.Image) {
		if s.artifactCallbacks.Image != nil {
			s.artifactCallbacks.Image(action, image)
		} else {
			notice(c)
		}
	}}
	// The demo's files are on no disk for another app to open.
	if !s.Demo {
		cb.OpenExternal = func() {
			if s.artifactCallbacks.OpenExternal != nil {
				s.artifactCallbacks.OpenExternal(f)
			} else {
				notice(c)
			}
		}
	}
	toolbar(c, "artifact-bar", func() {
		fileview.Title(c, f)
		fileview.Tools(c, f, cb)
	})
	fileview.Content(c, &s.view, id, f, cb)
}

// Subagent is a tab holding a subagent's session.
const Subagent Kind = "subagent"

// ShowChild opens a subagent's session as a tab titled title, or focuses it.
func (s *State) ShowChild(id, title string) {
	if s.children == nil {
		s.children = map[string]string{}
	}
	s.children[id] = title
	s.Show(Tab{Subagent, id})
}
