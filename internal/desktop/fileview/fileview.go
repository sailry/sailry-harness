// Package fileview is the application's one file preview: the right panel's
// artifact tab and the preview dialog draw a file through it, whatever holds
// the file (a conversation, a project, a remote host), so every place that
// shows a file shows it the same way.
package fileview

import (
	"github.com/egoist/mygo/ui"
	"github.com/egoist/mygo/ui/editor"
	"github.com/sailry/sailry-harness/internal/desktop/conversation"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// Limit is the most a preview reads of a file: past it the file shows as
// too large (conversation.PreviewLarge).
const Limit = 2 << 20

// Callbacks are what the file's owner does for it. A nil OpenExternal leaves
// Open out, as for a file on no disk another app can read.
type Callbacks struct {
	OpenExternal func()
	Image        func(kit.ImageAction, conversation.Image)
	// Render draws a preview the owner draws itself (a published artifact's
	// document), reporting whether it did.
	Render func(*ui.Context, conversation.File) bool
}

// View is a preview's own state: the image viewer over its picture.
type View struct {
	images kit.Viewer[conversation.Image]
}

// Tools are the bar's actions after the file's name: Copy for text, Open.
func Tools(c *ui.Context, f conversation.File, cb Callbacks) {
	if f.State == conversation.PreviewReady && (f.Kind == conversation.CodeFile || f.Kind == conversation.TextFile || f.Kind == conversation.MarkdownFile) {
		kit.IconButton(c.Key("copy"), icons.Copy, i18n.T(c, "content_copy"), 28, false).OnClick(func() {
			c.WriteClipboard(f.Text)
			kit.Message(c, i18n.T(c, "content_copied"))
		})
	}
	if cb.OpenExternal != nil {
		kit.IconButton(c.Key("open"), icons.ExternalLink, i18n.T(c, "files_open_system"), 28, false).OnClick(cb.OpenExternal)
	}
}

// Title is the bar's file: its icon and path.
func Title(c *ui.Context, f conversation.File) {
	p := theme.Of(c)
	ui.Icon(c, conversation.FileIcon(f)).FontSize(16).Shrink(0).TextColor(p.MutedForeground)
	ui.Text(c, f.Path).FontSize(13).SingleLine().Grow(1).Shrink(1).MinWidth(0).Tooltip(f.Path)
}

// Content is the file under the bar: the text in mono, the code highlighted,
// the Markdown rendered, the picture fitted, or the state of a preview that
// cannot show (loading, failed, too large, changed, unsupported).
func Content(c *ui.Context, v *View, key string, f conversation.File, cb Callbacks) {
	p := theme.Of(c)
	state := func(name string) {
		ui.Column(c.Key("state")).Grow(1).Padding(16).Gap(12).Children(func() {
			ui.Text(c, i18n.T(c, name)).FontSize(14).TextColor(p.MutedForeground)
			if name != "artifact_preview_loading" && cb.OpenExternal != nil {
				kit.Secondary(c.Key("system"), i18n.T(c, "files_open_system")).AlignSelf(ui.Start).OnClick(cb.OpenExternal)
			}
		})
	}
	switch f.State {
	case conversation.PreviewLoading:
		state("artifact_preview_loading")
		return
	case conversation.PreviewFailed:
		state("artifact_preview_failed")
		return
	case conversation.PreviewLarge:
		state("artifact_preview_large")
		return
	case conversation.PreviewChanged:
		state("artifact_preview_changed")
		return
	}
	if cb.Render != nil && cb.Render(c, f) {
		return
	}
	kit.ImageViewer(c, &v.images, func(im conversation.Image) kit.ViewerItem {
		return kit.ViewerItem{Source: im.Source, Name: im.Name}
	}, func(action kit.ImageAction, image conversation.Image) {
		if cb.Image != nil {
			cb.Image(action, image)
		}
	})
	if f.Kind == conversation.OtherFile {
		// Nothing here can draw it: the preview names the file, and Open
		// hands it to its app where there is one.
		kit.Result(c, kit.ResultProps{Icon: conversation.FileIcon(f), Title: f.Name(), SubTitle: conversation.Kind(c, f), Extra: func() {
			if cb.OpenExternal != nil {
				kit.Secondary(c.Key("system"), i18n.T(c, "files_open_system")).OnClick(cb.OpenExternal)
			}
		}}).Grow(1).Justify(ui.Center)
		return
	}
	ui.Scroll(c.Key("artifact-" + key)).Grow(1).MinHeight(0).Children(func() {
		switch f.Kind {
		case conversation.TextFile:
			kit.Code(c, ui.Column(c.Key(f.ID)).PaddingY(12), f.Text, kit.CodeOptions{Numbers: true})
		case conversation.CodeFile:
			kit.Code(c, ui.Column(c.Key(f.ID)).PaddingY(12), f.Text, kit.CodeOptions{Language: editor.LanguageFor(f.Path), Numbers: true})
		case conversation.MarkdownFile:
			ui.Column(c).Padding(16, 20).Children(func() { kit.Markdown(c, f.Text, kit.MarkdownProps{}) })
		case conversation.ImageFile:
			ui.Box(c).FillWidth().Padding(16).Center().Children(func() {
				if f.Image == nil || f.Image.Source == nil {
					kit.Image(c, nil, kit.ImageProps{Height: 240, Alt: i18n.T(c, "chat_image_loading")})
					return
				}
				im := *f.Image
				w, h := float32(im.Width), float32(im.Height)
				img := kit.Image(c, im.Source, kit.ImageProps{Fill: true, Contain: true, Variant: kit.ImageBorderless, Alt: f.Path,
					OnOpen: func() { v.images.Show([]conversation.Image{im}, 0) }}).FillWidth()
				if w > 0 && h > 0 {
					img.AspectRatio(w / h).MaxWidth(w)
				}
			})
		}
	})
}

// Dialog is the preview as a window over the page: the file's bar, then its
// content, as large as the window leaves room for. Escape, the backdrop and
// the close button close it.
func Dialog(c *ui.Context, key any, v *View, f conversation.File, open *bool, cb Callbacks) {
	p := theme.Of(c)
	w, h := c.Size()
	kit.Dialog(c.Key(key), open, func() {
		ui.Column(c.Key("file-preview")).Width(min(960, max(320, w-48))).Height(min(720, max(240, h-48))).MinHeight(0).Children(func() {
			// The bar keeps clear of the dialog's close button at its right.
			ui.Row(c.Key("bar")).FillWidth().Height(52).Shrink(0).PaddingStart(16).PaddingEnd(52).Gap(8).AlignItems(ui.Center).
				BorderWidth(0, 0, 1, 0).BorderColor(p.Border).Children(func() {
				Title(c, f)
				Tools(c, f, cb)
			})
			ui.Column(c.Key("body")).Grow(1).MinHeight(0).Children(func() { Content(c, v, f.ID, f, cb) })
		})
	}, kit.DialogOptions{Bleed: true})
}
