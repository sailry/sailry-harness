package operations

import (
	"context"
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"testing/fstest"

	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestSkillLoad(t *testing.T) {
	document := "\ufeff\n---\nname: inspect\ndescription: Inspect bounded resources\nmetadata:\n  display-name.zh-CN: 检查\n...\n\n# Instructions\nKeep the complete document Ω\n"
	tree := fstest.MapFS{"skills/inspect/SKILL.md": {Data: []byte(document)}, "skills/inspect/docs/guide.txt": {Data: []byte("complete text 界🙂\n")}, "skills/inspect/binary": {Data: []byte{0xff, 0xfe}}, "skills/inspect/large": {Data: []byte(strings.Repeat("x", SkillBytes+1))}}
	tree["plugin.json"] = &fstest.MapFile{Data: []byte(`{"$schema":"https://agent-plugins.org/schemas/1.0.0/plugin.schema.json","name":"fixture","version":"1.0.0","description":"Fixture"}`)}
	source, profile := t.TempDir(), t.TempDir()
	if err := os.CopyFS(source, tree); err != nil {
		t.Fatal(err)
	}
	info, err := plugins.NewManager(profile).Install(t.Context(), source, "", "fixture")
	if err != nil {
		t.Fatal(err)
	}
	if len(info.Skills) != 1 || info.Skills[0].DisplayLocales["zh-CN"] != "检查" {
		t.Fatal("skill metadata", info.Skills)
	}
	resources, err := CaptureResources(t.Context(), profile, []protocol.PluginReference{plugins.PackageReference(info)})
	if err != nil {
		t.Fatal(err)
	}
	value, err := resources.Read(t.Context(), "fixture:inspect", "SKILL.md")
	if err != nil || value.Content != document || !filepath.IsAbs(value.Directory) {
		t.Fatal(value, err)
	}
	if info, err := os.Stat(value.Directory); err != nil || !info.IsDir() {
		t.Fatal("skill directory is not a real Node directory", err)
	}
	read, err := resources.Read(t.Context(), "fixture:inspect", "docs/guide.txt")
	if err != nil || read.Content != "complete text 界🙂\n" {
		t.Fatal(read, err)
	}
	for _, path := range []string{".", "../escape", "/absolute", "docs/../guide.txt", "docs//guide.txt", `docs\guide.txt`, "binary", "large", "missing"} {
		if _, err := resources.Read(t.Context(), "fixture:inspect", path); err == nil {
			t.Errorf("invalid resource accepted: %q", path)
		}
	}
	for _, key := range []string{"inspect", "fixture:missing", "other:inspect", "fixture:inspect:extra"} {
		if _, err := resources.Read(t.Context(), key, "SKILL.md"); err == nil {
			t.Errorf("uncaptured skill accepted: %q", key)
		}
	}
	ctx, cancel := context.WithCancel(t.Context())
	cancel()
	if _, err := resources.Read(ctx, "fixture:inspect", "SKILL.md"); err == nil {
		t.Fatal("cancelled resource read succeeded")
	}
	t.Run("installed skill-only packages retain captured identities", func(t *testing.T) {
		captured, err := CaptureResources(t.Context(), profile, []protocol.PluginReference{plugins.PackageReference(info)})
		if err != nil || len(captured.Skills()) != 1 || captured.packages[0].Reference != plugins.PackageReference(info) {
			t.Fatal("skill-only package missing from admission", err)
		}
	})
	if err := os.WriteFile(filepath.Join(value.Directory, "docs", "guide.txt"), []byte("changed"), 0600); err != nil {
		t.Fatal(err)
	}
	if _, err := resources.Read(t.Context(), "fixture:inspect", "docs/guide.txt"); err == nil {
		t.Fatal("changed admitted resource was silently reread")
	}
	t.Run("stopped turn stops capturing an installed package", func(t *testing.T) {
		profile := t.TempDir()
		installed := protocol.PluginReference{Name: "fixture", Digest: strings.Repeat("a", 64)}
		if err := os.MkdirAll(filepath.Join(profile, "plugins", "packages", installed.Digest), 0o700); err != nil {
			t.Fatal(err)
		}
		if _, err := CaptureResources(ctx, profile, []protocol.PluginReference{installed}); !errors.Is(err, context.Canceled) {
			t.Fatalf("capture after stop=%v", err)
		}
	})
	t.Run("metadata preserves source scalar types", func(t *testing.T) {
		for _, header := range []string{"name: inspect\ndescription: 123\n", "name: inspect\ndescription: Description\nmetadata:\n  bad: 123\n", "name: inspect\ndescription: Description\nallowed-tools: null\n", "name: other\ndescription: Description\n"} {
			source, profile := t.TempDir(), t.TempDir()
			if err := os.CopyFS(source, tree); err != nil {
				t.Fatal(err)
			}
			if err := os.WriteFile(filepath.Join(source, "skills", "inspect", "SKILL.md"), []byte("---\n"+header+"---\nBody"), 0o600); err != nil {
				t.Fatal(err)
			}
			info, err := plugins.NewManager(profile).Install(t.Context(), source, "", "fixture")
			if err == nil && len(info.Skills) != 0 {
				t.Fatalf("invalid metadata accepted: %q", header)
			}
		}
	})
}

func TestCapturedSkillRetainsDigestAcrossUpdates(t *testing.T) {
	source, profile := t.TempDir(), t.TempDir()
	manager := plugins.NewManager(profile)
	const document = "---\nname: inspect\ndescription: Inspect captured resources\n---\nOriginal instructions\n"
	tree := fstest.MapFS{
		"plugin.json":             {Data: []byte(`{"$schema":"` + plugins.ManifestSchema + `","name":"fixture","version":"1.0.0"}`)},
		"skills/inspect/SKILL.md": {Data: []byte(document)},
	}
	if err := os.CopyFS(source, tree); err != nil {
		t.Fatal(err)
	}
	info, err := manager.Install(t.Context(), source, "", "fixture")
	if err != nil {
		t.Fatal(err)
	}
	ref := plugins.PackageReference(info)
	if err := os.WriteFile(filepath.Join(source, "skills", "inspect", "SKILL.md"), []byte(document+"Updated instructions\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	updated, err := manager.Install(t.Context(), source, "", "fixture")
	if err != nil || updated.Summary.Digest == ref.Digest {
		t.Fatal("update did not produce a new capture", err)
	}
	resources, err := CaptureResources(t.Context(), profile, []protocol.PluginReference{ref})
	if err != nil {
		t.Fatal(err)
	}
	resources.AddNativeSkills([]NativeSkillPackage{{Package: "fixture", Skills: []protocol.PluginSkill{{Name: "inspect"}}, Tree: fstest.MapFS{
		"skills/inspect/SKILL.md": {Data: []byte("Native instructions")},
	}}})
	read, err := resources.Read(t.Context(), "fixture:inspect", "SKILL.md")
	if err != nil || read.Content != document || resources.packages[0].Reference != ref {
		t.Fatal("captured skill was replaced by the updated package or native module", read, err)
	}
	if _, err := CaptureCatalog(t.Context(), profile, []protocol.PluginReference{ref}); err != nil {
		t.Fatal("old capture is no longer available", err)
	}
	root, err := manager.PackageRoot(ref)
	if err != nil {
		t.Fatal(err)
	}
	manifest := filepath.Join(root, "plugin.json")
	if err := os.Remove(manifest); err != nil {
		t.Fatal(err)
	}
	if _, err := resources.Read(t.Context(), "fixture:inspect", "SKILL.md"); err == nil {
		t.Fatal("read accepted a damaged published package")
	}
	if _, err := os.Stat(manifest); !errors.Is(err, os.ErrNotExist) {
		t.Fatalf("read modified an existing package: %v", err)
	}
}

func TestModeMapping(t *testing.T) {
	declaration := protocol.PluginModesDeclaration{ID: "inspect", Option: "workflow", Display: protocol.PluginDesktopNavigation{Label: "Inspect"}, Permissions: protocol.PluginModesPermissions{Write: false}, Instructions: "Inspect the selected work"}
	for _, row := range []struct {
		name      string
		mode      protocol.WorkMode
		write     bool
		second    bool
		wantError bool
	}{{"read-only Plan", protocol.WorkModePlan, false, false, false}, {"write selected in Plan", protocol.WorkModePlan, true, false, false}, {"read-only Code refused", protocol.WorkModeCode, false, false, true}, {"write Code", protocol.WorkModeCode, true, false, false}, {"Flagship conflict", protocol.WorkModeFlagship, true, false, true}, {"two package modes conflict", protocol.WorkModePlan, false, true, true}} {
		t.Run(row.name, func(t *testing.T) {
			copy := declaration
			copy.Permissions.Write = row.write
			p := Package{Reference: protocol.PluginReference{Name: "fixture", Digest: strings.Repeat("a", 64)}, Scope: protocol.PluginScopeHost, Modes: []protocol.PluginModesDeclaration{copy}, Options: json.RawMessage(`{"properties":{"workflow":{"type":"string","default":"","enum":["","inspect"]}}}`)}
			resources := &Resources{packages: []Package{p}}
			config := protocol.DefaultSessionConfig()
			config.Mode = row.mode
			config.Options = map[string]map[string]json.RawMessage{"fixture": {"workflow": json.RawMessage(`"inspect"`)}}
			if row.second {
				other := p
				other.Reference.Name = "other"
				resources.packages = append(resources.packages, other)
				config.Options["other"] = config.Options["fixture"]
			}
			selected, err := resources.SelectedMode(config)
			if (err != nil) != row.wantError || !row.wantError && (selected == nil || selected.Declaration.ID != "inspect") {
				t.Fatal(selected, err)
			}
		})
	}
	t.Run("native arguments must be an object", func(t *testing.T) {
		for _, operation := range []protocol.ToolOperation{"git.status", "files.search", "process.read"} {
			if _, err := Command(operation, json.RawMessage(`null`), protocol.WorktreeID{}, protocol.SessionID{}, protocol.TurnID{}, nil, true); err == nil {
				t.Fatalf("%s accepted null arguments", operation)
			}
		}
	})
}
