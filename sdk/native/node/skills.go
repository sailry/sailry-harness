package node

import (
	"io/fs"
	"path"

	"github.com/sailry/sailry-harness/internal/node/moduleapi"
	"github.com/sailry/sailry-harness/internal/node/plugins"
)

type BuiltInSkill = moduleapi.BuiltInSkill

type SkillProvider = moduleapi.SkillProvider

// LoadBuiltInSkill parses the skill document in a native module's embedded tree.
func LoadBuiltInSkill(tree fs.FS, pkg, name string) (BuiltInSkill, error) {
	file := path.Join("skills", name, "SKILL.md")
	document, err := fs.ReadFile(tree, file)
	if err != nil {
		return BuiltInSkill{}, err
	}
	skill, err := plugins.ParseSkill(document, name)
	if err != nil {
		return BuiltInSkill{}, err
	}
	skill.Path = file
	return BuiltInSkill{Package: pkg, Skill: skill, Tree: tree}, nil
}
