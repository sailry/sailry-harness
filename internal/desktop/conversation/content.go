package conversation

import (
	"fmt"
	"net/url"
	"strings"
	"time"
	"unicode/utf8"

	"github.com/ELY-Open-Source/CrychicUI/agent"
	"github.com/egoist/mygo/ui"
	"github.com/egoist/mygo/ui/markdown"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/kit/crychic"
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
	size := textSize(c)
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
					ui.Text(c, f.Name()).FontSize(size).SingleLine()
					ui.Text(c, Kind(c, f)).FontSize(size - 2).SingleLine().TextColor(p.MutedForeground)
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
				if f.Document {
					if m.Item(i18n.T(c, "conversation_export_html")).Disabled(!f.Exportable).Chosen() {
						actions.FileAction(f, ExportDocumentHTML)
					}
					if !f.Interactive && m.Item(i18n.T(c, "conversation_export_image")).Disabled(!f.Exportable).Chosen() {
						actions.FileAction(f, ExportDocumentImage)
					}
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
	// A picture that cannot show is a line of text, not an empty frame.
	shown := make([]Image, 0, len(list))
	missing := 0
	for _, im := range list {
		if im.Missing && im.Source == nil {
			missing++
			continue
		}
		shown = append(shown, im)
	}
	row := ui.Row(c.Key(key)).Wrap().Gap(8)
	if end {
		row.Justify(ui.End)
	}
	if len(shown) > 0 {
		kit.ImageGroup(c, row, &s.view, shown, func(_ int, im Image) (ui.ImageSource, kit.ImageProps) {
			w, h := float32(im.Width), float32(im.Height)
			if w <= 0 || h <= 0 {
				w, h = thumbMax, thumbMax
			}
			k := min(1, thumbMax/max(w, h))
			return im.Source, kit.ImageProps{Width: w * k, Height: h * k, Alt: im.Name}
		})
	}
	for i := range missing {
		ui.Text(c.Key(struct {
			Key   string
			Index int
		}{key, i}), i18n.T(c, "chat_image_missing")).FontSize(textSize(c)).TextColor(theme.Of(c).MutedForeground)
	}
}

// visual is a VisualBlock: the chart or diagram of its Mermaid source in
// place, with no card around it, under the block's title as one muted
// caption line (visuals.go). A source that cannot be drawn shows as code.
func visual(c *ui.Context, key string, v *Visual) {
	if v == nil || v.Mermaid == "" {
		return
	}
	box := ui.Column(c.Key(key)).FillWidth().MinWidth(0)
	if showVisual(c, box, v.Mermaid, v.Title) {
		return
	}
	box.Gap(visualGap).Children(func() {
		caption(c, v.Title)
		kit.Code(c, ui.Column(c.Key("source")), v.Mermaid, kit.CodeOptions{Tint: true})
	})
}

// notices are background results, on the user's side as a bubble: each task's
// icon, title and outcome in its tone.
func notices(c *ui.Context, list []Notice) {
	p := theme.Of(c)
	size := textSize(c)
	ui.Row(c.Key("notices")).Justify(ui.End).Children(func() {
		ui.Column(c).Gap(6).Padding(9, 14).Radius(p.Radius).Background(kit.Veil(c, p.Muted)).MaxWidth(bubbleMax).
			Label(i18n.T(c, "background_notice_title")).Children(func() {
			for i, n := range list {
				ui.Row(c.Key(i)).Gap(8).MinWidth(0).AlignItems(ui.Center).Children(func() {
					icon := icons.Terminal
					if n.Agent {
						icon = icons.Bot
					}
					ui.Icon(c, icon).FontSize(rowIcon(size)).Shrink(0).TextColor(p.MutedForeground)
					ui.Text(c, n.Title).FontSize(size).SingleLine().Grow(1).Shrink(1).MinWidth(0)
					ui.Text(c, i18n.T(c, outcomeKey(n.Outcome))).FontSize(size).Shrink(0).TextColor(outcomeTone(p, n.Outcome))
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

// grounding is a web search's row: it opens the result over the window,
// the answer, its numbered sources and the provider's suggestions.
func (s *State) grounding(c *ui.Context, key string, b Block) {
	head := Call{Label: i18n.T(c, "chat_search_result"), Icon: "globe"}
	t := rowSize(ui.ButtonBase(c.Key("search-"+key)), textSize(c), false).Gap(8).AlignSelf(ui.Start).MaxWidthPercent(100).
		Justify(ui.Start).Cursor(ui.CursorPointer)
	hovered := t.Hovered() || t.FocusVisible()
	sw := &kit.Sweep{}
	t.Children(func() { summary(c, head, hovered, false, false, sw) }).OnClick(func() {
		result := b
		s.result = &result
	})
	sw.Frame(t)
}

// webLink reports an http or https address with a host and no credentials:
// the only destinations a search result opens, in the system browser.
func webLink(link string) bool {
	u, err := url.Parse(link)
	return err == nil && (u.Scheme == "https" || u.Scheme == "http") && u.Host != "" && u.User == nil
}

// searchResult is the open search result: a dialog in place of the
// reference's window, since the transcript has no second window.
func (s *State) searchResult(c *ui.Context) {
	if s.result == nil {
		return
	}
	p := theme.Of(c)
	size := textSize(c) // the answer's and its sources', under the dialog's own title
	b := s.result
	open := true
	options := markdown.Options{Theme: s.md.Theme, OnLink: func(link string) {
		if webLink(link) {
			c.OpenURL(link)
		}
	}}
	if s.resultMD == nil {
		s.resultMD = &options
	} else {
		s.resultMD.Theme, s.resultMD.OnLink = options.Theme, options.OnLink
	}
	kit.Dialog(c, &open, func() {
		ui.Text(c, i18n.T(c, "chat_search_result")).FontSize(16).FontWeight(600)
		ui.Scroll(c.Key("search-answer")).Width(640).MaxWidthPercent(100).MaxHeight(480).Children(func() {
			ui.Column(c).FillWidth().Gap(12).Children(func() {
				if strings.TrimSpace(b.Text) != "" {
					kit.Markdown(c, b.Text, kit.MarkdownProps{Variant: kit.Chat, Options: s.resultMD})
				}
				// The numbered sources are CrychicUI's source cards, two
				// across: the number, the host, the title and the address,
				// in the message text size (the library's sizes are shares
				// of its theme's), each opening its page.
				sized := *c.Theme()
				sized.FontSize = size
				ui.Themed(c.Key("sources"), &sized).FillWidth().Background(ui.Transparent).Children(func() {
					ui.Row(c).FillWidth().Wrap().Gap(8).AlignItems(ui.Stretch).Children(func() {
						for i, src := range b.Sources {
							if !webLink(src.URI) {
								continue
							}
							ui.Column(c.Key(i)).Grow(1).Shrink(1).Basis(280).MinWidth(0).Children(func() {
								crychic.SourceCard(c, i+1, agent.Source{Title: src.Title, URL: src.URI})
							})
						}
					})
				})
				if len(b.Suggestions) > 0 {
					ui.Box(c).Height(1).FillWidth().Background(p.Border)
					ui.Column(c.Key("suggestions")).FillWidth().Gap(4).Children(func() {
						for i, src := range b.Suggestions {
							if !webLink(src.URI) {
								continue
							}
							title := src.Title
							if title == "" {
								title = src.URI
							}
							ui.Link(c.Key(i), title, src.URI).FontSize(size).SingleLine().Shrink(1).MinWidth(0)
						}
					})
				}
			})
		})
	})
	if !open {
		s.result = nil
	}
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
	head := Call{Label: label, Icon: "network", Running: active && !r.Stopped, Warning: r.Exhausted || r.Stopped}
	// The reason a retry stopped for; exhausted attempts are the label itself.
	if r.Reason != "" && !r.Exhausted {
		head.Detail = i18n.T(c, r.Reason)
	}
	if r.Detail == "" {
		plain(c, "retry-"+key, head)
		return
	}
	s.disclosure(c, "retry-"+key, func(hovered, open bool, sw *kit.Sweep) { summary(c, head, hovered, true, open, sw) }, 4, 8, func() {
		ui.Scroll(c).MaxHeight(160).Children(func() {
			ui.Text(c, r.Detail).Selectable().FontSize(textSize(c)).TextColor(p.MutedForeground)
		})
	})
}

// marker is the row of a turn a package continued, in the window's language:
// the package's label between two lines, as a compaction's marker and one
// row high.
func marker(c *ui.Context, m *Marker) {
	p := theme.Of(c)
	size := textSize(c)
	label := m.Label
	if s := m.Locales[i18n.Match(c.Locale())]; s != "" {
		label = s
	}
	if label == "" {
		label = i18n.T(c, "turn_continued")
	}
	ui.Row(c.Key("marker")).FillWidth().Height(rowHeight(size)).Gap(8).AlignItems(ui.Center).TextColor(p.MutedForeground).Children(func() {
		ui.Box(c).Height(1).Grow(1).Background(p.Border)
		ui.Text(c, label).FontSize(size)
		ui.Box(c).Height(1).Grow(1).Background(p.Border)
	})
}

// compaction is a compaction turn: a centered status between two lines,
// shimmering while it runs, with the failure's cause as its tooltip; a
// finished one opens its summary, a failed one its cause.
func (s *State) compaction(c *ui.Context, t *Turn) {
	p := theme.Of(c)
	size := textSize(c)
	k := t.Compaction
	key := map[CompactionState]string{
		Compacted: "chat_compacted", CompactionRunning: "chat_compacting", CompactionQueued: "chat_compact_queued",
		CompactionFailed: "chat_compact_failed", CompactionCancelled: "chat_compact_cancelled", CompactionUnneeded: "chat_compact_unneeded",
		CompactionInterrupted: "chat_compact_interrupted",
	}[k.State]
	label := i18n.T(c, key)
	// The state and how long the compaction ran, as a run's header.
	if k.Elapsed >= time.Second {
		label += " " + Elapsed(k.Elapsed)
	}
	stopped := k.State == CompactionFailed || k.State == CompactionInterrupted
	switch {
	case k.State == CompactionFailed || k.State == CompactionInterrupted && strings.TrimSpace(k.Cause) != "":
		head := Call{Label: label, Icon: "triangle-alert", Failed: true}
		cause := k.Cause
		if strings.TrimSpace(cause) == "" {
			cause = i18n.T(c, "compact_reason_unknown")
		}
		s.disclosure(c, "compaction-"+t.ID, func(hovered, open bool, sw *kit.Sweep) { summary(c, head, hovered, true, open, sw) }, 4, 8, func() {
			ui.Text(c, cause).Selectable().FontSize(size).TextColor(p.Danger)
		})
	case k.Summary != "":
		head := Call{Label: label, Icon: "file-text"}
		s.disclosure(c, "compaction-"+t.ID, func(hovered, open bool, sw *kit.Sweep) { summary(c, head, hovered, true, open, sw) }, 4, 8, func() {
			ui.Scroll(c).MaxHeight(240).Children(func() {
				ui.Text(c, k.Summary).Selectable().FontSize(size).LineHeight(22.0 / 14).TextColor(p.MutedForeground)
			})
		})
	default:
		tone := p.MutedForeground
		if stopped {
			tone = p.Danger
		}
		ui.Row(c.Key("compaction")).FillWidth().Height(rowHeight(size)).Gap(8).AlignItems(ui.Center).TextColor(tone).Children(func() {
			ui.Box(c).Height(1).Grow(1).Background(p.Border)
			if k.State == CompactionRunning || k.State == CompactionQueued {
				kit.Shimmer(c, label, size, p.MutedForeground)
			} else {
				ui.Text(c, label).FontSize(size)
			}
			ui.Box(c).Height(1).Grow(1).Background(p.Border)
		})
	}
}

func diffBadge(c *ui.Context, added, removed int) {
	p := theme.Of(c)
	size := textSize(c) - 2
	ui.Row(c).Gap(6).Shrink(0).Children(func() {
		if added > 0 || removed == 0 {
			ui.Text(c, fmt.Sprintf("+%d", added)).Font("monospace").FontSize(size).TextColor(p.Success)
		}
		if removed > 0 {
			ui.Text(c, fmt.Sprintf("−%d", removed)).Font("monospace").FontSize(size).TextColor(p.Danger)
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
	size := textSize(c) // the texts'; the card's tile, rows and buttons keep their sizes
	added, removed := 0, 0
	for _, f := range ch.Files {
		added, removed = added+f.Added, removed+f.Removed
	}
	moreKey := "changes-more-" + turn
	more := s.opened(moreKey)
	all := ch.Undone[""]
	ui.Column(c.Key("changes")).FillWidth().Radius(p.Radius2xl()).Border(1, p.Border).Background(p.Muted.Alpha(0.2)).Clip().Children(func() {
		ui.Row(c).FillWidth().MinWidth(0).Padding(12).Gap(12).AlignItems(ui.Center).Children(func() {
			ui.Box(c).Size(40, 40).Shrink(0).Radius(p.Radius2xl()).Background(p.MutedForeground.Alpha(0.12)).Center().Children(func() {
				ui.Icon(c, icons.FileText).FontSize(20).TextColor(p.MutedForeground)
			})
			ui.Column(c).Grow(1).MinWidth(0).Gap(4).Children(func() {
				ui.Text(c, i18n.T(c, "turn_changes_count", "count", len(ch.Files))).FontSize(size)
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
		if !more {
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
				ui.RichText(c, ui.Span{Text: dir, Color: p.MutedForeground}, ui.Span{Text: f.Path[len(dir):]}).FontSize(size).SingleLine().
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
			if more {
				label = i18n.T(c, "turn_changes_less")
			}
			t := ui.ButtonBase(c.Key("more")).Label(label).Expanded(more).FillWidth().Height(40).PaddingX(12).Gap(8).
				BorderWidth(1, 0, 0, 0).BorderColor(p.Border).Background(p.Muted.Alpha(0.2)).Cursor(ui.CursorPointer).Justify(ui.Start)
			hovered := t.Hovered()
			t.Children(func() {
				ui.Text(c, label).FontSize(size).TextColor(p.MutedForeground)
				chevron := icons.ChevronRight
				if more {
					chevron = icons.ChevronDown
				}
				ch := ui.Icon(c, chevron).FontSize(caretSize(size)).TextColor(p.MutedForeground)
				if !hovered {
					ch.Opacity(0)
				}
			}).OnClick(func() { s.toggle(moreKey) })
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
