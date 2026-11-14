package kit

import (
	"strings"
	"sync"

	"github.com/egoist/mygo/ui"
	"github.com/egoist/mygo/ui/editor"
	"github.com/egoist/mygo/ui/markdown"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// Code and diffs are MyGo's editor: editor.View for code and text
// (read-only but in file tabs), editor.DiffView for changes. Their look comes only from the
// editor's Theme, editor.DefaultTheme with the app theme's slots.

// codeFont is Kit's mono text: 13 DIPs.
var codeFont = ui.Font{Family: "monospace", Size: 13}

// codeLine is Kit's code line height, a multiple of the size: 1.6
// (content diff view).
const codeLine = 1.6

type codeThemeKey struct {
	p    *theme.Palette
	look codeLook
}

var codeThemes sync.Map // codeThemeKey -> *editor.Theme

// codeLook is which view a code theme is for.
type codeLook int

const (
	lookPlain    codeLook = iota // read-only code on the surface
	lookTint                     // read-only code in messages
	lookEditable                 // editable code: the caret shows
	lookDiff                     // diffs: an opaque surface to tint over
)

// CodeTheme is the editor's theme for the app theme: DefaultTheme with
// the text, gutter, selection and change colors from the palette, on no
// background. tint gives the background of code blocks in messages, the
// foreground at 3.5% over the background.
func CodeTheme(c *ui.Context, tint bool) *editor.Theme {
	if tint {
		return codeTheme(c, lookTint)
	}
	return codeTheme(c, lookPlain)
}

func codeTheme(c *ui.Context, look codeLook) *editor.Theme {
	p := theme.Of(c)
	key := codeThemeKey{p, look}
	if t, ok := codeThemes.Load(key); ok {
		return t.(*editor.Theme)
	}
	t := editor.DefaultTheme(c.Theme())
	// Transparent: file tabs and sources show the surface they sit on,
	// with no block ending at the last line, and no current-line band
	// (as the reference's views).
	t.Background = p.Background.Alpha(0)
	t.ActiveLine = p.Background.Alpha(0)
	switch look {
	case lookTint:
		t.Background = p.Background.Mix(p.Foreground, 0.035)
	case lookDiff:
		// MyGo mixes a diff's change tints into its background, and into
		// white when that is transparent: the opaque surface the diff
		// sits on keeps them dark-tinted in dark mode and still reads as
		// no background.
		t.Background = p.Background.Alpha(1)
	}
	t.Text = p.Foreground
	t.LineNumber = p.MutedForeground.Alpha(0.6)
	t.ActiveLineNumber = p.MutedForeground
	t.Selection = p.Selection
	t.InactiveSelection = p.Selection.Alpha(0.55)
	// Read-only views hide the caret, as MyGo's editor has no option to
	// draw none (it keeps the keyboard's selection and Copy): the color
	// is the only switch. Editable views show it.
	t.Cursor = p.Caret.Alpha(0)
	if look == lookEditable {
		t.Cursor = p.Caret
	}
	t.Added, t.Deleted = p.Success, p.Danger
	codeThemes.Store(key, t)
	return t
}

// codeView keeps an editor of a block of code and the text it shows.
type codeView struct {
	st   *editor.State
	text string
	// version is the buffer's version Changed last heard of.
	version uint64
}

// CodeOptions are how Code shows its text.
type CodeOptions struct {
	// Changed receives edited display text on the UI thread.
	Changed func(string)
	// Bind exposes the existing native editor state to its owning controls.
	Bind func(*editor.State)
	// Language highlights the text; nil leaves it plain.
	Language *editor.Language
	// Numbers shows line numbers, as file tabs do.
	Numbers bool
	// Tint is the background of code in messages.
	Tint bool
	// Editable lets the text be edited, with the caret showing, as file
	// tabs; the edits stay in the view.
	Editable bool
	// Wrap breaks long lines at the view's width instead of past it, from
	// the view's first frame: a file tab's own wrap toggle keeps its choice.
	Wrap bool
	// Fill makes the view as high as box and scroll its own lines, so the
	// editor's find bar stays at its top: a file's preview.
	Fill bool
}

// Code is an editor of text, read-only unless Editable, as high as its
// lines, inside box:
// it highlights by Language and keeps its editor while box stays.
// Trailing line breaks are left out, so no empty last line shows.
func Code(c *ui.Context, box ui.Element, text string, o CodeOptions) ui.Element {
	if o.Changed == nil {
		text = strings.TrimRight(text, "\r\n")
	}
	v := ui.Local(box, codeView{}, func() codeView {
		st := &editor.State{Buffer: editor.NewBuffer(text), ReadOnly: !o.Editable, FitContent: !o.Fill, NoScrollbars: !o.Fill, SoftWrap: o.Wrap,
			NoActiveLine: true, NoFolding: true, NoIndentGuides: true, Font: codeFont, LineHeight: codeLine}
		st.Buffer.SetLanguage(o.Language)
		if !o.Numbers {
			st.LineNumbers = editor.LineNumbersOff
		}
		return codeView{st: st, text: text, version: st.Buffer.Version()}
	})
	if v.text != text {
		v.text = text
		if v.st.Buffer.String() != text {
			v.st.Buffer.Set(text)
		}
		v.version = v.st.Buffer.Version()
	}
	look := lookPlain
	switch {
	case o.Editable:
		look = lookEditable
	case o.Tint:
		look = lookTint
	}
	v.st.Theme = codeTheme(c, look)
	if o.Bind != nil {
		o.Bind(v.st)
	}
	var native ui.Element
	box.Children(func() {
		native = editor.View(c, v.st).FillWidth()
		if o.Fill {
			native.Grow(1).MinHeight(0)
		}
	})
	// An edit reaches the buffer with its input event, between frames as
	// well as inside the view: Changed hears of every version once.
	if now := v.st.Buffer.Version(); now != v.version {
		v.version = now
		if edited := v.st.Buffer.String(); o.Changed != nil && edited != v.text {
			v.text = edited
			o.Changed(edited)
		}
	}
	return native
}

// CodeBlock is the Markdown hook (Options.CodeBlock) for code in
// messages and documents: the language and Copy over a read-only editor
// highlighting by the fence's language. Copy is an icon button, named by
// its tooltip, in place of MyGo's CopyButton, which has the icon and a
// text: it copies the block's code and confirms with the app's toast, as
// its other copy buttons, and waits for a block still being written to
// close, as MyGo's.
func CodeBlock(c *ui.Context, b *markdown.CodeBlockView) {
	p := theme.Of(c)
	ui.Column(c).AlignSelf(ui.Stretch).Background(p.Background.Mix(p.Foreground, 0.035)).Border(1, p.Border).
		Radius(p.Radius).Clip().Children(func() {
		ui.Row(c).AlignItems(ui.Center).Padding(4, 6, 0, 12).Children(func() {
			Level(c, ui.Text(c, b.Language).FontSize(12).TextColor(p.MutedForeground).Unselectable(), 12)
			ui.Spacer(c)
			IconButton(c.Key("copy"), icons.Copy, i18n.T(c, "content_copy"), 24, false).TextColor(p.MutedForeground).
				Disabled(b.Open).OnClick(func() {
				c.WriteClipboard(b.Code())
				Copied(c)
			})
		})
		body := ui.Column(c).Padding(4, 0, 8, 4)
		Code(c, body, b.Code(), CodeOptions{Language: editor.LanguageNamed(b.Language), Tint: true})
	})
}

// diffView keeps a diff viewer and the texts it compares.
type diffView struct {
	d        *editor.DiffState
	old, new string
}

// DiffOptions are how Diff shows a change.
type DiffOptions struct {
	// Language highlights both sides.
	Language *editor.Language
	// Fit makes the viewer as high as its lines, inside a list or
	// scroll, rather than scrolling them itself.
	Fit bool
	// NoNumbers leaves the gutter without numbers, as transcript
	// previews.
	NoNumbers bool
	// Split shows the old text on the left and the new on the right, their
	// lines aligned, in place of the unified text; the panel's diffs pass
	// the window's choice (DiffSplit) where they have the width for it.
	Split bool
	// Actions builds the actions of the change under the pointer, around
	// editor.DefaultHunkActions; nil shows none. Changed is called after
	// Revert or Keep changed a text.
	Actions func(c *ui.Context, bar ui.Element, a *editor.DiffHunkActions)
	Changed func()
	// HunkChanged observes the native controls; Revert waits for authoritative data.
	HunkChanged func(index int, keep bool)
}

// Diff is MyGo's diff viewer of old and new inside box, unified as
// content diff view or side by side (Split), read-only, keeping its state
// while box stays: the layout changes in place, with the scroll, the lines
// expanded and the hunk actions of the same state.
func Diff(c *ui.Context, box ui.Element, old, new string, o DiffOptions) {
	v := ui.Local(box, diffView{}, func() diffView {
		d := &editor.DiffState{Old: editor.NewBuffer(old), New: editor.NewBuffer(new), Layout: editor.DiffUnified,
			Indicators: editor.DiffClassic, FitContent: o.Fit, NoScrollbars: o.Fit, NoLineNumbers: o.NoNumbers,
			Font: codeFont, LineHeight: codeLine}
		d.Old.SetLanguage(o.Language)
		d.New.SetLanguage(o.Language)
		return diffView{d: d, old: old, new: new}
	})
	if v.old != old || v.new != new {
		v.old, v.new = old, new
		v.d.Old.Set(old)
		v.d.New.Set(new)
	}
	v.d.Theme = codeTheme(c, lookDiff)
	v.d.Layout = editor.DiffUnified
	if o.Split {
		v.d.Layout = editor.DiffSplit
	}
	v.d.HunkActions = o.Actions != nil
	v.d.RenderHunkActions = nil
	if o.Actions != nil {
		v.d.RenderHunkActions = func(c *ui.Context, bar ui.Element, a *editor.DiffHunkActions) {
			beforeOld, beforeNew := v.d.Old.Version(), v.d.New.Version()
			o.Actions(c, bar, a)
			changedOld, changedNew := v.d.Old.Version() != beforeOld, v.d.New.Version() != beforeNew
			if o.HunkChanged != nil && (changedOld || changedNew) {
				if changedNew {
					v.d.New.Set(new)
				}
				o.HunkChanged(a.Index, changedOld)
			}
			if o.Changed != nil && (changedOld || changedNew) {
				o.Changed()
			}
		}
	}
	box.Children(func() {
		e := editor.DiffView(c, v.d)
		if o.Fit {
			e.FillWidth()
		} else {
			e.Grow(1).MinHeight(0)
		}
	})
}

// diffSplit is a window's choice between a unified diff and a side-by-side
// one: its owner's (BindDiffSplit) or, in a window without one, its own.
type diffSplit struct {
	bound *bool
	own   bool
}

type diffSplitKey struct{}

// BindDiffSplit makes split the window's diff layout, which its owner
// keeps across launches; it runs before the window's diffs are built.
func BindDiffSplit(c *ui.Context, split *bool) {
	ui.Local(c.Root(), diffSplitKey{}, func() diffSplit { return diffSplit{} }).bound = split
}

// DiffSplit is the window's diff layout: true shows the old and the new
// text side by side. Every diff of the panel follows it, and the button at
// the end of a diff's footer writes it.
func DiffSplit(c *ui.Context) *bool {
	v := ui.Local(c.Root(), diffSplitKey{}, func() diffSplit { return diffSplit{} })
	if v.bound != nil {
		return v.bound
	}
	return &v.own
}
