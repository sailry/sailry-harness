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

// artifact is a file's preview, the application's one (fileview): the file
// or the state of a preview that cannot show, with Copy and Open floating
// at its top end. The tab names the file, and its tooltip is the path, so
// the preview has no title bar of its own.
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
	ui.Column(c.Key("artifact")).FillWidth().Grow(1).MinHeight(0).Children(func() {
		fileview.Content(c, &s.view, id, f, cb)
		if f.State == conversation.PreviewReady {
			kit.Floating(c, ui.Row(c.Key("artifact-tools"))).Absolute().Top(8).Right(12).Padding(2).Gap(2).
				Radius(8).Children(func() { fileview.Tools(c, f, cb) })
		}
	})
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
