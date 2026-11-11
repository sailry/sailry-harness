package sidebar

import (
	"fmt"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/picker"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// Editor is the project editor's draft: a new project, local or cloned, or an
// existing one's name, path and appearance.
type Editor struct {
	Open    bool
	Pending bool
	Project *preview.Project // nil for a new project
	Clone   int              // 0 local, 1 clone (new projects only)
	Name    string
	Host    string // the host's name as the select shows it
	Path    string
	Repo    string
	Branch  string
	// Icon and Color are the project's appearance: folder and none on a new
	// project.
	Icon, Color string
	suggest     string // the name last suggested from the path or repository
	source      string // the path or repository the name was last suggested from
	other       string // the path the other source kept (Local | Clone)
	mode        int    // the source the path belongs to
	host        string
	// folder marks the draft Open folder saves without showing the dialog.
	folder bool
}

// Draft returns the project editor's draft as it stands.
func (s *State) Draft() Editor { return s.editor }

// Editing reports whether the project editor shows.
func (s *State) Editing() bool { return s.editor.Open }

// OpenEditor opens the project editor for pr, or for a new project.
func (s *State) OpenEditor(pr *preview.Project) {
	s.editorGeneration++
	s.editor = Editor{Open: true, Project: pr, Icon: "folder", Color: "none"}
	if pr != nil {
		s.editor.Name, s.editor.Path, s.editor.source = pr.Name, pr.Path, pr.Path
		s.editor.host = pr.Host
		if pr.Icon != "" {
			s.editor.Icon = pr.Icon
		}
		if pr.Color != "" {
			s.editor.Color = pr.Color
		}
	}
}

func (s *State) OpenEditorAtHost(id string) {
	s.OpenEditor(nil)
	s.editor.host = id
}

// OpenCloneEditor opens the existing new-project form with Clone selected.
func (s *State) OpenCloneEditor() { s.OpenEditor(nil); s.editor.Clone, s.editor.mode = 1, 1 }

// SuggestName is the name a path or a repository suggests: its last segment,
// split on / \ and :, a repository's without ".git".
func SuggestName(path string, repository bool) string {
	path = strings.TrimRight(strings.TrimSpace(path), `/\`)
	if i := strings.LastIndexAny(path, `/\:`); i >= 0 {
		path = path[i+1:]
	}
	if repository {
		path = strings.TrimSuffix(path, ".git")
	}
	return path
}

// suggestName follows the path, or a clone's repository, with the name until
// the name is typed over.
func (e *Editor) suggestName() {
	source := e.Path
	if e.cloning() {
		source = e.Repo
	}
	name := SuggestName(source, e.cloning())
	if strings.TrimSpace(e.Name) == "" || e.Name == e.suggest {
		e.Name = name
	}
	e.suggest, e.source = name, source
}

func (e *Editor) cloning() bool { return e.Clone == 1 && e.Project == nil }

// CheckProject returns the i18n key of what is wrong with a local
// project's name and path, "" when nothing is: both are needed, and the
// host has no other project at the path, a trailing slash aside.
func CheckProject(name, path, host string, others []*preview.Project, self *preview.Project) string {
	if strings.TrimSpace(name) == "" || strings.TrimSpace(path) == "" {
		return "project_required"
	}
	clean := strings.TrimRight(strings.TrimSpace(path), "/")
	for _, p := range others {
		if p != self && p.Host == host && strings.TrimRight(p.Path, "/") == clean {
			return "project_duplicate_path"
		}
	}
	return ""
}

// CheckClone returns the i18n key of what is wrong with a clone, "" when
// nothing is: a name that is a folder's (no / \ : or NUL, not . or ..), a
// repository and a destination.
func CheckClone(name, repo, path string) string {
	name = strings.TrimSpace(name)
	if name == "" || name == "." || name == ".." || strings.ContainsAny(name, "/\\:\x00") ||
		strings.TrimSpace(repo) == "" || strings.TrimSpace(path) == "" {
		return "project_clone_required"
	}
	return ""
}

// Destination is where a clone goes: the folder, then the name, with the
// folder's separator.
func Destination(path, name string) string {
	sep := "/"
	if strings.Contains(path, `\`) {
		sep = `\`
	}
	return strings.TrimRight(path, `/\`) + sep + strings.TrimSpace(name)
}

// hostLabel is a host's name as the editor and the picker's toasts show it.
func (s *State) hostLabel(c *ui.Context, h preview.Host) string {
	// The preview's hosts and the local one are named by i18n keys; a
	// device's own name shows as it is.
	if s.Actions == nil || h.Name == "local_host_name" {
		return i18n.T(c, h.Name)
	}
	return h.Name
}

// editorDialog is the project editor: on a new project a Local | Clone
// switch, then the name behind its appearance, the host, the repository and
// branch of a clone, and the folder with the host's directory picker. The
// preview saves in memory only.
func (s *State) editorDialog(c *ui.Context) {
	e := &s.editor
	if !e.Open {
		return
	}
	p := theme.Of(c)
	hosts := make([]string, len(s.Data.Hosts))
	hostIDs := make([]string, len(s.Data.Hosts))
	for i, h := range s.Data.Hosts {
		hosts[i], hostIDs[i] = s.hostLabel(c, h), h.ID
		if e.host == "" && i == 0 {
			e.host = h.ID
		}
		if e.host == h.ID {
			e.Host = hosts[i]
		}
	}
	if e.Project != nil {
		e.host = e.Project.Host
	}
	// Each source keeps its own folder.
	if e.Clone != e.mode {
		e.mode, e.Path, e.other = e.Clone, e.other, e.Path
		e.suggestName()
	}
	// The name follows the path or the repository until it is typed over.
	source := e.Path
	if e.cloning() {
		source = e.Repo
	}
	if source != e.source {
		e.suggestName()
	}
	title := i18n.T(c, "project_add")
	if e.Project != nil {
		title = i18n.T(c, "project_edit")
	}
	kit.Dialog(c.Key("editor"), &e.Open, func() {
		ui.Column(c.Key("head")).Gap(6).Children(func() {
			ui.Text(c, title).FontSize(16).FontWeight(600)
			if s.Actions == nil {
				ui.Text(c, i18n.T(c, "project_editor_preview")).FontSize(14).TextColor(p.MutedForeground)
			}
		})
		ui.Scroll(c.Key("body")).Grow(1).Shrink(1).Gap(12).Children(func() {
			if e.Project == nil {
				kit.Segmented(c, "source", &e.Clone, []string{i18n.T(c, "project_local"), i18n.T(c, "project_clone")},
					kit.SegmentedProps{Block: true, Disabled: e.Pending})
			}
			kit.Field(c, "name", i18n.T(c, "project_name"), "", func() {
				ui.Row(c).Gap(6).AlignItems(ui.Center).Children(func() {
					kit.AppearancePicker(c.Key("appearance"), &e.Icon, &e.Color, e.Pending).Size(32, 32).Border(1, p.Input)
					ui.TextInput(c.Key("input"), &e.Name).Placeholder(i18n.T(c, "project_name_placeholder")).Label(i18n.T(c, "project_name")).Grow(1).Disabled(e.Pending)
				})
			})
			kit.Field(c, "host", i18n.T(c, "project_host"), "", func() {
				before := e.host
				b, _ := kit.Select(c.Key("select"), "project-host", i18n.T(c, "project_host"), &e.host, hosts, e.Project != nil || e.Pending, hostIDs)
				b.FillWidth()
				if e.host != before {
					// A path belongs to the host it was chosen on.
					e.Path, e.other = "", ""
					e.suggestName()
				}
			})
			if e.cloning() {
				kit.Field(c, "repo", i18n.T(c, "project_repository"), "", func() {
					ui.TextInput(c.Key("input"), &e.Repo).Placeholder(i18n.T(c, "project_repository_placeholder")).Label(i18n.T(c, "project_repository")).FillWidth().Disabled(e.Pending)
				})
				kit.Field(c, "branch", i18n.T(c, "project_branch"), "", func() {
					ui.TextInput(c.Key("input"), &e.Branch).Placeholder(i18n.T(c, "project_branch_placeholder")).Label(i18n.T(c, "project_branch")).FillWidth().Disabled(e.Pending)
				})
			}
			label := i18n.T(c, "project_path")
			hint := ""
			if e.cloning() {
				label = i18n.T(c, "project_destination")
				if CheckClone(e.Name, e.Repo, e.Path) == "" {
					hint = Destination(e.Path, e.Name)
				}
			}
			kit.Field(c, "path", label, hint, func() {
				ui.Row(c).Gap(6).Children(func() {
					ui.TextInput(c.Key("input"), &e.Path).Placeholder(i18n.T(c, "project_path_placeholder")).Label(label).Grow(1).Disabled(e.Pending)
					kit.IconButton(c.Key("browse"), icons.FolderOpen, i18n.T(c, "directory_title"), 32, false).
						Border(1, p.Input).Disabled(e.Pending).OnClick(func() { s.browse(c, false) })
				})
			})
		})
		ui.Row(c.Key("footer")).Gap(8).Justify(ui.End).Children(func() {
			kit.Secondary(c.Key("cancel"), i18n.T(c, "settings_cancel")).OnClick(func() { e.Open = false })
			saveKey := "settings_save_preview"
			switch {
			case e.cloning():
				saveKey = "project_clone"
			case s.Actions != nil:
				saveKey = "settings_save"
			}
			save := i18n.T(c, saveKey)
			kit.PrimaryBusy(c.Key("save"), save, false, e.Pending).OnClick(func() { s.submit(c) })
		})
	}).Width(560).Label(title)
}

// submit checks the draft and saves it. The preview checks the name, the
// path and its duplicates itself; a Node checks its own, so only a clone's
// destination, which is composed here, is checked before a live save.
func (s *State) submit(c *ui.Context) {
	e := &s.editor
	if e.Pending {
		return
	}
	if e.Project == nil && strings.TrimSpace(e.Name) == "" {
		e.suggestName()
	}
	problem := ""
	switch {
	case e.cloning():
		problem = CheckClone(e.Name, e.Repo, e.Path)
	case s.Actions == nil:
		problem = CheckProject(e.Name, e.Path, e.host, s.Data.Projects, e.Project)
	}
	if problem != "" {
		kit.Toast(c, kit.Failed, i18n.T(c, problem))
		return
	}
	s.saveProject(e.host)
}

// files is the file system of a host: the Node's through the live adapter,
// the sample catalog in the preview.
func (s *State) files(host string) (picker.Source, bool) {
	if s.Actions != nil {
		if s.Actions.Files == nil {
			return picker.Source{}, false
		}
		return s.Actions.Files(host)
	}
	if s.catalogs == nil {
		s.catalogs = map[string]*picker.Catalog{}
	}
	if s.catalogs[host] == nil {
		s.catalogs[host] = picker.NewCatalog(len(s.Data.Hosts) > 0 && s.Data.Hosts[0].ID != host)
	}
	return s.catalogs[host].Source(), true
}

// browse opens the host's directory picker for the draft. From the editor a
// choice fills the path; register (Open folder) opens the project already at
// the chosen directory or saves a new one, keeping the picker open meanwhile.
// The picker goes stale when the draft's host, or the path it started from,
// changes under it.
func (s *State) browse(c *ui.Context, register bool) {
	e := &s.editor
	host, original, generation := e.host, e.Path, s.editorGeneration
	source, ok := s.files(host)
	if !ok {
		kit.Toast(c, kit.Failed, i18n.T(c, "unavailable"))
		return
	}
	current := func() bool {
		return generation == s.editorGeneration && e.host == host && (register || e.Open && e.Path == original)
	}
	o := picker.Options{Mode: picker.Directory, Title: "directory_title", Confirm: "directory_title", Initial: strings.TrimSpace(original),
		Source: source, Projects: s.projectPlaces(host), Preview: s.Actions == nil, Valid: current, Pending: func() bool { return e.Pending },
		Choose: func(paths []string) bool {
			if !current() || len(paths) != 1 {
				return false
			}
			e.Path = paths[0]
			e.suggestName()
			if !register {
				return true
			}
			if s.openRegistered(host, paths[0]) {
				return true
			}
			s.saveProject(host)
			return false
		}}
	for _, h := range s.Data.Hosts {
		if h.ID == host {
			o.Host = s.hostLabel(c, h)
		}
	}
	if register {
		o.Title, o.Confirm = "menu_open_folder", "directory_open"
	}
	s.Picker.Open(o)
}

// OpenFolder chooses a directory of host and opens it as a project: the one
// already registered there, or a new one named after the directory.
func (s *State) OpenFolder(c *ui.Context, host string) {
	s.editorGeneration++
	s.editor = Editor{Icon: "folder", Color: "none", host: host, folder: true}
	if host == "" && len(s.Data.Hosts) > 0 {
		s.editor.host = s.Data.Hosts[0].ID
	}
	s.browse(c, true)
}

// openRegistered shows the project of host at path, if there is one.
func (s *State) openRegistered(host, path string) bool {
	for _, pr := range s.Data.Projects {
		if pr.Host == host && pr.Path == path {
			s.Reveal(pr.ID)
			if s.Actions != nil && s.Actions.Overview != nil {
				s.Actions.Overview(pr)
			}
			return true
		}
	}
	return false
}

// saveProject applies the editor's draft: through the live adapter, which
// keeps the draft on failure, or to the preview in memory.
func (s *State) saveProject(host string) {
	e := &s.editor
	if s.Actions != nil && s.Actions.SaveProject != nil {
		e.Pending = true
		draft := *e
		generation := s.editorGeneration
		s.Actions.SaveProject(draft, host, func(ok bool) {
			if generation != s.editorGeneration {
				return
			}
			e.Pending = false
			if ok {
				e.Open = false
				if e.folder {
					s.Picker.Close()
				}
			}
		})
		return
	}
	name, path := strings.TrimSpace(e.Name), strings.TrimSpace(e.Path)
	if e.cloning() {
		path = Destination(path, name)
	}
	if pr := e.Project; pr != nil {
		pr.Name, pr.Path, pr.Icon, pr.Color = name, path, e.Icon, e.Color
		pr.Worktrees[0].Path = path
	} else {
		s.created++
		branch := strings.TrimSpace(e.Branch)
		if branch == "" {
			branch = "main"
		}
		pr := &preview.Project{
			ID: fmt.Sprintf("project/new-%d", s.created), Name: name, Path: path, Branch: branch, Host: host,
			Icon: e.Icon, Color: e.Color, Worktrees: []preview.Worktree{{Branch: branch, Path: path, Main: true}},
		}
		s.Data.Projects = append(s.Data.Projects, pr)
		s.Reveal(pr.ID)
	}
	e.Open = false
	if e.folder {
		s.Picker.Close()
	}
}
