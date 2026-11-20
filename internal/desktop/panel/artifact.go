package panel

import (
	"strconv"

	"github.com/egoist/mygo/ui"
	"github.com/egoist/mygo/ui/markdown"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/fileview"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
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
	Markdown     func(conversation.File) *markdown.Options
	// Versions are the version a document's preview shows and the latest
	// its conversation sees, 0 for a file; ShowVersion shows another.
	Versions    func(conversation.File) (shown, latest uint32)
	ShowVersion func(conversation.File, uint32)
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
	if s.artifactCallbacks.Markdown != nil {
		cb.Markdown = s.artifactCallbacks.Markdown(f)
	}
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
				Radius(8).AlignItems(ui.Center).Children(func() {
				s.artifactVersions(c, f)
				fileview.Tools(c, f, cb)
			})
		}
	})
}

// artifactVersions is a revised document's version menu: the version shown,
// opening a list of every version, newest first, the latest marked.
func (s *State) artifactVersions(c *ui.Context, f conversation.File) {
	callbacks := s.artifactCallbacks
	if callbacks.Versions == nil || callbacks.ShowVersion == nil {
		return
	}
	shown, latest := callbacks.Versions(f)
	if latest < 2 || shown == 0 {
		return
	}
	version := func(n uint32) string {
		return i18n.T(c, "artifact_version", "revision", strconv.FormatUint(uint64(n), 10))
	}
	kit.MenuButton(c.Key("versions"), "artifact-versions/"+f.ID, func(b ui.Element) {
		label := version(shown)
		kit.GhostLook(c, b.Label(label), false).Row().Height(28).PaddingX(8).Gap(4).Children(func() {
			ui.Text(c, label).FontSize(13).SingleLine()
			ui.Icon(c, icons.ChevronDown).FontSize(14).Shrink(0)
		})
	}, func(m *kit.Menu) {
		for n := latest; n >= 1; n-- {
			item := m.Item(version(n)).Checked(n == shown)
			if n == latest {
				item.Trailing(i18n.T(c, "artifact_version_latest"))
			}
			if item.Chosen() && n != shown {
				callbacks.ShowVersion(f, n)
			}
		}
	}, kit.MenuOptions{At: ui.AnchorBottomEnd, Self: ui.AnchorTopEnd})
}

// ForgetArtifact removes a closed or revoked file projection and its content.
func (s *State) ForgetArtifact(id string) {
	s.Tabs, s.Active = Closed(s.Tabs, Tab{Artifact, id}, s.Active)
	delete(s.artifacts, id)
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
