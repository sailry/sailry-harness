package composer

import (
	"fmt"
	"path/filepath"
	"slices"
	"strings"

	"github.com/egoist/mygo/transfer"
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// The draft's files: chosen in the file picker, dropped on the surface or
// pasted, each a card in a row over the draft, a picture's showing its
// thumbnail, with its transfer's progress, Retry once it failed and Remove. A
// message carries up to MaxAttachments.

// seq numbers the attachments the composer makes.
var seq int

// add makes the draft's attachments of files and hands each to the backend.
// A batch that would pass the limit is refused whole, with the limit's toast
// (attach_drafts).
func (s *State) add(c *ui.Context, list []*Attachment, actions Actions) {
	if len(list) > MaxAttachments-len(s.Attachments) {
		kit.Toast(c, kit.Failed, i18n.T(c, "chat_attachment_limit"))
		return
	}
	for _, a := range list {
		seq++
		a.ID = fmt.Sprintf("attachment-%d", seq)
		s.Attachments = append(s.Attachments, a)
		actions.Upload(a)
	}
}

// attachPaths attaches files and folders dropped on the surface or pasted.
// Their names and kinds come from the paths; the backend reads them.
func (s *State) attachPaths(c *ui.Context, paths []string, actions Actions) {
	var list []*Attachment
	for _, path := range paths {
		list = append(list, &Attachment{Name: filepath.Base(path), Path: path, Mime: mimeOf(path)})
	}
	s.add(c, list, actions)
}

// pastedImages are the picture formats a paste attaches besides the PNG the
// system's own picture format is read as, with their file extensions.
var pastedImages = []struct {
	format    transfer.Format
	extension string
}{
	{transfer.PNG, "png"}, {"image/jpeg", "jpg"}, {"image/gif", "gif"}, {"image/webp", "webp"},
}

// paste attaches what was copied into the draft (paste_attachments): the
// copied files and folders by their paths, and a copied picture, named
// Image N by its place among the draft's files, its thumbnail decoded from
// it. Copied files come with their names as text, so they attach whatever
// text the clipboard has; a picture attaches only when the clipboard has no
// text, which is the draft's otherwise. It reports whether it took the paste.
func (s *State) paste(c *ui.Context, text string, actions Actions) bool {
	sv := c.Services()
	offered := sv.ClipboardFormats()
	var list []*Attachment
	if slices.Contains(offered, transfer.FileList) {
		if paths, err := sv.ReadClipboardData(transfer.FileList).Files(); err == nil {
			for _, path := range paths {
				list = append(list, &Attachment{Name: filepath.Base(path), Path: path, Mime: mimeOf(path)})
			}
		}
	}
	if len(list) == 0 && text == "" {
		for _, image := range pastedImages {
			if !slices.Contains(offered, image.format) {
				continue
			}
			data, err := sv.ReadClipboardData(image.format).Read(image.format)
			if err != nil || len(data) == 0 {
				continue
			}
			// Named in the frame, which knows the language.
			a := &Attachment{Name: "." + image.extension, Mime: string(image.format), Size: int64(len(data)), Data: data}
			if b, err := ui.DecodeBitmap(data); err == nil {
				a.Image = b
			}
			list = append(list, a)
			break
		}
	}
	if len(list) == 0 {
		return false
	}
	s.inFrame(c, func(c *ui.Context) {
		for _, a := range list {
			if a.Data != nil {
				a.Name = i18n.T(c, "chat_clipboard_image", "count", len(s.Attachments)+1) + a.Name
			}
		}
		s.add(c, list, actions)
	})
	return true
}

// mimeOf guesses a file's type from its extension, for its icon and kind.
func mimeOf(path string) string {
	switch strings.ToLower(filepath.Ext(path)) {
	case ".png":
		return "image/png"
	case ".jpg", ".jpeg":
		return "image/jpeg"
	case ".gif":
		return "image/gif"
	case ".webp":
		return "image/webp"
	case ".pdf":
		return "application/pdf"
	case ".md", ".markdown":
		return "text/markdown"
	case ".go", ".rs", ".ts", ".js", ".py", ".swift", ".c", ".h":
		return "text/x-source"
	case ".json":
		return "application/json"
	case ".txt", ".log":
		return "text/plain"
	}
	return "application/octet-stream"
}

// fileIcon is a file's icon by its type.
func fileIcon(mime string) *ui.SVG {
	switch {
	case strings.HasPrefix(mime, "image/"):
		return icons.FileImage
	case mime == "text/x-source" || mime == "application/json":
		return icons.FileCode
	case strings.HasPrefix(mime, "text/"):
		return icons.FileText
	}
	return icons.File
}

// size is a byte count as the attachments' metadata says it.
func size(c *ui.Context, n int64) string {
	switch {
	case n < 1024:
		return i18n.T(c, "chat_attachment_bytes", "value", n)
	case n < 1024*1024:
		return i18n.T(c, "chat_attachment_kibibytes", "value", fmt.Sprintf("%.1f", float64(n)/1024))
	}
	return i18n.T(c, "chat_attachment_mebibytes", "value", fmt.Sprintf("%.1f", float64(n)/(1024*1024)))
}

// status is the line under a card's name while it travels or failed, "" once
// it is ready.
func status(c *ui.Context, a *Attachment) string {
	switch a.Status {
	case Preparing:
		return i18n.T(c, "files_upload_preparing")
	case Sending:
		return i18n.T(c, "files_upload_sending")
	case Publishing:
		return i18n.T(c, "files_upload_publishing")
	case Failed:
		if a.Error != "" {
			return i18n.T(c, a.Error)
		}
		return i18n.T(c, "chat_attachment_unknown")
	case Uncertain:
		return i18n.T(c, "chat_attachment_unknown")
	case Removing:
		return i18n.T(c, "chat_attachment_removing")
	}
	return ""
}

// attachments is Kit's AttachmentGroup, a row of cards scrolling sideways,
// its ends fading into the surface.
func (s *State) attachments(c *ui.Context, actions Actions) {
	kit.ImageViewer(c, &s.view, func(a *Attachment) kit.ViewerItem {
		return kit.ViewerItem{Source: a.Image, Name: a.Name}
	}, func(action kit.ImageAction, a *Attachment) {
		if s.attachmentCallbacks != nil && s.attachmentCallbacks.Image != nil {
			s.attachmentCallbacks.Image(action, a)
			return
		}
		kit.Message(c, i18n.T(c, "preview_notice"))
	})
	if len(s.Attachments) == 0 {
		return
	}
	s.strip(c.Key("attachments"), s.Attachments, actions).FillWidth().Shrink(0).Padding(8, 2, 2, 2)
}

// strip is the row of list's cards. With actions nil the cards only show
// the files, as a queued message's editor does: no Retry or Remove.
func (s *State) strip(c *ui.Context, list []*Attachment, actions Actions) ui.Element {
	// A mask shows as much as its alpha: the stops between the faded ends
	// are opaque, as a theme's muted fill may be all but transparent.
	shown := theme.Of(c).Muted
	shown.A = 255
	return ui.ScrollHorizontal(c).Gap(8).Children(func() {
		for _, a := range list {
			s.card(c, a, list, actions)
		}
	}).MaskLinearGradient(ui.LinearGradient{Angle: 90, Stops: []ui.GradientStop{
		{Color: shown.Alpha(0), At: 0}, {Color: shown, At: 0.03}, {Color: shown, At: 0.97}, {Color: shown.Alpha(0), At: 1},
	}})
}

// card is Kit's Attachment: a picture as its rounded thumbnail; a file as
// its icon tile beside its name and kind and size. A card shows its
// transfer's state: a spinner or the progress over the media, a red
// border with Retry once failed; Remove shows on hover. A picture opens
// the viewer over list's pictures; with actions nil the card has neither
// Retry nor Remove.
func (s *State) card(c *ui.Context, a *Attachment, list []*Attachment, actions Actions) {
	p := theme.Of(c)
	failed := a.Status == Failed || a.Status == Uncertain
	busy := a.Status == Preparing || a.Status == Sending || a.Status == Publishing || a.Status == Removing
	tip := status(c, a)
	if tip == "" {
		tip = a.Name
	}
	box := ui.Row(c.Key(a.ID)).Shrink(0).Radius(p.Radius2xl()).Border(1, p.Border).Background(p.Background).
		AlignItems(ui.Center).Label(a.Name)
	kit.Tooltip(c, box, tip)
	box.Transition(ui.ElementTransition{
		Enter: &ui.Motion{Opacity: 0, Scale: 0.9}, Exit: &ui.Motion{Opacity: 0, Scale: 0.9}})
	if failed {
		box.Border(1, p.Danger)
	}
	if a.Image != nil {
		box.Padding(4)
	} else {
		box.Padding(6, 12, 6, 6).Gap(8)
	}
	media := func(w, h float32) {
		ui.Box(c.Key("media")).Size(w, h).Shrink(0).Radius(p.RadiusLg).Background(p.Muted).Clip().Center().Children(func() {
			if a.Image != nil {
				kit.Image(c, a.Image, kit.ImageProps{Fill: true, Variant: kit.ImageBorderless, Alt: a.Name, OnOpen: func() { s.showImage(a, list) }})
			} else {
				ui.Icon(c, fileIcon(a.Mime)).FontSize(20).TextColor(p.MutedForeground)
			}
			if busy {
				ui.Box(c).Absolute().Left(0).Top(0).Right(0).Bottom(0).Center().Background(p.Background.Alpha(0.6)).Children(func() {
					if a.Status == Sending {
						ui.Text(c, fmt.Sprintf("%d%%", int(a.Progress*100))).FontSize(12).FontWeight(500)
					} else {
						ui.Spinner(c).FontSize(16).Label(tip)
					}
				})
			}
		})
	}
	box.Children(func() {
		if a.Image != nil {
			media(56, 56)
		} else {
			media(40, 40)
			ui.Column(c.Key("text")).MaxWidth(180).MinWidth(0).Gap(2).Children(func() {
				ui.Text(c, a.Name).FontSize(14).SingleLine()
				line := status(c, a)
				if line == "" {
					kind := i18n.T(c, "chat_attachment_file")
					if a.Mime != "" && a.Mime != "application/octet-stream" {
						kind = a.Mime
					}
					line = i18n.T(c, "chat_attachment_metadata", "kind", kind, "size", size(c, a.Size))
				}
				t := ui.Text(c, line).FontSize(12).SingleLine().TextColor(p.MutedForeground)
				if failed {
					t.TextColor(p.Danger)
				}
			})
		}
		if a.Status == Sending {
			ui.Box(c.Key("progress")).Absolute().Left(8).Right(8).Bottom(2).Height(2).Radius(1).Background(p.Border).
				Children(func() {
					ui.Box(c).WidthPercent(a.Progress * 100).FillHeight().Radius(1).Background(p.Primary)
				})
		}
		if actions == nil {
			return
		}
		corner := ui.Row(c.Key("actions")).Absolute().Top(-6).Right(-6).Gap(2)
		corner.Children(func() {
			if failed {
				round(c, "retry", icons.RotateCw, i18n.T(c, "chat_retry"), func() { actions.Upload(a) })
			}
			if (box.Hovered() || failed) && a.Status != Removing && a.Status != Publishing {
				round(c, "remove", icons.Close, i18n.T(c, "chat_attachment_remove"), func() { s.remove(a, actions) })
			}
		})
	})
}

// showImage opens the image viewer on a, stepping through the pictures
// of list, the draft's or a queued message's.
func (s *State) showImage(a *Attachment, list []*Attachment) {
	var shown []*Attachment
	at := 0
	for _, x := range list {
		if x == a {
			at = len(shown)
		}
		if x.Image != nil {
			shown = append(shown, x)
		}
	}
	s.view.Show(shown, at)
}

// round is a card's 20 DIP round action over its corner.
func round(c *ui.Context, key string, icon *ui.SVG, label string, fn func()) {
	p := theme.Of(c)
	b := ui.ButtonBase(c.Key(key)).Label(label)
	kit.Tooltip(c, b, label)
	b.Size(20, 20).Radius(10).Center().Cursor(ui.CursorPointer).Background(p.Foreground).TextColor(p.Background)
	if b.Hovered() {
		b.Background(p.Foreground.Alpha(0.85))
	}
	b.Children(func() { ui.Icon(c, icon).FontSize(12) }).OnClick(fn)
}

// remove takes a file out of the draft and tells the backend.
func (s *State) remove(a *Attachment, actions Actions) {
	if s.attachmentCallbacks != nil && s.attachmentCallbacks.Remove != nil {
		s.attachmentCallbacks.Remove(a)
		return
	}
	for i, x := range s.Attachments {
		if x == a {
			s.Attachments = append(s.Attachments[:i:i], s.Attachments[i+1:]...)
			break
		}
	}
	actions.Discard(a)
}
