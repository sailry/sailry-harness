package composer

import (
	"fmt"
	"slices"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
)

// AttachmentCallbacks connect window-owned effects to the accepted draft
// cards. A nil Remove or Image keeps the preview's behavior.
type AttachmentCallbacks struct {
	// Pick opens the window's file picker in its multi-file mode for this
	// composer; the owner attaches what is chosen (AttachPaths). The composer
	// has no picker of its own: without Pick the attach button is disabled.
	Pick   func()
	Remove func(*Attachment)
	Image  func(kit.ImageAction, *Attachment)
}

func (s *State) SetAttachmentCallbacks(callbacks *AttachmentCallbacks) {
	s.attachmentCallbacks = callbacks
}

// openFiles asks the window for its file picker, from the attach button and
// the @ picker's Attach files entry. Enter in the @ picker chooses between
// frames: the picker opens in the next one, where it can name its host.
func (s *State) openFiles(c *ui.Context, ctx *Context) {
	if s.canPickFiles(ctx) {
		s.inFrame(c, func(*ui.Context) { s.attachmentCallbacks.Pick() })
	}
}

// canPickFiles reports whether files can be chosen: not in a read-only
// conversation, and only where the window offers its picker.
func (s *State) canPickFiles(ctx *Context) bool {
	return !ctx.Readonly && s.attachmentCallbacks != nil && s.attachmentCallbacks.Pick != nil
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

// AttachElement adds the attachments of an element picked in the browser,
// its picture and its page context, behind one chip named label at the end
// of the draft: all of them or, over the limit, none. They have no cards
// (Attachment.Chip) and leave when the chip is deleted (dropChipped).
func (s *State) AttachElement(c *ui.Context, id, label string, list []*Attachment, actions Actions) bool {
	if len(s.Attachments)+len(list) > MaxAttachments {
		kit.Toast(c, kit.Failed, i18n.T(c, "chat_attachment_limit"))
		return false
	}
	// A chip is found by its text: a second element of the same name is
	// numbered.
	name := label
	for n := 2; slices.ContainsFunc(s.References, func(r Reference) bool { return r.Label == name }); n++ {
		name = fmt.Sprintf("%s %d", label, n)
	}
	ref := Reference{ID: id, Kind: Element, Label: name}
	for _, a := range list {
		a.Chip = id
	}
	s.add(c, list, actions)
	s.References = append(s.References, ref)
	if s.Draft != "" && !strings.HasSuffix(s.Draft, " ") && !strings.HasSuffix(s.Draft, "\n") {
		s.Draft += " "
	}
	s.Draft += token(ref) + " "
	s.caret = len([]rune(s.Draft))
	s.focus = true
	s.ref = refState{}
	return true
}

// dropChipped removes the attachments whose chip left the draft.
func (s *State) dropChipped(actions Actions) {
	for _, a := range slices.Clone(s.Attachments) {
		if a.Chip != "" && !slices.ContainsFunc(s.References, func(r Reference) bool { return r.ID == a.Chip }) {
			s.remove(a, actions)
		}
	}
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
