package plugins

import (
	"bytes"
	"slices"
	"strings"
	"unicode"
	"unicode/utf8"

	"github.com/sailry/sailry-harness/internal/protocol"
	"go.yaml.in/yaml/v3"
)

const MaxSkills = 64
const MaxSkillDocumentBytes = 128 * 1024

func skillName(name string) bool {
	if name == "" || utf8.RuneCountInString(name) > 64 || strings.HasPrefix(name, "-") || strings.HasSuffix(name, "-") || strings.Contains(name, "--") {
		return false
	}
	for _, value := range name {
		if value != '-' && (!unicode.IsLetter(value) && !unicode.IsNumber(value) || unicode.IsUpper(value)) {
			return false
		}
	}
	return true
}

func validateYAML(node *yaml.Node, depth int, count *int) bool {
	*count++
	if depth > 16 || *count > 4096 || node.Kind == yaml.AliasNode {
		return false
	}
	if node.Kind == yaml.MappingNode {
		seen := map[string]bool{}
		for index := 0; index < len(node.Content); index += 2 {
			key := node.Content[index]
			if key.Kind != yaml.ScalarNode || key.Tag != "!!str" || seen[key.Value] {
				return false
			}
			seen[key.Value] = true
		}
	}
	for _, child := range node.Content {
		if !validateYAML(child, depth+1, count) {
			return false
		}
	}
	return true
}

func ParseSkill(data []byte, name string) (protocol.PluginSkill, error) {
	skill := protocol.DefaultPluginSkill()
	if len(data) > MaxSkillDocumentBytes || !utf8.Valid(data) {
		return skill, packageFault("skill document exceeds its UTF-8 size limit")
	}
	text := strings.TrimLeft(strings.TrimPrefix(string(data), "\ufeff"), "\r\n \t")
	lines := strings.Split(strings.ReplaceAll(text, "\r\n", "\n"), "\n")
	if len(lines) == 0 || strings.TrimSpace(lines[0]) != "---" {
		return skill, packageFault("skill document requires YAML frontmatter")
	}
	end := -1
	for index := 1; index < len(lines); index++ {
		if lines[index] == "---" || lines[index] == "..." {
			end = index
			break
		}
	}
	if end < 0 {
		return skill, packageFault("skill frontmatter is unterminated")
	}
	frontmatter := []byte(strings.Join(lines[1:end], "\n"))
	if len(frontmatter) > 16*1024 {
		return skill, packageFault("skill frontmatter exceeds its size limit")
	}
	var document yaml.Node
	count := 0
	if yaml.Unmarshal(frontmatter, &document) != nil || !validateYAML(&document, 0, &count) {
		return skill, packageFault("invalid skill frontmatter")
	}
	var fields map[string]any
	if document.Decode(&fields) != nil || fields == nil {
		return skill, packageFault("skill frontmatter must be an object")
	}
	declared, ok := fields["name"].(string)
	if !ok || !skillName(declared) || name != "" && name != declared {
		return skill, packageFault("skill name does not match its directory")
	}
	description, ok := fields["description"].(string)
	if !ok || strings.TrimSpace(description) == "" || utf8.RuneCountInString(description) > 1024 || strings.ContainsRune(description, 0) {
		return skill, packageFault("invalid skill description")
	}
	skill.Name, skill.Description = declared, description
	skill.Path = "SKILL.md"
	for key, target := range map[string]**string{"license": &skill.License, "compatibility": &skill.Compatibility} {
		if raw, exists := fields[key]; exists {
			value, ok := raw.(string)
			if !ok || strings.ContainsRune(value, 0) || key == "compatibility" && (value == "" || utf8.RuneCountInString(value) > 500) {
				return skill, packageFault("invalid skill metadata")
			}
			*target = &value
		}
	}
	if raw, exists := fields["allowed-tools"]; exists {
		switch value := raw.(type) {
		case string:
			if strings.ContainsRune(value, 0) {
				return skill, packageFault("invalid skill allowed tools")
			}
		case []any:
			for _, item := range value {
				if text, ok := item.(string); !ok || strings.ContainsRune(text, 0) {
					return skill, packageFault("invalid skill allowed tools")
				}
			}
		default:
			return skill, packageFault("invalid skill allowed tools")
		}
	}
	if raw, exists := fields["metadata"]; exists {
		metadata, ok := raw.(map[string]any)
		if !ok {
			return skill, packageFault("skill metadata must contain string values")
		}
		for key, value := range metadata {
			text, ok := value.(string)
			if !ok {
				return skill, packageFault("skill metadata must contain string values")
			}
			if display := strings.TrimSpace(text); key == "display-name" && display != "" && len(display) <= 256 {
				skill.DisplayName = &display
			}
			if locale, localized := strings.CutPrefix(key, "display-name."); localized {
				// A locale is ASCII subtags joined by hyphens, as every reader of
				// these names expects.
				tagged := !slices.ContainsFunc(strings.Split(locale, "-"), func(part string) bool {
					return part == "" || strings.ContainsFunc(part, func(r rune) bool {
						return r > unicode.MaxASCII || !unicode.IsLetter(r) && !unicode.IsDigit(r)
					})
				})
				if !tagged || strings.TrimSpace(text) == "" || strings.ContainsFunc(text, unicode.IsControl) {
					return skill, packageFault("invalid localized skill display name")
				}
				skill.DisplayLocales[locale] = text
			}
		}
	}
	return skill, nil
}

func DiscoverSkills(files map[string][]byte) ([]protocol.PluginSkill, []protocol.PluginIssue) {
	names := []string{}
	for name := range files {
		if strings.HasPrefix(name, "skills/") && strings.HasSuffix(name, "/SKILL.md") && strings.Count(name, "/") == 2 {
			names = append(names, name)
		}
	}
	slices.Sort(names)
	skills := []protocol.PluginSkill{}
	issues := []protocol.PluginIssue{}
	if len(names) > MaxSkills {
		return skills, []protocol.PluginIssue{{Path: "skills", Kind: protocol.PluginIssueKindInvalidSkills}}
	}
	for _, name := range names {
		parts := strings.Split(name, "/")
		skill, err := ParseSkill(files[name], parts[1])
		if err != nil {
			issues = append(issues, protocol.PluginIssue{Path: name, Kind: protocol.PluginIssueKindInvalidSkill})
			continue
		}
		skill.Path = name
		if data, exists := files["skills/"+skill.Name+"/agents/openai.yaml"]; exists && len(data) <= 16*1024 && utf8.Valid(data) {
			var document yaml.Node
			count := 0
			if yaml.Unmarshal(data, &document) == nil && validateYAML(&document, 0, &count) {
				var metadata struct {
					Interface struct {
						DisplayName *string `yaml:"display_name"`
					} `yaml:"interface"`
				}
				if document.Decode(&metadata) == nil && metadata.Interface.DisplayName != nil && strings.TrimSpace(*metadata.Interface.DisplayName) != "" {
					skill.DisplayName = metadata.Interface.DisplayName
				}
			}
		}
		skills = append(skills, skill)
	}
	return skills, issues
}

func SkillBody(data []byte) ([]byte, error) {
	if _, err := ParseSkill(data, ""); err != nil {
		return nil, err
	}
	data = bytes.TrimLeft(bytes.TrimPrefix(data, []byte("\xef\xbb\xbf")), "\r\n \t")
	lines := bytes.Split(data, []byte("\n"))
	for index := 1; index < len(lines); index++ {
		line := bytes.TrimSuffix(lines[index], []byte("\r"))
		if bytes.Equal(line, []byte("---")) || bytes.Equal(line, []byte("...")) {
			return bytes.TrimSpace(bytes.Join(lines[index+1:], []byte("\n"))), nil
		}
	}
	return nil, packageFault("skill frontmatter is unterminated")
}
