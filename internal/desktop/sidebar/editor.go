package sidebar

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

// Editor is the project editor's draft: a new project, local or cloned, or an
// existing one's name and path.
type Editor struct {
	Open    bool
	Project *preview.Project // nil for a new project
	Clone   int              // 0 local, 1 clone (new projects only)
	Name    string
	Host    string // the host's name as the select shows it
	Path    string
	Repo    string
	Branch  string
	suggest string // the name last suggested from the path or repository
}

// Editing reports whether the project editor shows.
func (s *State) Editing() bool { return s.editor.Open }

// OpenEditor opens the project editor for pr, or for a new project.
func (s *State) OpenEditor(pr *preview.Project) {
	s.editor = Editor{Open: true, Project: pr}
	if pr != nil {
		s.editor.Name, s.editor.Path = pr.Name, pr.Path
	}
}

// SuggestName is the name a path or a repository suggests: its last segment,
// split on / \ and:, without ".git".
func SuggestName(path string) string {
	path = strings.TrimRight(path, `/\:`)
	if i := strings.LastIndexAny(path, `/\:`); i >= 0 {
		path = path[i+1:]
	}
	return strings.TrimSuffix(path, ".git")
}

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

// editorDialog is the project editor: on a new project a Local | Clone
// switch, then the name, the host, the repository and branch of a clone,
// and the folder. Saving changes the preview in memory only.
func (s *State) editorDialog(c *ui.Context) {
	e := &s.editor
	if !e.Open {
		return
	}
	p := theme.Of(c)
	hosts := make([]string, len(s.Data.Hosts))
	for i, h := range s.Data.Hosts {
		hosts[i] = i18n.T(c, h.Name)
	}
	if e.Host == "" && len(hosts) > 0 {
		e.Host = hosts[0]
	}
	hostID := ""
	for i, h := range s.Data.Hosts {
		if hosts[i] == e.Host {
			hostID = h.ID
		}
	}
	if e.Project != nil {
		hostID = e.Project.Host
	}
	// The name follows the path or the repository until it is typed over.
	source := e.Path
	if e.Clone == 1 {
		source = e.Repo
	}
	if sug := SuggestName(source); e.Name == "" || e.Name == e.suggest {
		e.Name, e.suggest = sug, sug
	}
	title := i18n.T(c, "project_add")
	if e.Project != nil {
		title = i18n.T(c, "project_edit")
	}
	kit.Dialog(c.Key("editor"), &e.Open, func() {
		ui.Column(c.Key("head")).Gap(6).Children(func() {
			ui.Text(c, title).FontSize(16).FontWeight(600)
			ui.Text(c, i18n.T(c, "project_editor_preview")).FontSize(14).TextColor(p.MutedForeground)
		})
		ui.Scroll(c.Key("body")).Grow(1).Shrink(1).Gap(12).Children(func() {
			if e.Project == nil {
				kit.Segmented(c, "source", &e.Clone, []string{i18n.T(c, "project_local"), i18n.T(c, "project_clone")}, kit.SegmentedProps{Block: true})
			}
			kit.Field(c, "name", i18n.T(c, "project_name"), "", func() {
				ui.TextInput(c.Key("input"), &e.Name).Placeholder(i18n.T(c, "project_name_placeholder")).Label(i18n.T(c, "project_name")).FillWidth()
			})
			kit.Field(c, "host", i18n.T(c, "project_host"), "", func() {
				b, _ := kit.Select(c.Key("select"), "project-host", i18n.T(c, "project_host"), &e.Host, hosts, e.Project != nil)
				b.FillWidth()
			})
			if e.Clone == 1 && e.Project == nil {
				kit.Field(c, "repo", i18n.T(c, "project_repository"), "", func() {
					ui.TextInput(c.Key("input"), &e.Repo).Placeholder(i18n.T(c, "project_repository_placeholder")).Label(i18n.T(c, "project_repository")).FillWidth()
				})
				kit.Field(c, "branch", i18n.T(c, "project_branch"), "", func() {
					ui.TextInput(c.Key("input"), &e.Branch).Placeholder(i18n.T(c, "project_branch_placeholder")).Label(i18n.T(c, "project_branch")).FillWidth()
				})
			}
			label := i18n.T(c, "project_path")
			hint := ""
			if e.Clone == 1 && e.Project == nil {
				label = i18n.T(c, "project_destination")
				if e.Path != "" && e.Name != "" {
					hint = Destination(e.Path, e.Name)
				}
			}
			kit.Field(c, "path", label, hint, func() {
				ui.Row(c).Gap(6).Children(func() {
					ui.TextInput(c.Key("input"), &e.Path).Placeholder(i18n.T(c, "project_path_placeholder")).Label(label).Grow(1)
					// The folder picker would read the disk.
					kit.IconButton(c.Key("browse"), icons.FolderOpen, i18n.T(c, "directory_title"), 32, false).
						Border(1, p.Input).OnClick(func() { notice(c) })
				})
			})
		})
		ui.Row(c.Key("footer")).Gap(8).Justify(ui.End).Children(func() {
			kit.Secondary(c.Key("cancel"), i18n.T(c, "settings_cancel")).OnClick(func() { e.Open = false })
			save := i18n.T(c, "settings_save_preview")
			kit.Primary(c.Key("save"), save, false).Children(func() { ui.Text(c, save).FontSize(14) }).OnClick(func() {
				problem := CheckProject(e.Name, e.Path, hostID, s.Data.Projects, e.Project)
				if e.Clone == 1 && e.Project == nil {
					problem = CheckClone(e.Name, e.Repo, e.Path)
				}
				if problem != "" {
					kit.Toast(c, kit.Failed, i18n.T(c, problem))
					return
				}
				s.saveProject(hostID)
			})
		})
	}).Width(560).Label(title)
}

// saveProject applies the editor's draft to the preview, in memory.
func (s *State) saveProject(host string) {
	e := &s.editor
	name, path := strings.TrimSpace(e.Name), strings.TrimSpace(e.Path)
	if e.Clone == 1 && e.Project == nil {
		path = Destination(path, name)
	}
	if pr := e.Project; pr != nil {
		pr.Name, pr.Path = name, path
		pr.Worktrees[0].Path = path
	} else {
		s.created++
		branch := strings.TrimSpace(e.Branch)
		if branch == "" {
			branch = "main"
		}
		s.Data.Projects = append(s.Data.Projects, &preview.Project{
			ID: fmt.Sprintf("project/new-%d", s.created), Name: name, Path: path, Branch: branch, Host: host,
			Worktrees: []preview.Worktree{{Branch: branch, Path: path, Main: true}},
		})
	}
	e.Open = false
}
