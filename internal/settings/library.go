package settings

import (
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-go/internal/i18n"
	"github.com/sailry/sailry-go/internal/icons"
	"github.com/sailry/sailry-go/internal/kit"
	"github.com/sailry/sailry-go/internal/preview"
	"github.com/sailry/sailry-go/internal/theme"
)

// The model library (settings/model_library*): a dialog opened by Manage
// models, with the hub, the query and Search over the models found, each
// with its kind, size and status, its download's progress, its details
// and its removal.

var libraryHubs = []string{"model_library_huggingface", "model_library_modelscope"}

// library is model_library.rs's state: the hub, the query as typed and
// as searched, and the model whose details or removal show.
type library struct {
	open            bool
	hub             int
	query, searched string
	info, removing  string
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
		if !load.Loaded && !load.Loading {
			load.Start() // the first page, as the dialog opens
		}
		if load.Advance(c.Now()) {
			c.After(preview.Tick)
		}
		search := func() { e.searched = e.query; load.Start() }
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
				if x.Source == libraryHubs[e.hub] && (q == "" || strings.Contains(strings.ToLower(x.Name+" "+x.Repo), q)) {
					shown = append(shown, x)
				}
			}
			ui.Scroll(c.Key("results")).MaxHeight(420).Children(func() {
				if load.Loading && !load.Loaded {
					librarySkeleton(c)
					return
				}
				if len(shown) == 0 {
					kit.EmptyGroup(c, icons.Search, t("model_library_empty"))
					return
				}
				ui.Column(c.Key("found")).FillWidth().Children(func() {
					libraryRows(c, m, shown)
					if load.Loading {
						// A later search keeps the rows under a spinner.
						ui.Box(c.Key("library-loading")).Absolute().Fill().Center().Busy(true).Children(func() { kit.Loader(c, 16) })
					}
				})
			})
		}).Width(min(640, width(c)-48)).Label(title)
	}
	libraryDialogs(c, m)
}

// libraryRows are the models found (model_library/view.rs results).
func libraryRows(c *ui.Context, m *Model, shown []*preview.LibraryModel) {
	e := &m.library
	t := func(k string, a ...any) string { return i18n.T(c, k, a...) }
	p := theme.Of(c)
	kit.Card(c, func() {
		for _, x := range shown {
			kit.Entry(c, x.ID, func() {
				kit.Record(c, x.Name, x.Repo+" · "+t(x.Kind)+" · "+x.Size)
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
				kit.Action(c.Key("info"), icons.Info, t("knowledge_model_details"), false).OnClick(func() { e.info = x.ID })
				switch x.Status {
				case preview.ModelMissing:
					kit.Action(c.Key("install"), icons.Download, t("knowledge_install"), false).OnClick(func() { notice(c) })
				case preview.ModelDownloading:
					kit.Action(c.Key("cancel"), icons.CircleX, t("settings_cancel"), false).OnClick(func() { notice(c) })
				default:
					kit.Action(c.Key("delete"), icons.Trash, t("knowledge_delete"), false).OnClick(func() { e.removing = x.ID })
				}
			})
		}
	})
}

// librarySkeleton holds the results' place before the first page
// (model_library/view.rs pending): three rows of a name and a repository
// line over a badge.
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

// libraryDialogs are a model's details and its removal.
func libraryDialogs(c *ui.Context, m *Model) {
	e := &m.library
	t := func(k string, a ...any) string { return i18n.T(c, k, a...) }
	if x := m.libraryModel(e.info); x != nil {
		open := true
		kit.Dialog(c.Key("model-library-details"), &open, func() {
			ui.Text(c, x.Name).FontSize(16).FontWeight(600)
			facts(c, [][2]string{{t("model_library_source"), t(x.Source) + " · " + x.Repo}, {t("knowledge_model_size"), x.Size}, {t("settings_license"), x.License}})
			ui.Row(c.Key("footer")).Justify(ui.End).Children(func() {
				kit.Secondary(c.Key("close"), t("close")).OnClick(func() { open = false })
			})
		}).Width(min(480, width(c)-48)).Label(x.Name)
		if !open {
			e.info = ""
		}
	}
	if x := m.libraryModel(e.removing); x != nil {
		open := true
		if confirm(c, "library-delete", &open, t("knowledge_delete")+" "+x.Name, t("knowledge_delete_model_effect", "name", x.Name), t("knowledge_delete")) {
			x.Status = preview.ModelMissing
			kit.Toast(c, kit.Done, t("knowledge_deleted"))
		}
		if !open {
			e.removing = ""
		}
	}
}
