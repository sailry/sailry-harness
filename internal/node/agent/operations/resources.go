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
	"unicode/utf8"

	"github.com/sailry/sailry-harness/internal/core"
	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/node/python"
	"github.com/sailry/sailry-harness/internal/protocol"
)

const SkillBytes = 128 * 1024

// Package keeps metadata and resources from one immutable captured package.
type Package struct {
	Reference    protocol.PluginReference
	Requirements *protocol.PluginRequirements
	Skills       []protocol.PluginSkill
	Scope        protocol.PluginScope
	Modes        []protocol.PluginModesDeclaration
	Options      json.RawMessage
	standalone   bool
	native       bool
	tree         fs.FS
	directory    string
	read         func(context.Context, string, int) ([]byte, error)
}

// NativeSkillPackage describes a compile-time module skill tree embedded in the
// binary (with files laid out under skills/<name>/SKILL.md inside Tree).
type NativeSkillPackage struct {
	Package string
	Skills  []protocol.PluginSkill
	Tree    fs.FS
}

// AddNativeSkills registers built-in skills provided by assembled native
// modules unless an installed plugin with the same package name is already
// captured in this turn.
func (r *Resources) AddNativeSkills(packages []NativeSkillPackage) {
	if r == nil {
		return
	}
	for _, entry := range packages {
		if entry.Package == "" || len(entry.Skills) == 0 || entry.Tree == nil {
			continue
		}
		if slices.ContainsFunc(r.packages, func(p Package) bool { return p.Reference.Name == entry.Package }) {
			continue
		}
		r.packages = append(r.packages, Package{
			Reference: protocol.PluginReference{Name: entry.Package},
			Skills:    slices.Clone(entry.Skills),
			Scope:     protocol.PluginScopeHost,
			native:    true,
			tree:      entry.Tree,
		})
	}
}

type Resources struct {
	profile  string
	packages []Package
}

type SkillText struct{ Content, Directory string }

// CaptureResources resolves only the exact package versions admitted by a turn.
func CaptureResources(ctx context.Context, profile string, references []protocol.PluginReference) (*Resources, error) {
	value := &Resources{profile: profile}
	manager := plugins.NewManager(profile)
	for _, reference := range references {
		info, err := manager.CapturedInfo(ctx, reference)
		if err != nil {
			return nil, err
		}
		directory, err := manager.PackageRoot(reference)
		if err != nil {
			return nil, err
		}
		captured := Package{Reference: reference, Requirements: info.Requirements, Skills: info.Skills, directory: directory, standalone: info.Skill != nil}
		captured.read = func(ctx context.Context, name string, limit int) ([]byte, error) {
			return manager.ReadResource(ctx, reference, name, limit)
		}
		if info.Extension != nil {
			captured.Scope, captured.Modes = info.Extension.Scope, info.Extension.ConversationModes
			if info.Extension.ConversationOptions != nil {
				captured.Options = *info.Extension.ConversationOptions
			}
		}
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

func (r *Resources) Python(ctx context.Context, key string, manager *python.Manager) (*python.Ready, error) {
	owner, _, _ := strings.Cut(key, ":")
	index := slices.IndexFunc(r.packages, func(value Package) bool { return value.Reference.Name == owner })
	if index < 0 || manager == nil {
		return nil, nil
	}
	value := r.packages[index]
	var requirements *python.Requirements
	if !value.standalone {
		var content []byte
		var err error
		if value.read != nil {
			content, err = value.read(ctx, "requirements.txt", 512*1024)
		} else {
			content, err = fs.ReadFile(value.tree, "requirements.txt")
		}
		var fault *protocol.Fault
		if err != nil && !errors.Is(err, fs.ErrNotExist) && (!errors.As(err, &fault) || fault.Code != protocol.ErrorCodeNotFound) {
			return nil, err
		}
		if err == nil {
			requirements, err = python.NewRequirements(value.Reference, string(content))
			if err != nil {
				return nil, err
			}
		}
	}
	ready, err := manager.Ensure(ctx, requirements)
	if err != nil && requirements == nil && ctx.Err() == nil {
		return nil, nil
	}
	return ready, err
}

func (r *Resources) Catalog(child bool) (string, error) {
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
	selection := "the user explicitly names or whose descriptions clearly match the task"
	if child {
		selection = "the parent assigns or whose descriptions clearly match the delegated scope"
	}
	return "Available skills (package:skill) for this turn:\n1. Review this catalog and select the smallest set " + selection + ".\n2. Load their complete instructions with load_skill before substantive work unless the full instructions were already supplied.\n3. Apply the relevant workflow within this turn's permissions.\nRead relevant referenced text with read_skill_resource. Resolve relative script paths against the returned skill directory and quote absolute script paths in command calls. Command cwd defaults to the session worktree. Skill files grant no tool permission.\nSkills:\n" + text, nil
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
	var expected []byte
	var err error
	if p.read != nil {
		expected, err = p.read(ctx, resource, SkillBytes)
	} else {
		expected, err = fs.ReadFile(p.tree, resource)
	}
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

func (r *Resources) materialize(ctx context.Context, p Package) (string, error) {
	if p.directory != "" {
		return p.directory, nil
	}
	if p.native && p.tree != nil {
		if err := core.PrivateDirectory(r.profile); err != nil {
			return "", err
		}
		profile, err := filepath.Abs(r.profile)
		if err != nil {
			return "", err
		}
		parent := profile
		for _, segment := range []string{"modules", "skills", p.Reference.Name} {
			parent = filepath.Join(parent, segment)
			info, statErr := os.Lstat(parent)
			if errors.Is(statErr, os.ErrNotExist) {
				if err := os.Mkdir(parent, 0o700); err != nil && !errors.Is(err, os.ErrExist) {
					return "", err
				}
				continue
			}
			if statErr != nil {
				return "", statErr
			}
			if !info.IsDir() {
				return "", resourceFault(protocol.ErrorCodeUnavailable, "native skill resource directory changed")
			}
		}
		directory := parent
		err = fs.WalkDir(p.tree, ".", func(entryPath string, d fs.DirEntry, walkErr error) error {
			if walkErr != nil || entryPath == "." {
				return walkErr
			}
			target := filepath.Join(directory, filepath.FromSlash(entryPath))
			if d.IsDir() {
				info, statErr := os.Lstat(target)
				if errors.Is(statErr, os.ErrNotExist) {
					return os.MkdirAll(target, 0o700)
				}
				if statErr != nil {
					return statErr
				}
				if !info.IsDir() {
					return resourceFault(protocol.ErrorCodeUnavailable, "native skill resource directory changed")
				}
				return nil
			}
			data, err := fs.ReadFile(p.tree, entryPath)
			if err != nil {
				return err
			}
			if info, statErr := os.Lstat(target); statErr == nil {
				if !info.Mode().IsRegular() {
					return resourceFault(protocol.ErrorCodeUnavailable, "native skill resource changed")
				}
				if existing, err := os.ReadFile(target); err == nil && bytes.Equal(existing, data) {
					return nil
				}
			} else if !errors.Is(statErr, os.ErrNotExist) {
				return statErr
			}
			dir := filepath.Dir(target)
			if err := os.MkdirAll(dir, 0o700); err != nil {
				return err
			}
			tmp, err := os.CreateTemp(dir, ".skill-*")
			if err != nil {
				return err
			}
			tmpPath := tmp.Name()
			if _, err := tmp.Write(data); err != nil {
				_ = tmp.Close()
				_ = os.Remove(tmpPath)
				return err
			}
			if err := tmp.Chmod(0o600); err != nil {
				_ = tmp.Close()
				_ = os.Remove(tmpPath)
				return err
			}
			if err := tmp.Close(); err != nil {
				_ = os.Remove(tmpPath)
				return err
			}
			if err := os.Rename(tmpPath, target); err != nil {
				_ = os.Remove(tmpPath)
				return err
			}
			return nil
		})
		if err != nil {
			return "", err
		}
		return directory, nil
	}
	return "", resourceFault(protocol.ErrorCodeUnavailable, "captured plugin resources are unavailable")
}

func resourceFault(code protocol.ErrorCode, message string) error {
	return &protocol.Fault{Code: code, Message: message}
}
