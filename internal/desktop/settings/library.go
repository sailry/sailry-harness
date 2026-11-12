package settings

import (
	"fmt"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// The model library (settings/model_library*): a dialog opened by Manage
// models, with the hub, the query and Search over the models found, each
// with its kind, size and status, its download's progress, its details
// and its removal.

var libraryHubs = []string{"model_library_huggingface", "model_library_modelscope"}

// library is the hub, the query as typed and as searched, and the model whose
// details or removal show. A live library is opened for a page's kind and
// counts the searches run (LibraryRequest).
type library struct {
	open            bool
	hub             int
	query, searched string
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

func libraryDialog(c *ui.Context, m *Model) {
	e := &m.library
	t := func(k string, a ...any) string { return i18n.T(c, k, a...) }
	if e.open {
		title := t("model_library_manage")
		load := &m.LibraryLoad
		// A live library's pages come from the host's hub search, which
		// follows LibraryRequest and sets the load; the preview plays its
		// own on the frame clock.
		if !m.Live {
			if !load.Loaded && !load.Loading {
				load.Start() // the first page, as the dialog opens
			}
			if load.Advance(c.Now()) {
				c.After(preview.Tick)
			}
		}
		search := func() {
			e.searched = e.query
			e.serial++
			if !m.Live {
				load.Start()
			}
		}
		kit.Dialog(c.Key("model-library"), &e.open, func() {
			ui.Text(c, title).FontSize(16).FontWeight(600)
			ui.Row(c.Key("controls")).Gap(12).Children(func() {
				pick(c, "hub", t("model_library_source"), &e.hub, labels(c, libraryHubs), false).Width(192)
				ui.TextInput(c.Key("query"), &e.query).Label(t("model_library_query")).Placeholder(t("model_library_query")).Grow(1).
					OnSubmit(search)
				kit.Action(c.Key("search"), icons.Search, t("model_library_search"), false).OnClick(search)
			})
			q := strings.ToLower(strings.TrimSpace(e.searched))
			var shown []*preview.LibraryModel
			for i := range m.Library {
				x := &m.Library[i]
				// A live library holds what the hub found for the request.
				if m.Live || x.Source == libraryHubs[e.hub] && (q == "" || strings.Contains(strings.ToLower(x.Name+" "+x.Repo), q)) {
					shown = append(shown, x)
				}
			}
			ui.Scroll(c.Key("results")).MaxHeight(420).Children(func() {
				if load.Loading && !load.Loaded {
					librarySkeleton(c)
					return
				}
				if len(shown) == 0 {
					props := kit.ResultProps{Icon: icons.Search, Title: t("model_library_empty"), Card: true}
					if m.Live && m.LibraryFailed {
						// The failure was the toast; the search runs again from here.
						props.Extra = func() { kit.Secondary(c.Key("retry"), t("settings_retry")).OnClick(search) }
					}
					kit.Result(c, props)
					return
				}
				ui.Column(c.Key("found")).FillWidth().Gap(12).Children(func() {
					libraryRows(c, m, shown)
					if m.Live && m.LibraryMore {
						ui.Row(c.Key("more")).Justify(ui.Center).Children(func() {
							kit.Secondary(c.Key("load"), t("model_library_more")).OnClick(func() {
								if m.ModelActions != nil && m.ModelActions.LibraryMore != nil && !load.Loading {
									m.ModelActions.LibraryMore()
								}
							})
						})
					}
					if load.Loading {
						// A later search keeps the rows under a spinner.
						ui.Box(c.Key("library-loading")).Absolute().Fill().Center().Busy(true).Children(func() { kit.Loader(c, 20) })
					}
				})
			})
		}).Width(min(640, width(c)-48)).Label(title)
	}
	libraryDialogs(c, m)
}

// libraryRows are the models found.
func libraryRows(c *ui.Context, m *Model, shown []*preview.LibraryModel) {
	e := &m.library
	t := func(k string, a ...any) string { return i18n.T(c, k, a...) }
	p := theme.Of(c)
	kit.Card(c, func() {
		for _, x := range shown {
			kit.Entry(c, x.ID, func() {
				detail := x.Repo + " · " + t(x.Kind)
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
				kit.Action(c.Key("info"), icons.Info, t("knowledge_model_details"), false).OnClick(func() {
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
		title, consequence := t("knowledge_delete")+" "+x.Name, t("knowledge_delete_model_effect", "name", x.Name)
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
