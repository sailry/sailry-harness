package settings

import (
	"fmt"
	"strings"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// The model library (settings/model_library*): a model page's Available
// tab, with the hub and the query beside the tabs over the page's
// recommended models and the hub's models of the page's kind, each with its
// size and status, its download's progress, its details and its removal.
// The hub's next page loads as the page scrolls to its end.

var libraryHubs = []string{"model_library_huggingface", "model_library_modelscope"}

// searchDelay is how long the query rests before the hub searches it;
// Enter searches at once.
const searchDelay = 400 * time.Millisecond

// moreDistance is how near the page's end the hub's next page loads.
const moreDistance = 320

// library is the hub, the query as typed and as searched, and the model whose
// details or removal show. It is open while a page's Available tab shows
// (Page clears it each frame), for that page's kind, and counts the
// searches run (LibraryRequest).
type library struct {
	open            bool
	hub             int
	query, searched string
	typed           time.Time // when the query last changed
	rows            int       // the hub's rows the page last laid out
	info, removing  string
	kind            string
	serial          uint64
	// prefixes are an embedding model's prefixes as the details edit them,
	// taken from the model each time it resolves another definition
	// (prefixed).
	prefixes [2]string
	prefixed string
}

func (m *Model) libraryModel(id string) *preview.LibraryModel {
	for i := range m.Library {
		if m.Library[i].ID == id {
			return &m.Library[i]
		}
	}
	return nil
}

// showLibrary opens the library for a page's kind as its Available tab
// shows. Another kind starts empty on the kind's hub (ModelScope for OCR)
// with its first page on the way; the preview keeps its models.
func (m *Model) showLibrary(kind, query string) {
	e := &m.library
	if e.kind != kind {
		*e = library{kind: kind, query: query, searched: strings.TrimSpace(query)}
		if kind == ModelsOCR {
			e.hub = 1
		}
		m.LibraryLoad = preview.Fetch{}
		if m.Live {
			m.Library, m.LibraryLoad = nil, preview.Fetch{Loading: true}
			m.LibraryMore, m.LibraryFailed = false, false
		}
	}
	e.open = true
}

// searchLibrary searches the hub for the query as typed.
func (m *Model) searchLibrary(query string) {
	e := &m.library
	e.searched = strings.TrimSpace(query)
	e.serial++
	if !m.Live {
		m.LibraryLoad.Start()
	}
}

// libraryBar is a model page's Installed and Available tabs; Available
// adds the hub and the query, which filters the recommended models at once
// and searches the hub once it rests or on Enter.
func libraryBar(c *ui.Context, m *Model, kind, title string, tab *int, query *string) {
	t := func(k string, a ...any) string { return i18n.T(c, k, a...) }
	e := &m.library
	if *tab == 1 {
		m.showLibrary(kind, *query)
		if *query != e.query {
			e.query, e.typed = *query, c.Now()
		}
		if q := strings.TrimSpace(*query); q != e.searched {
			if wait := searchDelay - c.Now().Sub(e.typed); wait > 0 {
				c.After(wait)
			} else {
				m.searchLibrary(q)
			}
		}
	}
	ui.Row(c).Gap(12).Wrap().Children(func() {
		kit.Segmented(c, "tabs", tab, []string{t("knowledge_installed"), t("local_models_available")}, kit.SegmentedProps{}).Label(title)
		if *tab != 1 {
			return
		}
		if pick(c, "hub", t("model_library_source"), &e.hub, labels(c, libraryHubs), false).Width(160).Changed() && !m.Live {
			m.LibraryLoad.Start()
		}
		ui.SearchField(c.Key("search"), query).Label(t("model_library_query")).Placeholder(t("model_library_query")).Width(260).
			OnSubmit(func() { m.searchLibrary(*query) })
	})
}

// recommendedModels is the page's recommended models the query matches,
// under their heading; none hides it.
func recommendedModels(c *ui.Context, count int, rows func()) {
	if count == 0 {
		return
	}
	kit.Group(c, "recommended", i18n.T(c, "local_models_recommended"), "", nil, func() { kit.Card(c, rows) })
}

// hubModels are the hub's models of the library's kind under their heading,
// but for those the page recommends (repos): placeholder rows before the
// first page, a spinner over the rows during a later search, and the next
// page as the page nears its end.
func hubModels(c *ui.Context, m *Model, repos map[string]bool) {
	e := &m.library
	t := func(k string, a ...any) string { return i18n.T(c, k, a...) }
	load := &m.LibraryLoad
	// A live library's pages come from the host's hub search, which
	// follows LibraryRequest and sets the load; the preview plays its own
	// on the frame clock.
	if !m.Live {
		if !load.Loaded && !load.Loading {
			load.Start() // the first page, as the tab shows
		}
		if load.Advance(c.Now()) {
			c.After(preview.Tick)
		}
	}
	q := strings.ToLower(e.searched)
	var shown []*preview.LibraryModel
	for i := range m.Library {
		x := &m.Library[i]
		if repos[x.Repo] {
			continue
		}
		// A live library holds what the hub found for the request.
		if m.Live || x.Kind == e.kind && x.Source == libraryHubs[e.hub] && (q == "" || strings.Contains(strings.ToLower(x.Name+" "+x.Repo), q)) {
			shown = append(shown, x)
		}
	}
	kit.Group(c, "hub", t("model_library_available"), "", nil, func() {
		if load.Loading && !load.Loaded {
			librarySkeleton(c)
			return
		}
		if len(shown) == 0 {
			props := kit.ResultProps{Icon: icons.Search, Title: t("model_library_no_results"), Card: true}
			if m.Live && m.LibraryFailed {
				// The failure was the toast; the search runs again from here.
				props.Extra = func() {
					kit.Secondary(c.Key("retry"), t("settings_retry")).OnClick(func() { m.searchLibrary(e.searched) })
				}
			}
			kit.Result(c, props)
			return
		}
		ui.Column(c.Key("found")).FillWidth().Gap(12).Children(func() {
			libraryRows(c, m, shown)
			if load.Loading && m.Live && m.LibraryMore {
				// The next page loads under the rows.
				ui.Row(c.Key("more")).Justify(ui.Center).PaddingY(4).Busy(true).Children(func() { kit.Loader(c, 16) })
			} else if load.Loading {
				// A later search keeps the rows under a spinner.
				ui.Box(c.Key("library-loading")).Absolute().Fill().Center().Busy(true).Children(func() { kit.Loader(c, 20) })
			}
		})
		moreLibrary(c, m, len(shown))
	})
}

// moreLibrary reads the hub's next page once the page is scrolled near its
// end, or does not fill its height, as the frame laid it out with the rows
// shown: a page whose rows just arrived waits for the next layout.
func moreLibrary(c *ui.Context, m *Model, rows int) {
	e := &m.library
	if rows != e.rows {
		e.rows = rows
		c.After(time.Millisecond)
		return
	}
	s := &m.pageScroll
	if !m.Live || !m.LibraryMore || m.LibraryLoad.Loading || s.MaxY-s.Y > moreDistance {
		return
	}
	if m.ModelActions != nil && m.ModelActions.LibraryMore != nil {
		m.ModelActions.LibraryMore()
	}
}

// libraryRows are the hub's models, with the recommended rows' details
// entry.
func libraryRows(c *ui.Context, m *Model, shown []*preview.LibraryModel) {
	e := &m.library
	t := func(k string, a ...any) string { return i18n.T(c, k, a...) }
	p := theme.Of(c)
	kit.Card(c, func() {
		for _, x := range shown {
			kit.Entry(c, x.ID, func() {
				// The page names the kind.
				detail := x.Repo
				if x.Size != "" { // a hub's result has no size until it is resolved
					detail += " · " + x.Size
				}
				kit.Record(c, x.Name, detail)
			}, func() {
				switch x.Status {
				case preview.ModelDownloading:
					ui.Row(c).Gap(8).Children(func() {
						ui.Progress(c, x.Percent/100).Width(80).Height(4)
						ui.Text(c, t("knowledge_downloading")).FontSize(12).TextColor(p.MutedForeground)
					})
				case preview.ModelInstalled:
					kit.Badge(c, t("knowledge_installed"), kit.Positive)
				}
			}, func() {
				kit.Action(c.Key("info"), icons.Settings2, t("knowledge_model_details"), false).OnClick(func() {
					e.info, e.prefixed = x.ID, ""
					if m.Live && m.ModelActions != nil {
						m.ModelActions.LibraryDetails(x.ID)
					}
				})
				switch x.Status {
				case preview.ModelMissing:
					kit.Action(c.Key("install"), icons.Download, t("knowledge_install"), false).OnClick(func() {
						m.modelAction(c, func(a *ModelActions) { a.LibraryInstall(x.ID, nil) })
					})
				case preview.ModelDownloading:
					kit.Action(c.Key("cancel"), icons.CircleX, t("settings_cancel"), false).OnClick(func() {
						m.modelAction(c, func(a *ModelActions) { a.LibraryCancel(x.ID) })
					})
				default:
					kit.Action(c.Key("delete"), icons.Trash, t("knowledge_delete"), false).OnClick(func() { e.removing = x.ID })
				}
			})
		}
	})
}

// librarySkeleton holds the results' place before the first page: three rows
// of a name and a repository line over a badge.
func librarySkeleton(c *ui.Context) {
	kit.Card(c, func() {
		for _, w := range []float32{180, 140, 210} {
			ui.Row(c.Key(w)).FillWidth().PaddingY(12).Gap(12).Children(func() {
				ui.Column(c).Grow(1).Shrink(1).MinWidth(0).Gap(8).Children(func() {
					kit.Skeleton(c, kit.SkeletonProps{Width: w, Height: 16, Radius: 2})
					kit.Skeleton(c, kit.SkeletonProps{Width: w * 1.6, Height: 12, Radius: 2, Secondary: true})
				})
				kit.Skeleton(c, kit.SkeletonProps{Width: 48, Height: 24, Secondary: true})
			})
		}
	}).Busy(true)
}

// libraryDialogs are a model's details and its removal. The details show
// the model's source, the version to install, its license and size and an
// embedding model's prefixes, over Install for the version shown.
func libraryDialogs(c *ui.Context, m *Model) {
	e := &m.library
	t := func(k string, a ...any) string { return i18n.T(c, k, a...) }
	if x := m.libraryModel(e.info); x != nil {
		open := true
		embedding := x.Kind == ModelsEmbedding
		// The prefixes follow the definition shown until the user types.
		if shown := x.ID + "\x00" + fmt.Sprint(x.Version, x.Resolved); e.prefixed != shown {
			e.prefixed, e.prefixes = shown, [2]string{x.QueryPrefix, x.DocumentPrefix}
		}
		resolved := !m.Live || x.Resolved
		current := !m.Live || x.Current
		kit.Dialog(c.Key("model-library-details"), &open, func() {
			ui.Text(c, x.Name).FontSize(16).FontWeight(600)
			rows := [][2]string{{t("model_library_source"), t(x.Source) + " · " + x.Repo}}
			for _, row := range [][2]string{{t("settings_license"), x.License}, {t("knowledge_model_size"), x.Size}} {
				// A live model's size and license show once the hub resolved it.
				if !m.Live || row[1] != "" {
					rows = append(rows, row)
				}
			}
			facts(c, rows)
			if len(x.Versions) > 0 {
				version := x.Version
				title := t("model_library_version")
				kit.Field(c, "version", title, "", func() {
					if pick(c, "select", title, &version, x.Versions, false).Fill().Changed() && m.ModelActions != nil && m.ModelActions.LibraryVersion != nil {
						m.ModelActions.LibraryVersion(x.ID, version)
					}
				})
			}
			if embedding {
				for i, key := range []string{"model_library_query_prefix", "model_library_document_prefix"} {
					kit.Field(c, key, t(key), "", func() {
						ui.TextInput(c.Key("input"), &e.prefixes[i]).Label(t(key)).FillWidth()
					})
				}
			}
			ui.Row(c.Key("footer")).Gap(8).Justify(ui.End).Children(func() {
				kit.Secondary(c.Key("cancel"), t("settings_cancel")).OnClick(func() { e.info = "" })
				label, installed := t("knowledge_install"), current && x.Status == preview.ModelInstalled
				if installed {
					label = t("knowledge_installed")
				}
				busy := current && x.Status == preview.ModelDownloading
				kit.PrimaryBusy(c.Key("install"), label, !resolved || installed, busy).OnClick(func() {
					m.modelAction(c, func(a *ModelActions) {
						a.LibraryInstall(x.ID, &LibraryChoice{QueryPrefix: e.prefixes[0], DocumentPrefix: e.prefixes[1]})
					})
				})
			})
		}).Width(min(480, width(c)-48)).Label(x.Name)
		if !open {
			e.info = ""
		}
	}
	if x := m.libraryModel(e.removing); x != nil {
		open := true
		title, consequence := t("knowledge_delete"), t("knowledge_delete_model_effect", "name", x.Name)
		if m.Live && x.Kind != ModelsEmbedding {
			// Only an embedding model's removal changes collections; the
			// others name the model, as the reference's prompt does.
			title, consequence = t("knowledge_delete"), x.Name
		}
		if confirm(c, "library-delete", &open, title, consequence, t("knowledge_delete")) {
			if m.Live {
				// The host deletes the files; its outcome is the toast.
				if m.ModelActions != nil {
					m.ModelActions.LibraryDelete(x.ID)
				}
			} else {
				x.Status = preview.ModelMissing
				kit.Toast(c, kit.Done, t("knowledge_deleted"))
			}
		}
		if !open {
			e.removing = ""
		}
	}
}
