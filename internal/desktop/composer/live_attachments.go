package composer

import (
	"slices"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
)

// AttachmentCallbacks connect controller-owned native effects to the accepted
// draft cards. A nil callback keeps the preview's existing behavior.
type AttachmentCallbacks struct {
	Pick   func()
	Remove func(*Attachment)
	Image  func(kit.ImageAction, *Attachment)
}

func (s *State) SetAttachmentCallbacks(callbacks *AttachmentCallbacks) {
	s.attachmentCallbacks = callbacks
}

// SetReferenceChildren keeps an open page tied to its current shared listing.
func (s *State) SetReferenceChildren(children func(Reference) ([]Reference, bool)) {
	s.referenceChildren = children
}

func (s *State) SetReferencePickerOpen(open func()) {
	s.referencePickerOpen = open
}

func (s *State) AttachPaths(c *ui.Context, paths []string, actions Actions) {
	s.attachPaths(c, paths, actions)
}

// Attach adds attachments that belong together, such as an element picked in
// the browser with its page context: all of them or, over the limit, none.
func (s *State) Attach(c *ui.Context, list []*Attachment, actions Actions) bool {
	if len(s.Attachments)+len(list) > MaxAttachments {
		kit.Toast(c, kit.Failed, i18n.T(c, "chat_attachment_limit"))
		return false
	}
	s.add(c, list, actions)
	return true
}

// InsertFiles adds normal file-reference tokens; it never submits the draft.
func (s *State) InsertFiles(paths []string, prompt string) {
	if s.Draft != "" {
		s.Draft += "\n\n"
	}
	for _, path := range paths {
		ref := Reference{ID: path, Kind: File, Label: path, Icon: icons.File}
		if !slices.ContainsFunc(s.References, func(value Reference) bool { return value.Kind == File && value.ID == path }) {
			s.References = append(s.References, ref)
		}
		s.Draft += token(ref) + " "
	}
	if prompt != "" {
		s.Draft += "\n" + prompt
	}
	s.Draft = strings.TrimRight(s.Draft, " ")
	s.caret = len([]rune(s.Draft))
	s.focus = true
	s.ref = refState{}
}

// AddReferenceChildren gives each dynamic page first refusal over its own rows.
func (s *State) AddReferenceChildren(children func(Reference) ([]Reference, bool)) {
	previous := s.referenceChildren
	s.referenceChildren = func(page Reference) ([]Reference, bool) {
		if rows, ok := children(page); ok {
			return rows, true
		}
		if previous != nil {
			return previous(page)
		}
		return nil, false
	}
}
