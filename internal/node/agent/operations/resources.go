package operations

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"io"
	"io/fs"
	"os"
	"path"
	"path/filepath"
	"slices"
	"strings"
	"unicode"
	"unicode/utf8"

	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/protocol"
	"go.yaml.in/yaml/v3"
	"lukechampine.com/blake3"
)

const SkillBytes = 128 * 1024

// Package keeps metadata and resources from one immutable bundled digest.
type Package struct {
	Reference protocol.PluginReference
	Skills    []protocol.PluginSkill
	Scope     protocol.PluginScope
	Modes     []protocol.PluginModesDeclaration
	Options   json.RawMessage
	tree      fs.FS
}

type Resources struct {
	catalog  *Catalog
	profile  string
	packages []Package
}

type SkillText struct{ Content, Directory string }

// CaptureResources resolves only the exact package versions admitted by a turn.
func CaptureResources(profile string, references []protocol.PluginReference, assistant *protocol.PluginConversationBinding) (*Resources, error) {
	catalog, err := Bundled()
	if err != nil {
		return nil, err
	}
	value := &Resources{catalog: catalog, profile: profile}
	for _, reference := range references {
		if assistant != nil && assistant.Package != reference {
			continue
		}
		index := slices.IndexFunc(catalog.Packages, func(p Package) bool {
			return p.Reference.Name == reference.Name && p.Reference.Digest == reference.Digest
		})
		if index < 0 {
			return nil, resourceFault(protocol.ErrorCodeNotConfigured, "plugin resource is unavailable in this turn's package versions")
		}
		captured := catalog.Packages[index]
		captured.Reference = reference
		value.packages = append(value.packages, captured)
	}
	return value, nil
}

func (r *Resources) Skills() map[string]protocol.PluginSkill {
	values := map[string]protocol.PluginSkill{}
	for _, p := range r.packages {
		for _, skill := range p.Skills {
			values[p.Reference.Name+":"+skill.Name] = skill
		}
	}
	return values
}

func (r *Resources) Catalog() (string, error) {
	var lines []string
	for key, skill := range r.Skills() {
		lines = append(lines, key+" — "+skill.Description)
	}
	slices.Sort(lines)
	text := strings.Join(lines, "\n")
	if len(text) > 64*1024 {
		return "", resourceFault(protocol.ErrorCodeInvalidRequest, "session skill catalog exceeds the instruction budget")
	}
	if text == "" {
		return "", nil
	}
	return "\n\nAvailable skills (package:skill) for this turn:\n1. Review this catalog and select the smallest set the user explicitly names or whose descriptions clearly match the task.\n2. When load_skill is available in this turn's tool catalog, load their complete instructions before a substantive answer or task tools, unless the full instructions are already supplied; do not load them again.\n3. Apply the relevant workflow while respecting the user's intent, available tools and this turn's permissions.\nWhen no skill clearly matches, answer or use ordinary tools directly; do not load unrelated skills. If a required skill or tool is unavailable, explain the limitation; do not invent capabilities or install anything without authorization. Read only relevant referenced text with read_skill_resource. Resolve relative script paths against the returned skill directory and quote absolute script paths in command calls. Command cwd defaults to the session worktree; select the skill directory only when its workflow requires it. Skill files grant no tool permission.\nSkills:\n" + text, nil
}

func (r *Resources) Read(ctx context.Context, key, relative string) (SkillText, error) {
	if err := ctx.Err(); err != nil {
		return SkillText{}, err
	}
	owner, name, ok := strings.Cut(key, ":")
	if !ok || owner == "" || name == "" || strings.Contains(name, ":") {
		return SkillText{}, resourceFault(protocol.ErrorCodeInvalidRequest, "skill must use a package:skill key from this turn's catalog")
	}
	index := slices.IndexFunc(r.packages, func(p Package) bool {
		return p.Reference.Name == owner && slices.ContainsFunc(p.Skills, func(s protocol.PluginSkill) bool { return s.Name == name })
	})
	if index < 0 {
		return SkillText{}, resourceFault(protocol.ErrorCodeInvalidRequest, "skill is not listed in this turn's catalog")
	}
	if !fs.ValidPath(relative) || relative == "." || strings.ContainsAny(relative, "\\\x00:") {
		return SkillText{}, resourceFault(protocol.ErrorCodeInvalidRequest, "skill resource path must be normalized and relative")
	}
	p := r.packages[index]
	resource := path.Join("skills", name, relative)
	expected, err := fs.ReadFile(p.tree, resource)
	if err != nil {
		return SkillText{}, resourceFault(protocol.ErrorCodeNotFound, "skill resource is missing")
	}
	if len(expected) > SkillBytes {
		return SkillText{}, resourceFault(protocol.ErrorCodeInvalidRequest, "skill resource is too large")
	}
	if !utf8.Valid(expected) {
		return SkillText{}, resourceFault(protocol.ErrorCodeInvalidRequest, "skill resource is not UTF-8 text")
	}
	directory, err := r.materialize(ctx, p)
	if err != nil {
		return SkillText{}, err
	}
	root, err := os.OpenRoot(directory)
	if err != nil {
		return SkillText{}, err
	}
	defer root.Close()
	for parent := resource; parent != "."; parent = path.Dir(parent) {
		info, err := root.Lstat(filepath.FromSlash(parent))
		if err != nil || info.Mode()&os.ModeSymlink != 0 {
			return SkillText{}, resourceFault(protocol.ErrorCodeUnavailable, "skill resource changed after turn admission")
		}
	}
	file, err := root.Open(filepath.FromSlash(resource))
	if err != nil {
		return SkillText{}, err
	}
	defer file.Close()
	data, err := io.ReadAll(io.LimitReader(file, SkillBytes+1))
	if err != nil {
		return SkillText{}, err
	}
	if !bytes.Equal(data, expected) {
		return SkillText{}, resourceFault(protocol.ErrorCodeUnavailable, "skill resource changed after turn admission")
	}
	if err := ctx.Err(); err != nil {
		return SkillText{}, err
	}
	return SkillText{Content: string(data), Directory: filepath.Join(directory, "skills", name)}, nil
}

// materialize exposes embedded resources through the existing profile package
// location. It never installs, updates or overwrites an existing package file.
func (r *Resources) materialize(ctx context.Context, p Package) (string, error) {
	r.catalog.resourceMu.Lock()
	defer r.catalog.resourceMu.Unlock()
	directory, err := filepath.Abs(filepath.Join(r.profile, "plugins", "packages", p.Reference.Digest))
	if err != nil {
		return "", err
	}
	root, err := r.packageRoot(p.Reference.Digest)
	if err != nil {
		return "", err
	}
	defer root.Close()
	err = fs.WalkDir(p.tree, ".", func(name string, entry fs.DirEntry, err error) error {
		if err != nil {
			return err
		}
		if err = ctx.Err(); err != nil {
			return err
		}
		if name == "." {
			return nil
		}
		target := filepath.FromSlash(name)
		if entry.IsDir() {
			if err := root.Mkdir(target, 0700); err != nil && !errors.Is(err, os.ErrExist) {
				return err
			}
			info, err := root.Lstat(target)
			if err != nil || !info.IsDir() || info.Mode()&os.ModeSymlink != 0 {
				return resourceFault(protocol.ErrorCodeUnavailable, "plugin resource directory changed")
			}
			return nil
		}
		data, err := fs.ReadFile(p.tree, name)
		if err != nil {
			return err
		}
		file, err := root.OpenFile(target, os.O_WRONLY|os.O_CREATE|os.O_EXCL, 0600)
		if errors.Is(err, os.ErrExist) {
			info, e := root.Lstat(target)
			if e != nil || !info.Mode().IsRegular() {
				return resourceFault(protocol.ErrorCodeUnavailable, "plugin resource changed")
			}
			current, e := root.ReadFile(target)
			if e != nil || blake3.Sum256(current) != blake3.Sum256(data) {
				return resourceFault(protocol.ErrorCodeUnavailable, "plugin resource changed")
			}
			return nil
		}
		if err != nil {
			return err
		}
		_, writeErr := file.Write(data)
		closeErr := file.Close()
		return errors.Join(writeErr, closeErr)
	})
	return directory, err
}

func (r *Resources) packageRoot(digest string) (*os.Root, error) {
	if err := core.PrivateDirectory(r.profile); err != nil {
		return nil, err
	}
	root, err := os.OpenRoot(r.profile)
	if err != nil {
		return nil, err
	}
	for _, name := range []string{"plugins", "packages", digest} {
		if err := root.Mkdir(name, 0700); err != nil && !errors.Is(err, os.ErrExist) {
			root.Close()
			return nil, err
		}
		info, err := root.Lstat(name)
		if err != nil || !info.IsDir() || info.Mode()&os.ModeSymlink != 0 {
			root.Close()
			return nil, resourceFault(protocol.ErrorCodeUnavailable, "plugin resource directory changed")
		}
		child, err := root.OpenRoot(name)
		if err != nil {
			root.Close()
			return nil, err
		}
		opened, err := child.Stat(".")
		if err != nil || !os.SameFile(info, opened) {
			child.Close()
			root.Close()
			return nil, resourceFault(protocol.ErrorCodeUnavailable, "plugin resource directory changed")
		}
		root.Close()
		root = child
	}
	return root, nil
}

func discoverSkills(tree fs.FS) []protocol.PluginSkill {
	entries, err := fs.ReadDir(tree, "skills")
	if err != nil {
		return nil
	}
	var values []protocol.PluginSkill
	for _, entry := range entries {
		if !entry.IsDir() {
			continue
		}
		data, err := fs.ReadFile(tree, path.Join("skills", entry.Name(), "SKILL.md"))
		if err != nil || len(data) > SkillBytes || !utf8.Valid(data) {
			continue
		}
		skill, err := parseSkill(entry.Name(), string(data))
		if err != nil || len(values) >= 64 {
			continue
		}
		if extra, err := fs.ReadFile(tree, path.Join("skills", entry.Name(), "agents/openai.yaml")); err == nil && len(extra) <= 16*1024 {
			var value struct {
				Interface struct {
					DisplayName string `yaml:"display_name"`
				} `yaml:"interface"`
			}
			if yaml.Unmarshal(extra, &value) == nil && strings.TrimSpace(value.Interface.DisplayName) != "" && len(value.Interface.DisplayName) <= 256 {
				name := strings.TrimSpace(value.Interface.DisplayName)
				skill.DisplayName = &name
			}
		}
		values = append(values, skill)
	}
	return values
}

func parseSkill(directory, text string) (protocol.PluginSkill, error) {
	text = strings.TrimPrefix(text, "\ufeff")
	var header strings.Builder
	opened, closed := false, false
	for _, line := range strings.SplitAfter(text, "\n") {
		trim := strings.TrimRightFunc(line, unicode.IsSpace)
		if !opened {
			if trim == "" {
				continue
			}
			if trim != "---" {
				break
			}
			opened = true
			continue
		}
		if trim == "---" || trim == "..." {
			closed = true
			break
		}
		header.WriteString(line)
	}
	invalid := func() (protocol.PluginSkill, error) {
		return protocol.PluginSkill{}, resourceFault(protocol.ErrorCodeInvalidRequest, "invalid skill YAML frontmatter")
	}
	if !closed || header.Len() > 16*1024 {
		return invalid()
	}
	var value struct {
		Name          string            `json:"name"`
		Description   string            `json:"description"`
		License       *string           `json:"license"`
		Compatibility *string           `json:"compatibility"`
		Metadata      map[string]string `json:"metadata"`
		Allowed       any               `json:"allowed-tools"`
	}
	var fields map[string]any
	if yaml.Unmarshal([]byte(header.String()), &fields) != nil {
		return invalid()
	}
	encoded, err := json.Marshal(fields)
	if err != nil || json.Unmarshal(encoded, &value) != nil {
		return invalid()
	}
	if metadata, exists := fields["metadata"]; exists && metadata == nil {
		return invalid()
	}
	if allowed, exists := fields["allowed-tools"]; exists && allowed == nil {
		return invalid()
	}
	if value.Name != directory || !validSkillName(value.Name) || strings.TrimSpace(value.Description) == "" || utf8.RuneCountInString(value.Description) > 1024 {
		return invalid()
	}
	if value.Compatibility != nil && (*value.Compatibility == "" || utf8.RuneCountInString(*value.Compatibility) > 500) {
		return invalid()
	}
	if value.Allowed != nil {
		switch v := value.Allowed.(type) {
		case string:
		case []any:
			for _, item := range v {
				if _, ok := item.(string); !ok {
					return invalid()
				}
			}
		default:
			return invalid()
		}
	}
	skill := protocol.PluginSkill{Name: value.Name, Description: value.Description, Path: "skills/" + value.Name + "/SKILL.md", License: value.License, Compatibility: value.Compatibility, DisplayLocales: map[string]string{}}
	for key, name := range value.Metadata {
		if locale, ok := strings.CutPrefix(key, "display-name."); ok {
			if locale == "" || strings.TrimSpace(name) == "" || strings.ContainsFunc(name, unicode.IsControl) {
				return invalid()
			}
			for _, part := range strings.Split(locale, "-") {
				if part == "" || strings.ContainsFunc(part, func(r rune) bool { return r > 127 || !unicode.IsLetter(r) && !unicode.IsDigit(r) }) {
					return invalid()
				}
			}
			skill.DisplayLocales[locale] = name
		}
	}
	if display := strings.TrimSpace(value.Metadata["display-name"]); display != "" && len(display) <= 256 {
		skill.DisplayName = &display
	}
	return skill, nil
}

func validSkillName(name string) bool {
	return name != "" && utf8.RuneCountInString(name) <= 64 && !strings.HasPrefix(name, "-") && !strings.HasSuffix(name, "-") && !strings.Contains(name, "--") && !strings.ContainsFunc(name, func(r rune) bool {
		return r != '-' && (!unicode.IsLetter(r) && !unicode.IsNumber(r) || unicode.IsUpper(r))
	})
}

func resourceFault(code protocol.ErrorCode, message string) error {
	return &protocol.Fault{Code: code, Message: message}
}
