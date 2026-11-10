package conversation

import (
	"fmt"
	"strings"
	"sync"
	"unicode/utf8"

	"github.com/egoist/mygo/ui"
	"github.com/firede/limu"
	"github.com/firede/limu/layout"
	"github.com/firede/limu/limumygo"
	"github.com/firede/limu/scene"
	"github.com/firede/limu/style"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// Content blocks of a message beyond text and tools (live/messages/*,
// content/files, content/images): file cards, thumbnails and the image
// viewer, visuals, background notices, grounding, model retries,
// compaction turns, the changes card and citations.

const (
	previewFiles = 3
	deltaWidth   = 88
	thumbMax     = 240
)

// Kind is a file's kind label (content/files kind): the office formats,
// PDF, notebooks, HTML and Markdown by extension, else its MIME type.
func Kind(c *ui.Context, f File) string {
	ext := strings.ToLower(f.Path[strings.LastIndexByte(f.Path, '.')+1:])
	switch ext {
	case "docx", "doc":
		return "Word"
	case "xlsx", "xls":
		return "Excel"
	case "pptx", "ppt":
		return "PowerPoint"
	case "pdf":
		return "PDF"
	case "ipynb":
		return "Notebook"
	case "html", "htm":
		return "HTML"
	case "md", "markdown", "mdown":
		return i18n.T(c, "content_document")
	}
	return f.Mime
}

// FileIcon is a file's icon by kind.
func FileIcon(f File) *ui.SVG {
	switch f.Kind {
	case CodeFile:
		return icons.FileCode
	case ImageFile:
		return icons.FileImage
	case MarkdownFile, TextFile:
		return icons.FileText
	}
	return icons.File
}

// fileCard is a 2xl-rounded group box, a 40 DIP icon tile, the name over its
// kind, Open and the More menu (Preview, Open in another app, Save as,
// Request edit). A document with a picture shows it on top.
func fileCard(c *ui.Context, key string, f File, actions Actions) {
	p := theme.Of(c)
	card := ui.Column(c.Key(key)).FillWidth().MinWidth(0).Radius(p.Radius2xl()).Border(1, p.Border).
		Background(p.Surface.Alpha(0.8)).Clip()
	card.Children(func() {
		if f.Image != nil && f.Image.Source != nil && f.Kind != ImageFile {
			w, h := float32(f.Image.Width), float32(f.Image.Height)
			t := kit.Image(c.Key("thumb"), f.Image.Source, kit.ImageProps{Fill: true, Variant: kit.ImageBorderless,
				Alt: i18n.T(c, "artifact_open"), OnOpen: func() { actions.OpenFile(f) }}).
				FillWidth().Radius(0).BorderWidth(0, 0, 1, 0).BorderColor(p.Border)
			if w > 0 && h > 0 {
				t.AspectRatio(w / h)
			}
		}
		ui.Row(c).FillWidth().MinWidth(0).Padding(8).Gap(8).AlignItems(ui.Center).Children(func() {
			ui.Row(c).Height(56).Grow(1).MinWidth(0).Gap(12).PaddingX(8).AlignItems(ui.Center).Children(func() {
				ui.Box(c).Size(40, 40).Shrink(0).Radius(p.Radius).Background(p.Muted).Center().Children(func() {
					ui.Icon(c, FileIcon(f)).FontSize(28).TextColor(p.MutedForeground)
				})
				ui.Column(c).Grow(1).MinWidth(0).Gap(4).Children(func() {
					ui.Text(c, f.Name()).FontSize(14).SingleLine()
					ui.Text(c, Kind(c, f)).FontSize(12).SingleLine().TextColor(p.MutedForeground)
				})
			})
			kit.IconButton(c.Key("open"), icons.ExternalLink, i18n.T(c, "artifact_open"), 28, false).OnClick(func() { actions.OpenFile(f) })
			kit.MenuButton(c.Key("more"), "file-"+key, func(b ui.Element) {
				kit.IconLook(c, b, icons.Ellipsis, i18n.T(c, "more"), "", 28, false)
			}, func(m *kit.Menu) {
				label := "artifact_open"
				if f.Kind != OtherFile {
					label = "artifact_preview"
				}
				if m.Item(i18n.T(c, label)).Chosen() {
					actions.OpenFile(f)
				}
				if m.Item(i18n.T(c, "files_open_system")).Chosen() {
					actions.FileAction(f, OpenExternal)
				}
				if m.Item(i18n.T(c, "files_download")).Chosen() {
					actions.FileAction(f, Download)
				}
				if m.Item(i18n.T(c, "artifact_request_edit")).Disabled(!f.Editable).Chosen() {
					actions.FileAction(f, RequestEdit)
				}
			}, kit.MenuOptions{At: ui.AnchorBottomEnd, Self: ui.AnchorTopEnd})
		})
	})
}

// files lays file cards out one under another, the gap Kit's lg spacing.
func files(c *ui.Context, key string, list []File, actions Actions) {
	ui.Column(c.Key(key)).FillWidth().Gap(12).Children(func() {
		for i, f := range list {
			fileCard(c, fmt.Sprintf("%s-%d", key, i), f, actions)
		}
	})
}

// thumbnails are a message's pictures: Kit's Images, rounded, up to 240
// DIPs, wrapping, a Skeleton while one loads; one opens the image viewer
// on the gallery of them all.
func (s *State) thumbnails(c *ui.Context, key string, list []Image, end bool) {
	row := ui.Row(c.Key(key)).Wrap().Gap(8)
	if end {
		row.Justify(ui.End)
	}
	kit.ImageGroup(c, row, &s.view, list, func(_ int, im Image) (ui.ImageSource, kit.ImageProps) {
		w, h := float32(im.Width), float32(im.Height)
		if w <= 0 || h <= 0 {
			w, h = thumbMax, thumbMax
		}
		k := min(1, thumbMax/max(w, h))
		return im.Source, kit.ImageProps{Width: w * k, Height: h * k, Alt: im.Name}
	})
}

// visual is a chart or diagram the model drew, from its Mermaid source:
// limu lays it out to the card's width in the app's fonts, and limumygo
// paints it in the theme's colors on the group box. A source limu cannot
// read shows as code.
func visual(c *ui.Context, key string, v *Visual) {
	p := theme.Of(c)
	if v == nil || v.Mermaid == "" {
		return
	}
	box := ui.Column(c.Key(key)).FillWidth().Gap(8).Padding(12).Radius(p.RadiusLg).Border(1, p.Border).Background(p.Surface.Alpha(0.8))
	w := box.LayoutBounds().W - 24
	box.Children(func() {
		if v.Title != "" {
			ui.Text(c, v.Title).FontSize(12).TextColor(p.MutedForeground)
		}
		sc, err := diagram(v.Mermaid, w)
		if err != nil {
			kit.Code(c, ui.Column(c.Key("source")), v.Mermaid, kit.CodeOptions{Tint: true})
			return
		}
		look := &limumygo.Options{Look: style.Look{Theme: diagramTheme(p), ThemeOnly: true}, Fonts: limumygo.DefaultFonts}
		ui.Row(c.Key("diagram")).FillWidth().Justify(ui.Center).Children(func() {
			limumygo.View(c, sc, look).Label(v.Title)
		})
	})
}

// diagramKey is a laid out diagram: its source, at a width.
type diagramKey struct {
	src   string
	width int
}

// laidOut is what limu made of a source at a width: its scene, or why it
// could not read it.
type laidOut struct {
	sc  *scene.Scene
	err error
}

// diagramLimit bounds the layouts kept: a window resized across many
// widths starts over rather than keeping a scene for each.
const diagramLimit = 256

// diagrams keeps the scenes laid out, as layout measures every label, and
// the sources limu could not read, so they are not parsed every frame.
var diagrams = struct {
	sync.Mutex
	m map[diagramKey]laidOut
}{m: map[diagramKey]laidOut{}}

// diagram is src laid out for a width (a frame's whole DIPs; none before
// the first layout), in the app's fonts and 14 DIP text.
func diagram(src string, width float32) (*scene.Scene, error) {
	k := diagramKey{src, max(int(width), 0)}
	diagrams.Lock()
	defer diagrams.Unlock()
	if d, ok := diagrams.m[k]; ok {
		return d.sc, d.err
	}
	o := &layout.Options{Measurer: limumygo.DefaultFonts, FontSize: 14, MaxWidth: float64(k.width), Snap: 0.5}
	sc, err := limu.Layout(src, o)
	if len(diagrams.m) >= diagramLimit {
		clear(diagrams.m)
	}
	diagrams.m[k] = laidOut{sc, err}
	return sc, err
}

// diagramTheme is limu's theme in the palette's colors: no fill behind
// the diagram, the text, lines and outlines of the transcript, and the
// series in the primary and the theme's accents.
func diagramTheme(p *theme.Palette) *style.Theme {
	col := func(c ui.Color) style.Color { return style.Color{R: c.R, G: c.G, B: c.B, A: c.A} }
	return &style.Theme{
		Bg: col(p.Surface.Alpha(0)), Fg: col(p.Foreground), Muted: col(p.MutedForeground), Line: col(p.MutedForeground),
		Surface: col(p.Muted), SurfaceAlt: col(p.Secondary), Border: col(p.Border), Accent: col(p.Primary), Critical: col(p.Danger),
		Series:      [8]style.Color{col(p.Primary), col(p.Orange), col(p.Success), col(p.Danger), col(p.Blue), col(p.Green), col(p.Yellow), col(p.Magenta)},
		Radius:      float64(p.Radius) / 2,
		StrokeWidth: 1,
	}
}

// notices are background results, on the user's side as a bubble: each task's
// icon, title and outcome in its tone.
func notices(c *ui.Context, list []Notice) {
	p := theme.Of(c)
	ui.Row(c.Key("notices")).Justify(ui.End).Children(func() {
		ui.Column(c).Gap(6).Padding(9, 14).Radius(p.Radius).Background(kit.Veil(c, p.Muted)).MaxWidth(bubbleMax).
			Label(i18n.T(c, "background_notice_title")).Children(func() {
			for i, n := range list {
				ui.Row(c.Key(i)).Gap(8).MinWidth(0).AlignItems(ui.Center).Children(func() {
					icon := icons.Terminal
					if n.Agent {
						icon = icons.Bot
					}
					ui.Icon(c, icon).FontSize(16).Shrink(0).TextColor(p.MutedForeground)
					ui.Text(c, n.Title).FontSize(14).SingleLine().Grow(1).Shrink(1).MinWidth(0)
					ui.Text(c, i18n.T(c, outcomeKey(n.Outcome))).FontSize(14).Shrink(0).TextColor(outcomeTone(p, n.Outcome))
				})
			}
		})
	})
}

func outcomeKey(o NoticeOutcome) string {
	return [...]string{"background_outcome_completed", "background_outcome_failed", "background_outcome_cancelled",
		"background_outcome_stopped", "background_outcome_timed_out", "background_outcome_interrupted",
		"background_outcome_unknown", "background_outcome_stalled"}[o]
}

func outcomeTone(p *theme.Palette, o NoticeOutcome) ui.Color {
	switch o {
	case OutcomeFailed, OutcomeTimedOut, OutcomeInterrupted, OutcomeUnknown:
		return p.Danger
	case OutcomeStalled:
		return p.Warning
	}
	return p.MutedForeground
}

// grounding is a search the model grounded its answer on: a Globe row opening
// the sources, each a link.
func (s *State) grounding(c *ui.Context, key string, b Block) {
	p := theme.Of(c)
	head := Call{Label: i18n.T(c, "chat_search_result"), Icon: "globe"}
	s.disclosure(c, "search-"+key, func(hovered, open bool, sw *kit.Sweep) { summary(c, head, hovered, true, open, sw) }, 4, 8, func() {
		for i, src := range b.Sources {
			title := src.Title
			if title == "" {
				title = src.URI
			}
			ui.Row(c.Key(i)).Gap(8).MinWidth(0).Children(func() {
				ui.Text(c, fmt.Sprint(i+1)).FontSize(12).Shrink(0).TextColor(p.MutedForeground)
				ui.Link(c, title, src.URI).FontSize(14).SingleLine().Shrink(1).MinWidth(0)
			})
		}
	})
}

// retry is a model request retried: a Network row, shimmering while it
// retries, opening the provider's message.
func (s *State) retry(c *ui.Context, key string, r *Retry, active bool) {
	p := theme.Of(c)
	label := ""
	switch {
	case r.Exhausted:
		label = i18n.T(c, "chat_model_retry_exhausted")
	case r.Limit > 0 && active:
		label = i18n.T(c, "chat_model_reconnecting", "attempt", r.Attempt, "limit", r.Limit)
	case r.Limit > 0:
		label = i18n.T(c, "chat_model_retried", "attempt", r.Attempt, "limit", r.Limit)
	case active:
		label = i18n.T(c, "chat_model_reconnecting_continuously")
	default:
		label = i18n.T(c, "chat_model_connection_retried")
	}
	head := Call{Label: label, Icon: "network", Running: active, Warning: r.Exhausted}
	if r.Reason != "" {
		head.Detail = i18n.T(c, r.Reason)
	}
	if r.Detail == "" {
		plain(c, "retry-"+key, head)
		return
	}
	s.disclosure(c, "retry-"+key, func(hovered, open bool, sw *kit.Sweep) { summary(c, head, hovered, true, open, sw) }, 4, 8, func() {
		ui.Scroll(c).MaxHeight(160).Children(func() {
			ui.Text(c, r.Detail).Selectable().FontSize(14).TextColor(p.MutedForeground)
		})
	})
}

// compaction is a compaction turn: a centered status between two lines,
// shimmering while it runs, with the failure's cause as its tooltip; a
// finished one opens its summary, a failed one its cause.
func (s *State) compaction(c *ui.Context, t *Turn) {
	p := theme.Of(c)
	k := t.Compaction
	key := map[CompactionState]string{
		Compacted: "chat_compacted", CompactionRunning: "chat_compacting", CompactionQueued: "chat_compact_queued",
		CompactionFailed: "chat_compact_failed", CompactionCancelled: "chat_compact_cancelled", CompactionUnneeded: "chat_compact_unneeded",
	}[k.State]
	label := i18n.T(c, key)
	switch {
	case k.State == CompactionFailed:
		head := Call{Label: label, Icon: "triangle-alert", Failed: true}
		cause := k.Cause
		if strings.TrimSpace(cause) == "" {
			cause = i18n.T(c, "compact_reason_unknown")
		}
		s.disclosure(c, "compaction-"+t.ID, func(hovered, open bool, sw *kit.Sweep) { summary(c, head, hovered, true, open, sw) }, 4, 8, func() {
			ui.Text(c, cause).Selectable().FontSize(14).TextColor(p.Danger)
		})
	case k.Summary != "":
		head := Call{Label: label, Icon: "file-text"}
		s.disclosure(c, "compaction-"+t.ID, func(hovered, open bool, sw *kit.Sweep) { summary(c, head, hovered, true, open, sw) }, 4, 8, func() {
			ui.Scroll(c).MaxHeight(240).Children(func() {
				ui.Text(c, k.Summary).Selectable().FontSize(14).LineHeight(22.0 / 14).TextColor(p.MutedForeground)
			})
		})
	default:
		ui.Row(c.Key("compaction")).FillWidth().Gap(8).AlignItems(ui.Center).TextColor(p.MutedForeground).Children(func() {
			ui.Box(c).Height(1).Grow(1).Background(p.Border)
			if k.State == CompactionRunning || k.State == CompactionQueued {
				kit.Shimmer(c, label, 14, p.MutedForeground)
			} else {
				ui.Text(c, label).FontSize(14)
			}
			ui.Box(c).Height(1).Grow(1).Background(p.Border)
		})
	}
}

// diffBadge is +N −M in mono 12, success and danger.
func diffBadge(c *ui.Context, added, removed int) {
	p := theme.Of(c)
	ui.Row(c).Gap(6).Shrink(0).Children(func() {
		if added > 0 || removed == 0 {
			ui.Text(c, fmt.Sprintf("+%d", added)).Font("monospace").FontSize(12).TextColor(p.Success)
		}
		if removed > 0 {
			ui.Text(c, fmt.Sprintf("−%d", removed)).Font("monospace").FontSize(12).TextColor(p.Danger)
		}
	})
}

// changes is the turn's changes card: a FileText tile, "Edited N files" over
// the totals, Undo and Review; then up to three files, each 40 high with its
// directory muted and its counts, Review blocks and Undo showing on hover in
// their place; Show N more toggles the rest.
func (s *State) changes(c *ui.Context, turn string, ch *Changes, actions Actions) {
	p := theme.Of(c)
	if ch == nil || len(ch.Files) == 0 {
		return
	}
	added, removed := 0, 0
	for _, f := range ch.Files {
		added, removed = added+f.Added, removed+f.Removed
	}
	more := s.opened("changes-more-" + turn)
	all := ch.Undone[""]
	ui.Column(c.Key("changes")).FillWidth().Radius(p.Radius2xl()).Border(1, p.Border).Background(p.Muted.Alpha(0.2)).Clip().Children(func() {
		ui.Row(c).FillWidth().MinWidth(0).Padding(12).Gap(12).AlignItems(ui.Center).Children(func() {
			ui.Box(c).Size(40, 40).Shrink(0).Radius(p.Radius2xl()).Background(p.MutedForeground.Alpha(0.12)).Center().Children(func() {
				ui.Icon(c, icons.FileText).FontSize(20).TextColor(p.MutedForeground)
			})
			ui.Column(c).Grow(1).MinWidth(0).Gap(4).Children(func() {
				ui.Text(c, i18n.T(c, "turn_changes_count", "count", len(ch.Files))).FontSize(14)
				diffBadge(c, added, removed)
			})
			undo := i18n.T(c, "turn_changes_undo")
			if all {
				undo = i18n.T(c, "turn_changes_undone")
			}
			kit.GhostLook(c, ui.ButtonBase(c.Key("undo")).Label(undo).Disabled(all), false).Height(28).PaddingX(8).Gap(6).Children(func() {
				ui.Icon(c, icons.Undo).FontSize(14)
				ui.Text(c, undo).FontSize(14)
			}).OnClick(func() { actions.Undo(turn, "") })
			kit.Secondary(c.Key("review"), i18n.T(c, "turn_changes_review")).Height(28).OnClick(func() { actions.Review(turn, "") })
		})
		n := len(ch.Files)
		shown := n
		if !*more {
			shown = min(n, previewFiles)
		}
		for i, f := range ch.Files[:shown] {
			row := ui.ButtonBase(c.Key(f.Path)).Label(f.Path).FillWidth().MinWidth(0).Height(40).PaddingX(12).Cursor(ui.CursorPointer)
			kit.Tooltip(c, row, f.Path)
			if i > 0 {
				row.BorderWidth(1, 0, 0, 0).BorderColor(p.Border)
			}
			hovered := row.Hovered() || row.FocusVisible()
			if hovered {
				row.Background(p.ListHover)
			}
			done := all || ch.Undone[f.Path]
			row.Children(func() {
				dir := f.Path[:strings.LastIndexByte(f.Path, '/')+1]
				ui.RichText(c, ui.Span{Text: dir, Color: p.MutedForeground}, ui.Span{Text: f.Path[len(dir):]}).FontSize(14).SingleLine().
					Grow(1).Shrink(1).MinWidth(0)
				ui.Row(c).Width(deltaWidth).Shrink(0).Justify(ui.End).Children(func() {
					if !hovered {
						diffBadge(c, f.Added, f.Removed)
						return
					}
					kit.IconButton(c.Key("hunks"), icons.ListChecks, i18n.T(c, "hunks_review"), 28, false).OnClick(func() { actions.Review(turn, f.Path) })
					label := "turn_changes_undo"
					if done {
						label = "turn_changes_undone"
					}
					kit.IconButton(c.Key("undo"), icons.Undo, i18n.T(c, label), 28, false).Disabled(done).OnClick(func() { actions.Undo(turn, f.Path) })
				})
			}).OnClick(func() { actions.Review(turn, f.Path) })
		}
		if n > previewFiles {
			label := i18n.T(c, "turn_changes_more", "count", n-previewFiles)
			if *more {
				label = i18n.T(c, "turn_changes_less")
			}
			t := ui.ButtonBase(c.Key("more")).Label(label).Expanded(*more).FillWidth().Height(40).PaddingX(12).Gap(8).
				BorderWidth(1, 0, 0, 0).BorderColor(p.Border).Background(p.Muted.Alpha(0.2)).Cursor(ui.CursorPointer).Justify(ui.Start)
			hovered := t.Hovered()
			t.Children(func() {
				ui.Text(c, label).FontSize(14).TextColor(p.MutedForeground)
				chevron := icons.ChevronRight
				if *more {
					chevron = icons.ChevronDown
				}
				ch := ui.Icon(c, chevron).FontSize(caretSize).TextColor(p.MutedForeground)
				if !hovered {
					ch.Opacity(0)
				}
			}).OnClick(func() { *more = !*more })
		}
	})
}

// Cite places the answer's sources: " [n](<uri>)" after the rune at each
// citation's End, in order, and those without an End after the text. The view
// shows the numbers as links.
func Cite(text string, cites []Citation) string {
	if len(cites) == 0 {
		return text
	}
	runes := utf8.RuneCountInString(text)
	at := map[int][]int{} // byte offset → citation numbers
	var order []int
	offset := func(r int) int {
		if r >= runes {
			return len(text)
		}
		n := 0
		for i := range text {
			if n == r {
				return i
			}
			n++
		}
		return len(text)
	}
	for i, ct := range cites {
		pos := len(text)
		if ct.End > 0 {
			pos = offset(ct.End)
		}
		if _, ok := at[pos]; !ok {
			order = append(order, pos)
		}
		at[pos] = append(at[pos], i)
	}
	// Offsets in ascending order.
	for i := 1; i < len(order); i++ {
		for j := i; j > 0 && order[j] < order[j-1]; j-- {
			order[j], order[j-1] = order[j-1], order[j]
		}
	}
	var b strings.Builder
	copied := 0
	for _, pos := range order {
		b.WriteString(text[copied:pos])
		for _, i := range at[pos] {
			uri := strings.NewReplacer("<", "%3C", ">", "%3E", "\\", "%5C", "\n", "%0A", "\r", "%0D").Replace(cites[i].URI)
			fmt.Fprintf(&b, " [%d](<%s>)", i+1, uri)
		}
		copied = pos
	}
	b.WriteString(text[copied:])
	return b.String()
}
