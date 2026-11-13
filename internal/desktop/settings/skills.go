package settings

import (
	"slices"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/icons"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
	"github.com/sailry/sailry-harness/internal/desktop/theme"
)

// The Skills page (settings/skills): the search, Check updates and
// Install skill over the installed skills as resource cards; a skill
// from a plugin is managed with its plugin. The install dialog takes a
// Git repository, a skills.sh or skillhub.cn URL, a directory or an
// archive (skill-sources).

// skillSources are the install dialog's sources.
var skillSources = []string{"skill_source_git", "skill_source_skills", "skill_source_hub", "skill_source_directory", "skill_source_archive"}

// skillInstall is the source and its fields.
type skillInstall struct {
	open                        bool
	pending                     bool
	generation                  uint64
	source                      int
	repository, rev, path, link string
}

// Check validates the source: the i18n key of the problem, or "".
func (e *skillInstall) Check() string {
	switch e.source {
	case 0:
		if !strings.HasPrefix(e.repository, "https://") {
			return "skills_invalid"
		}
	case 1, 2:
		host := map[int]string{1: "https://skills.sh/", 2: "https://skillhub.cn/skills/"}[e.source]
		if !strings.HasPrefix(e.link, host) {
			return "skill_source_link_invalid"
		}
	}
	return ""
}

func (m *Model) skill(id string) *preview.Skill {
	for _, list := range [][]preview.Skill{m.SkillList, m.MarketSkills} {
		for i := range list {
			if list[i].ID == id {
				return &list[i]
			}
		}
	}
	return nil
}

func skillSource(c *ui.Context, s *preview.Skill) string {
	if s.Package != "" {
		return i18n.T(c, "skills_package_source", "name", s.Package)
	}
	return s.Origin
}

func skills(c *ui.Context, s *State) {
	m := s.Model
	t := func(k string, a ...any) string { return i18n.T(c, k, a...) }
	ui.Column(c.Key("skills")).Gap(16).Children(func() {
		ui.Row(c.Key("toolbar")).Gap(8).Children(func() {
			ui.SearchField(c.Key("search"), &m.SkillQuery).Placeholder(t("skills_search")).Label(t("skills_search")).Grow(1)
			kit.SecondaryBusy(c.Key("check"), t("plugins_check_updates"), m.Busy[BusySkillUpdates]).OnClick(func() {
				if m.PackageActions != nil && m.PackageActions.CheckUpdates != nil {
					m.PackageActions.CheckUpdates(preview.KindSkill)
					return
				}
				notice(c)
			})
			iconButton(c, "install", icons.Plus, t("skills_install"), true, false).OnClick(func() { m.skillAdd = skillInstall{open: true, generation: m.skillAdd.generation + 1} })
		})
		q := strings.ToLower(strings.TrimSpace(m.SkillQuery))
		var shown []int
		for i, x := range m.SkillList {
			if q == "" || strings.Contains(strings.ToLower(x.Name+" "+x.Description), q) {
				shown = append(shown, i)
			}
		}
		if len(shown) == 0 {
			kit.Result(c, kit.ResultProps{Icon: icons.Bot, Title: t("skills_no_matches"), Card: true})
			return
		}
		kit.Grid(c, func() {
			for _, i := range shown {
				skillCard(c, s, &m.SkillList[i])
			}
		})
	})
	skillInstallDialog(c, m)
	if sk := m.skill(m.skillRemoving); sk != nil {
		open := true
		if confirm(c, "uninstall", &open, t("plugins_uninstall"), t("skills_remove_effect", "name", sk.Name), t("plugins_uninstall")) {
			if m.PackageActions != nil && m.PackageActions.Uninstall != nil {
				m.PackageActions.Uninstall(sk.ID, func(bool) {})
				m.skillRemoving = ""
				return
			}
			m.SkillList = slices.DeleteFunc(m.SkillList, func(x preview.Skill) bool { return x.ID == sk.ID })
		}
		if !open {
			m.skillRemoving = ""
		}
	}
}

// skillCard is the skill, where it came from, its update, switch and menu.
func skillCard(c *ui.Context, s *State, sk *preview.Skill) {
	m := s.Model
	t := func(k string, a ...any) string { return i18n.T(c, k, a...) }
	desc := sk.Description
	if sk.Package != "" {
		desc = skillSource(c, sk)
	}
	busy := m.Busy[BusyPackage+sk.ID]
	kit.ResourceCard(c, sk.ID, sk.Name, desc, func() { kit.Tile(c, sk.Name, icons.Bot) }, func() { s.ShowDetails(DetailSkill, sk.ID) }, func(bool) {
		if busy {
			// Its update, import, switch or removal runs.
			kit.BusyAction(c.Key("busy"), sk.Name)
		} else if sk.Update {
			kit.Action(c.Key("update"), icons.Download, t("settings_update"), false).OnClick(func() {
				if m.PackageActions != nil && m.PackageActions.Update != nil {
					m.PackageActions.Update([]string{sk.ID})
					return
				}
				notice(c)
			})
		}
		if ui.Switch(c.Key("enabled"), &sk.Enabled).Label(t("plugins_enabled")+" "+sk.Name).Disabled(sk.Package != "" || busy).Changed() && m.PackageActions != nil && m.PackageActions.Toggle != nil {
			m.PackageActions.Toggle(sk.ID, sk.Enabled)
		}
		kit.MenuButton(c.Key("menu"), "skill-menu-"+sk.ID, func(b ui.Element) {
			kit.IconLook(c, b, icons.Ellipsis, t("more"), "", 28, false)
		}, func(menu *Menu) {
			if menu.Item(t("plugins_information")).Icon(icons.Info).Chosen() {
				s.ShowDetails(DetailSkill, sk.ID)
			}
			if sk.Package != "" {
				if menu.Item(t("skills_manage_package")).Icon(icons.Puzzle).Chosen() {
					s.Open(Plugins)
				}
				return
			}
			if menu.Item(t("skill_source_reimport")).Icon(icons.RotateCw).Disabled(busy).Chosen() {
				if m.PackageActions != nil && m.PackageActions.ReimportSkill != nil {
					m.PackageActions.ReimportSkill(sk.ID)
				} else {
					notice(c)
				}
			}
			menu.Separator()
			if menu.Item(t("plugins_uninstall")).Icon(icons.Trash).Danger().Disabled(busy).Chosen() {
				m.skillRemoving = sk.ID
			}
		}, kit.MenuOptions{At: ui.AnchorBottomEnd, Self: ui.AnchorTopEnd})
	})
}

// skillInstallDialog is the source as segments, then its fields; the preview
// fetches nothing.
func skillInstallDialog(c *ui.Context, m *Model) {
	e := &m.skillAdd
	if !e.open {
		return
	}
	t := func(k string, a ...any) string { return i18n.T(c, k, a...) }
	title := t("skills_install")
	input := func(key, label, hint string, v *string) {
		kit.Field(c, key, t(label), "", func() {
			ui.TextInput(c.Key("input"), v).Label(t(label)).Placeholder(t(hint)).FillWidth().Disabled(e.pending)
		})
	}
	kit.Dialog(c.Key("skill-install"), &e.open, func() {
		ui.Text(c, title).FontSize(16).FontWeight(600)
		kit.Segmented(c, "source", &e.source, labels(c, skillSources), kit.SegmentedProps{Block: true}).Label(t("skill_source_origin")).Disabled(e.pending)
		switch e.source {
		case 0:
			ui.Text(c, t("skill_source_git_help")).FontSize(12).TextColor(theme.Of(c).MutedForeground)
			input("repository", "skills_repository", "skills_repository_hint", &e.repository)
			input("rev", "skills_revision", "skills_revision_hint", &e.rev)
			input("path", "skills_subdirectory", "skills_path_hint", &e.path)
		case 1:
			ui.Text(c, t("skill_source_skills_help")).FontSize(12).TextColor(theme.Of(c).MutedForeground)
			input("link", "skill_source_link", "skill_source_skills_hint", &e.link)
		case 2:
			ui.Text(c, t("skill_source_hub_help")).FontSize(12).TextColor(theme.Of(c).MutedForeground)
			input("link", "skill_source_link", "skill_source_hub_hint", &e.link)
		default:
			key := "skill_source_choose_directory"
			if e.source == 4 {
				key = "skill_source_choose_archive"
			}
			kit.Secondary(c.Key("choose"), t(key)).AlignSelf(ui.Start).Disabled(e.pending).OnClick(func() {
				if m.PackageActions != nil && m.PackageActions.ChooseSkill != nil {
					m.PackageActions.ChooseSkill(e.source, func(path string) { e.path = path })
					return
				}
				notice(c)
			})
		}
		footer(c, t("plugins_install"), func() {
			if m.PackageActions != nil && m.PackageActions.CancelSkill != nil {
				m.PackageActions.CancelSkill()
			}
			e.open = false
		}, func() {
			if e.pending {
				return
			}
			if problem := e.Check(); problem != "" {
				kit.Toast(c, kit.Failed, t(problem))
				return
			}
			if m.PackageActions != nil && m.PackageActions.InstallSkill != nil {
				e.pending = true
				generation := e.generation
				m.PackageActions.InstallSkill(SkillSourceDraft{Source: e.source, Repository: e.repository, Revision: e.rev, Path: e.path, Link: e.link}, func(ok bool) {
					if e.generation != generation {
						return
					}
					e.pending = false
					if ok {
						e.open = false
					}
				})
				return
			}
			e.open = false
			notice(c)
		}, e.pending)
	}).Width(min(560, width(c)-48)).Label(title)
	if !e.open && e.pending && m.PackageActions != nil && m.PackageActions.CancelSkill != nil {
		m.PackageActions.CancelSkill()
	}
}
