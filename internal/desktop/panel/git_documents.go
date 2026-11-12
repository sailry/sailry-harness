package panel

import (
	"slices"

	"github.com/egoist/mygo/ui"
	"github.com/egoist/mygo/ui/editor"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
	"github.com/sailry/sailry-harness/internal/desktop/viewmodel"
)

// GitDocumentFile retains source flags beside the existing native diff model.
type GitDocumentFile struct {
	viewmodel.FileChange
	Binary, Truncated bool
}

type gitDocument struct {
	title, message, text string
	textOnly, combined   bool
	truncated            bool
	files                []GitDocumentFile
	folded               map[string]bool
}

func cloneGitDocumentFiles(files []GitDocumentFile) []GitDocumentFile {
	files = slices.Clone(files)
	for i := range files {
		files[i].Hunks = slices.Clone(files[i].Hunks)
		for h := range files[i].Hunks {
			files[i].Hunks[h].Lines = slices.Clone(files[i].Hunks[h].Lines)
		}
	}
	return files
}

// SetGitCommit stores a captured commit in the Git component, independently of
// mutable worktree changes. The returned ID belongs to the existing Git tabs.
func (pr *Project) SetGitCommit(id, title, message string, files []GitDocumentFile, truncated bool) string {
	key := ":commit/" + id
	document := &gitDocument{title: title, message: message, combined: true, files: cloneGitDocumentFiles(files), truncated: truncated}
	pr.setGitDocument(key, document)
	return key
}

// SetGitDiff stores the captured stash patch without mutable hunk actions.
func (pr *Project) SetGitDiff(id, title string, file GitDocumentFile) string {
	key := ":stash/" + id
	pr.setGitDocument(key, &gitDocument{title: title, files: cloneGitDocumentFiles([]GitDocumentFile{file})})
	return key
}

// SetGitText keeps command output literal in a read-only native code view.
func (pr *Project) SetGitText(id, title, text string) string {
	key := ":git/" + id
	pr.setGitDocument(key, &gitDocument{title: title, text: text, textOnly: true})
	return key
}

func (pr *Project) setGitDocument(id string, document *gitDocument) {
	if pr.Git.documents == nil {
		pr.Git.documents = map[string]*gitDocument{}
	}
	document.folded = map[string]bool{}
	if previous := pr.Git.documents[id]; previous != nil {
		document.folded = previous.folded
	}
	pr.Git.documents[id] = document
	pr.Git.Show(id)
}

func (pr *Project) gitDocumentKnown(id string) bool { return pr.Git.documents[id] != nil }

func (pr *Project) gitDocumentLabel(id, fallback string) string {
	if document := pr.Git.documents[id]; document != nil {
		return document.title
	}
	return fallback
}

func (pr *Project) gitDocumentFile(c *ui.Context, box ui.Element, file GitDocumentFile, fit bool) {
	if file.Binary {
		box.Children(func() {
			ui.Text(c, i18n.T(c, "git_diff_binary")).FontSize(14).TextColor(theme.Of(c).MutedForeground)
		})
		return
	}
	old, next := file.Texts()
	kit.Diff(c, box, old, next, kit.DiffOptions{Language: editor.LanguageFor(file.Path), Fit: fit, Split: pr.diffSplit(c)})
}

func (pr *Project) gitDocument(c *ui.Context, id string) bool {
	document := pr.Git.documents[id]
	if document == nil {
		return false
	}
	if document.textOnly {
		ui.Scroll(c.Key(id)).Grow(1).MinHeight(0).Children(func() {
			kit.Code(c, ui.Column(c.Key("output")).FillWidth(), document.text, kit.CodeOptions{})
		})
		return true
	}
	p := theme.Of(c)
	added, removed := 0, 0
	for _, file := range document.files {
		added, removed = added+file.Added, removed+file.Removed
	}
	// The footer's layout choice goes with a diff: a document without
	// files, or of one binary file, shows none.
	diffs := false
	for _, file := range document.files {
		diffs = diffs || !file.Binary
	}
	if len(document.files) == 0 {
		kit.Result(c, kit.ResultProps{Icon: icons.Network, Title: i18n.T(c, "git_diff_empty")}).Grow(1).Justify(ui.Center)
	} else if !document.combined {
		pr.gitDocumentFile(c, ui.Column(c.Key(id)).Grow(1).MinHeight(0), document.files[0], false)
		if document.files[0].Truncated && !document.files[0].Binary {
			ui.Text(c, i18n.T(c, "git_diff_partial")).FontSize(14).Padding(8, 12).TextColor(p.MutedForeground)
		}
	} else {
		ui.Scroll(c.Key(id)).Grow(1).MinHeight(0).Padding(8).Gap(8).Children(func() {
			for _, file := range document.files {
				open := !document.folded[file.Path]
				ui.Column(c.Key(file.Path)).Shrink(0).Radius(p.Radius).Border(1, p.Border).Clip().Children(func() {
					head := ui.ButtonBase(c.Key("heading")).Label(file.Path).Height(32).PaddingX(8).Gap(8).Cursor(ui.CursorPointer).Expanded(open)
					if head.Hovered() {
						head.Background(p.Hover)
					}
					head.Children(func() {
						arrow := icons.ChevronRight
						if open {
							arrow = icons.ChevronDown
						}
						ui.Icon(c, arrow).FontSize(12).Shrink(0).TextColor(p.MutedForeground)
						ui.Row(c).Grow(1).Shrink(1).MinWidth(0).Gap(8).Children(func() {
							ui.Text(c, baseName(file.Path)).FontSize(14).SingleLine().Shrink(1).MinWidth(0)
							if directory := parentDir(file.Path); directory != "" {
								ui.Text(c, directory).FontSize(14).SingleLine().Shrink(1).MinWidth(0).TextColor(p.MutedForeground)
							}
						})
						stats(c, p, file.Added, file.Removed)
					}).OnClick(func() { document.folded[file.Path] = open })
					if open {
						pr.gitDocumentFile(c, ui.Column(c.Key("diff")).BorderWidth(1, 0, 0, 0).BorderColor(p.Border), file, true)
						if file.Truncated && !file.Binary {
							ui.Text(c, i18n.T(c, "git_diff_partial")).FontSize(14).Padding(8, 12).TextColor(p.MutedForeground)
						}
					}
				})
			}
			if document.truncated {
				ui.Text(c, i18n.T(c, "git_diff_partial")).FontSize(14).Padding(8, 12).TextColor(p.MutedForeground)
			}
		})
	}
	if diffs {
		pr.diffStatus(c, document.title, func() { stats(c, p, added, removed) })
	} else {
		status(c, document.title, func() { stats(c, p, added, removed) })
	}
	return true
}
