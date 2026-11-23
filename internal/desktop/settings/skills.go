package settings

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
)

// SkillPageState discards dialogs that belonged to the previous package Node.
type SkillPageState interface {
	CloseDialogs()
}

// PackageTargetOffline reports whether the selected Node can serve its packages.
func (m *Model) PackageTargetOffline() bool { return m.packageTargetOffline() }

// Skill looks up a projected installed or market skill.
func (m *Model) Skill(id string) *preview.Skill {
	for _, list := range [][]preview.Skill{m.SkillList, m.MarketSkills} {
		for i := range list {
			if list[i].ID == id {
				return &list[i]
			}
		}
	}
	return nil
}

func (m *Model) SetSkillContent(id, content string) {
	for _, list := range [][]preview.Skill{m.SkillList, m.MarketSkills} {
		for i := range list {
			if list[i].ID == id {
				list[i].Content = content
				break
			}
		}
	}
}

// SkillSource labels an installed or market skill by its origin.
func SkillSource(c *ui.Context, s *preview.Skill) string {
	if s.BuiltIn {
		return i18n.T(c, "market_details_source_official")
	}
	if s.Package != "" {
		return i18n.T(c, "skills_package_source", "name", s.Package)
	}
	return s.Origin
}
