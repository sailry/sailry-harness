package plugins

import (
	"archive/tar"
	"archive/zip"
	"bytes"
	"compress/gzip"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"io/fs"
	"math/rand/v2"
	"net/http"
	"net/http/httptest"
	"net/url"
	"os"
	"path/filepath"
	"runtime"
	"slices"
	"strings"
	"testing"
	"testing/fstest"
	"time"

	git "github.com/go-git/go-git/v5"
	"github.com/go-git/go-git/v5/plumbing/object"
	sailry "github.com/sailry/sailry-harness"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func packageFixture(t *testing.T, directory, name, text string) {
	t.Helper()
	filename := filepath.Join(directory, filepath.FromSlash(name))
	if err := os.MkdirAll(filepath.Dir(filename), 0o700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filename, []byte(text), 0o600); err != nil {
		t.Fatal(err)
	}
}

func TestPackageBoundaries(t *testing.T) {
	t.Parallel()
	t.Run("Status declarations", testStatusExtension)
	for _, name := range []string{"", "-plugin", "plugin-", "Plugin", "a--b", "a..b", "a/b", "a_b", strings.Repeat("a", 65)} {
		if ValidateName(name) {
			t.Errorf("accepted invalid package name %q", name)
		}
	}
	for _, name := range []string{"a", "plugin.v1", "my-plugin", strings.Repeat("a", 64)} {
		if !ValidateName(name) {
			t.Errorf("rejected portable package name %q", name)
		}
	}
	manifest := `{"$schema":"` + ManifestSchema + `","name":"fixture","version":"0.1.0","extra":true}`
	parsed, err := ValidateManifest([]byte(manifest))
	if err != nil || len(parsed.Issues) != 1 || parsed.Issues[0].Kind != protocol.PluginIssueKindIgnoredManifestField {
		t.Fatalf("portable metadata issue: %#v %v", parsed.Issues, err)
	}
	t.Run("connector command line", func(t *testing.T) {
		for _, entry := range []struct {
			name, command, env, argv, timeout string
			valid                             bool
		}{
			{"declared", "gh", `{"GH_PROMPT_DISABLED":"1"}`, `["auth","token","--user","{account}"]`, "5000", true},
			{"search path", "gh", `{"PATH":"/package/bin"}`, `["auth","token"]`, "5000", false},
			{"home", "gh", `{"HOME":"/package"}`, `["auth","token"]`, "5000", false},
			{"roaming profile", "gh", `{"APPDATA":"C:\\package"}`, `["auth","token"]`, "5000", false},
			{"lower-case name", "gh", `{"path":"/package/bin"}`, `["auth","token"]`, "5000", false},
			{"value with NUL", "gh", `{"GH_HOST":"a\u0000b"}`, `["auth","token"]`, "5000", false},
			{"oversized value", "gh", `{"GH_HOST":"` + strings.Repeat("a", 2049) + `"}`, `["auth","token"]`, "5000", false},
			{"nine variables", "gh", `{"A":"","B":"","C":"","D":"","E":"","F":"","G":"","H":"","I":""}`, `["auth","token"]`, "5000", false},
			{"command path", "../gh", `{}`, `["auth","token"]`, "5000", false},
			{"command case", "GH", `{}`, `["auth","token"]`, "5000", false},
			{"command prefix", "-gh", `{}`, `["auth","token"]`, "5000", false},
			{"command length", strings.Repeat("g", 65), `{}`, `["auth","token"]`, "5000", false},
			{"unknown placeholder", "gh", `{}`, `["auth","{home}"]`, "5000", false},
			{"no arguments", "gh", `{}`, `[]`, "5000", false},
			{"unbounded run", "gh", `{}`, `["auth","token"]`, "30001", false},
		} {
			parsed, err := ValidateManifest([]byte(`{"$schema":"` + ManifestSchema + `","name":"fixture","version":"0.1.0","extensions":{"dev.sailry.platform":{"api_version":"v1","actions":[],"connectors":[` +
				`{"id":"account","display":{"label":"Fixture account"},"accounts":"single","origins":["https://fixture.test"],"fields":[],"inject":[],"auth":[{"kind":"local_cli","command":"` + entry.command +
				`","env":` + entry.env + `,"status":{"argv":["auth","status"],"account":"/login","condition":{"pointer":"/active","values":[true,"yes",1]}},"token":{"argv":` + entry.argv +
				`,"parse":{"json":"/token"}},"timeout_ms":` + entry.timeout + `,"max_output":4096}]}]}}}`))
			if err != nil || (parsed.Extension != nil) != entry.valid {
				t.Errorf("%s: extension admitted=%v issues=%v error=%v", entry.name, parsed.Extension != nil, parsed.Issues, err)
			}
		}
	})
	source, profile := t.TempDir(), t.TempDir()
	packageFixture(t, source, "plugin.json", manifest)
	packageFixture(t, source, "resource.txt", "captured")
	manager := NewManager(profile)
	ctx := t.Context()
	first, err := manager.Install(ctx, source, "", "fixture")
	if err != nil {
		t.Fatal(err)
	}
	packageFixture(t, source, "resource.txt", "updated")
	second, err := manager.Install(ctx, source, "", "fixture")
	if err != nil || first.Summary.Digest == second.Summary.Digest {
		t.Fatalf("new bytes did not produce an immutable package: %v", err)
	}
	body, err := manager.ReadResource(ctx, PackageReference(first), "resource.txt", 100)
	if err != nil || string(body) != "captured" {
		t.Fatalf("capture changed after source edit: %q %v", body, err)
	}
	bundle, err := manager.ReadBundle(ctx, PackageReference(first), "resource.txt", nil)
	if err != nil || len(bundle) != 1 || bundle["resource.txt"] != "captured" {
		t.Fatalf("declared entry with no additional resources was not captured: %v %v", bundle, err)
	}
	if _, err := manager.ReadResource(ctx, PackageReference(first), "../resource.txt", 100); err == nil {
		t.Fatal("resource read escaped its package")
	}
	if _, err := manager.Inspect(ctx, profile, "", "fixture"); err == nil {
		t.Fatal("profile overlap was accepted")
	}
	if runtime.GOOS != "windows" {
		if err := os.Symlink(filepath.Join(source, "resource.txt"), filepath.Join(source, "link.txt")); err != nil {
			t.Fatal(err)
		}
		info, err := manager.Inspect(ctx, source, "", "fixture")
		if err != nil {
			t.Fatal(err)
		}
		found := false
		for _, issue := range info.Issues {
			found = found || issue.Path == "link.txt" && issue.Kind == protocol.PluginIssueKindUnavailablePath
		}
		if !found {
			t.Fatal("symbolic link was not reported as unavailable")
		}
	}
	directory, err := manager.PackageRoot(PackageReference(second))
	if err != nil {
		t.Fatal(err)
	}
	packageFixture(t, directory, "resource.txt", "tampered")
	if _, err := manager.Install(ctx, source, "", "fixture"); err == nil {
		t.Fatal("changed immutable package was reused")
	}
	for _, name := range []string{"../plugin.json", "/plugin.json", "folder\\plugin.json"} {
		var archive bytes.Buffer
		writer := zip.NewWriter(&archive)
		entry, err := writer.Create(name)
		if err != nil {
			t.Fatal(err)
		}
		if _, err := entry.Write([]byte(manifest)); err != nil {
			t.Fatal(err)
		}
		if err := writer.Close(); err != nil {
			t.Fatal(err)
		}
		if _, err := manager.InstallArchive(ctx, bytes.NewReader(archive.Bytes()), int64(archive.Len()), "fixture"); err == nil {
			t.Errorf("accepted unconfined archive entry %q", name)
		}
	}
}

func TestBundledPackageContracts(t *testing.T) {
	t.Parallel()
	manager := NewManager(t.TempDir())
	names, err := fs.Glob(sailry.Plugins, "plugins/*/plugin.json")
	if err != nil {
		t.Fatal(err)
	}
	for _, filename := range names {
		name := strings.Split(filename, "/")[1]
		t.Run(name, func(t *testing.T) {
			info, err := manager.InspectBundled(t.Context(), name)
			if err != nil {
				t.Fatal(err)
			}
			if info.Summary.Name != name || info.Extension == nil || len(info.Summary.Digest) != 64 {
				t.Fatalf("bundled declaration was lost: %#v", info)
			}
			for _, issue := range info.Issues {
				if issue.Kind != protocol.PluginIssueKindIgnoredManifestField {
					t.Errorf("bundled resource issue: %#v", issue)
				}
			}
		})
	}
	if len(BundledDefaults()) != 25 {
		t.Fatal("initial distribution changed")
	}
	for _, selection := range BundledDefaults() {
		info, err := manager.InstallBundled(t.Context(), selection.Name)
		if err != nil {
			t.Fatal(err)
		}
		root, err := manager.PackageRoot(PackageReference(info))
		if err != nil {
			t.Fatal(err)
		}
		if err := verifyPublished(t.Context(), root, info.Summary.Digest); err != nil {
			t.Fatal(err)
		}
	}
}

func TestPackageSchemasAndSkills(t *testing.T) {
	t.Parallel()
	t.Run("canonical card contracts", testCardContracts)
	t.Run("desktop declaration scopes", testDesktopDeclarations)
	t.Run("schemas cannot load host files", func(t *testing.T) {
		directory := t.TempDir()
		packageFixture(t, directory, "external-schema.json", `{"type":"object"}`)
		path := filepath.ToSlash(filepath.Join(directory, "external-schema.json"))
		if path[0] != '/' {
			path = "/" + path
		}
		document := map[string]any{"$ref": (&url.URL{Scheme: "file", Path: path}).String()}
		if _, err := compileSchema(document); err == nil {
			t.Fatal("plugin schema loaded an external host file")
		}
		if _, err := compileSchemaAt(document, ""); err == nil {
			t.Fatal("storage schema loaded an external host file")
		}
		local := map[string]any{"$defs": map[string]any{"name": map[string]any{"type": "string"}}, "$ref": "#/$defs/name"}
		for _, compile := range []func(any) error{
			func(document any) error {
				schema, err := compileSchema(document)
				if err == nil {
					err = schema.Validate("captured")
				}
				return err
			},
			func(document any) error {
				schema, err := compileSchemaAt(document, "")
				if err == nil {
					err = schema.Validate("captured")
				}
				return err
			},
		} {
			if err := compile(local); err != nil {
				t.Fatalf("registered local schema reference failed: %v", err)
			}
		}
	})
	for _, scenario := range []struct {
		before, after string
		newer         bool
	}{
		{"0.1.0", "0.1.1", true},
		{"0.1.0-alpha.1", "0.1.0-alpha.2", true},
		{"0.1.0-alpha.1", "0.1.0", true},
		{"0.1.0+local", "0.1.0+published", false},
		{"0.1.0", "0.1.0-alpha.1", false},
		{"1.0", "2.0.0", false},
	} {
		if IsNewer(&scenario.before, &scenario.after) != scenario.newer {
			t.Errorf("incorrect SemVer precedence: %#v", scenario)
		}
	}
	settings := []byte(`{"$schema":"` + SettingsSchema + `","type":"object","additionalProperties":false,"properties":{"enabled":{"type":"boolean","default":true},"token":{"type":"string","minLength":2,"x-sailry-secret":{"origin":"https://api.example.test","header":"Authorization","prefix":"Bearer "}}},"required":["enabled","token"]}`)
	schema, err := ValidateSettings(settings)
	if err != nil {
		t.Fatal(err)
	}
	if err := ValidatePublicSettings(schema, map[string]json.RawMessage{"enabled": json.RawMessage("true")}); err != nil {
		t.Fatal(err)
	}
	if err := ValidatePublicSettings(schema, map[string]json.RawMessage{"enabled": json.RawMessage("true"), "token": json.RawMessage(`"secret"`)}); err == nil {
		t.Fatal("secret entered public settings")
	}
	for _, invalid := range []string{
		strings.Replace(string(settings), `"origin":"https://api.example.test"`, `"origin":"https://api.example.test/path"`, 1),
		strings.Replace(string(settings), `"header":"Authorization"`, `"header":"Mcp-Session-Id"`, 1),
		strings.Replace(string(settings), `"minLength":2`, `"default":"secret"`, 1),
	} {
		if _, err := ValidateSettings([]byte(invalid)); err == nil {
			t.Fatal("invalid secret schema was accepted")
		}
	}
	key := "settings"
	storage := protocol.PluginStorageDeclaration{Collections: []protocol.PluginStorageCollection{{
		Scope: protocol.PluginStorageScopeNode, Key: &key,
		Schema: json.RawMessage(`{"type":"object","properties":{"title":{"type":"string"},"count":{"type":"integer","default":1}}}`),
	}}}
	if err := ValidateStorage(storage); err != nil {
		t.Fatal(err)
	}
	value, err := NormalizeStorageValue(&storage.Collections[0], json.RawMessage(`{"title":"old","private":{"retained":true}}`), json.RawMessage(`{"title":"new"}`))
	if err != nil || !bytes.Contains(value, []byte(`"private"`)) || !bytes.Contains(value, []byte(`"count":1`)) {
		t.Fatalf("storage projection lost private fields or defaults: %s %v", value, err)
	}
	for _, raw := range []string{
		`{"type":"object","additionalProperties":false}`,
		`{"$ref":"https://example.test/schema.json"}`,
		`{"properties":{"value":{"$id":"nested"}}}`,
		`{"properties":{"count":{"type":"integer","default":"wrong"}}}`,
	} {
		storage.Collections[0].Schema = json.RawMessage(raw)
		if err := ValidateStorage(storage); err == nil {
			t.Fatalf("unsupported storage schema was accepted: %s", raw)
		}
	}
	document := "\ufeff\n---\nname: review\ndescription: Review changes\nmetadata:\n  display-name: Code review\n  display-name.zh-CN: 代码审阅\nallowed-tools: [read, search]\n---\nRead all changes"
	skill, err := ParseSkill([]byte(document), "review")
	if err != nil || skill.DisplayName == nil || *skill.DisplayName != "Code review" || skill.DisplayLocales["zh-CN"] != "代码审阅" {
		t.Fatalf("skill metadata did not survive: %#v %v", skill, err)
	}
	for _, document := range []string{
		"---\nname: review\nname: duplicate\ndescription: Invalid\n---\nBody",
		"---\nname: Uppercase\ndescription: Invalid\n---\nBody",
		"---\nname: review\ndescription: ''\n---\nBody",
	} {
		if _, err := ParseSkill([]byte(document), ""); err == nil {
			t.Fatal("invalid skill frontmatter was accepted")
		}
	}
	t.Run("archive executable and license", func(t *testing.T) {
		for _, format := range []protocol.PluginSkillsArchiveFormat{protocol.PluginSkillsArchiveFormatZip, protocol.PluginSkillsArchiveFormatTar, protocol.PluginSkillsArchiveFormatTarGz} {
			t.Run(string(format), func(t *testing.T) {
				data := skillSourceArchive(t, format)
				manager := NewManager(t.TempDir())
				discovery, err := manager.DiscoverSkillImport(t.Context(), data, protocol.NewStreamID(), format, protocol.PluginUploadSourceArchive)
				if err != nil || len(discovery.Skills) != 1 {
					t.Fatalf("discovery: %#v %v", discovery, err)
				}
				candidate := discovery.Skills[0]
				info, err := manager.InstallSkillImport(t.Context(), data, format, protocol.PluginSkillsImported{Digest: discovery.Digest, Path: candidate.Path, Kind: discovery.Kind}, candidate.Name)
				if err != nil {
					t.Fatal(err)
				}
				root, err := manager.PackageRoot(PackageReference(info))
				if err != nil {
					t.Fatal(err)
				}
				metadata, err := os.Stat(filepath.Join(root, "skills/review/scripts/check"))
				if err != nil || runtime.GOOS != "windows" && metadata.Mode().Perm()&0o111 == 0 {
					t.Fatalf("executable was lost: %v", err)
				}
				if _, err := os.Stat(filepath.Join(root, "skills/review/empty")); err != nil {
					t.Fatalf("empty directory was lost: %v", err)
				}
				license, err := os.ReadFile(filepath.Join(root, "SOURCE-LICENSE"))
				if err != nil || string(license) != "Source license" {
					t.Fatalf("source license was lost: %q %v", license, err)
				}
			})
		}
	})
	t.Run("snapshot source ceiling", func(t *testing.T) {
		source := t.TempDir()
		packageFixture(t, source, "review/SKILL.md", "---\nname: review\ndescription: Review changes\n---\nBody")
		packageFixture(t, source, ".git/private", "not included")
		packageFixture(t, source, ".sailry/worktrees/another/private", "not included")
		for index := 0; index < 1025; index++ {
			packageFixture(t, source, fmt.Sprintf("other/file-%04d", index), "")
		}
		manager := NewManager(t.TempDir())
		data, format, kind, err := manager.SnapshotSkills(t.Context(), source, "")
		if err != nil {
			t.Fatal(err)
		}
		files, err := readSkillArchive(t.Context(), data, format)
		if err != nil || files[".git/private"] != nil || files[".sailry/worktrees/another/private"] != nil {
			t.Fatalf("snapshot scope: %v", err)
		}
		discovery, err := manager.DiscoverSkillImport(t.Context(), data, protocol.NewStreamID(), format, kind)
		if err != nil || len(discovery.Skills) != 1 {
			t.Fatalf("pre-selection source was limited to a package: %v", err)
		}
		tree := fstest.MapFS{}
		for index := 0; index <= 10000; index++ {
			tree[fmt.Sprintf("file-%05d", index)] = &fstest.MapFile{}
		}
		_, _, err = capturePackage(t.Context(), tree, func(name string) (io.ReadCloser, error) { return tree.Open(name) }, packageCaptureOptions{maximum: 10000})
		if err == nil {
			t.Fatal("snapshot accepted more than 10000 entries")
		}
	})
	t.Run("Git skill commit update", func(t *testing.T) {
		source := t.TempDir()
		repository, err := git.PlainInit(source, false)
		if err != nil {
			t.Fatal(err)
		}
		worktree, err := repository.Worktree()
		if err != nil {
			t.Fatal(err)
		}
		packageFixture(t, source, "nested/review/SKILL.md", "---\nname: review\ndescription: Review changes\n---\nFirst body")
		packageFixture(t, source, "nested/review/scripts/check", "first executable")
		if err := os.Chmod(filepath.Join(source, "nested/review/scripts/check"), 0700); err != nil {
			t.Fatal(err)
		}
		packageFixture(t, source, "LICENSE", "Repository license")
		commit := func(message string) {
			t.Helper()
			if _, err := worktree.Add("."); err != nil {
				t.Fatal(err)
			}
			if _, err := worktree.Commit(message, &git.CommitOptions{Author: &object.Signature{Name: "Fixture", Email: "fixture@example.test", When: time.Unix(0, 0)}}); err != nil {
				t.Fatal(err)
			}
		}
		commit("Initial skill")
		path := "nested/review"
		manager := NewManager(t.TempDir())
		discovery, err := manager.DiscoverRepositorySkills(t.Context(), protocol.PluginSkillsSource{Repository: source, Path: &path})
		if err != nil || len(discovery.Skills) != 1 {
			t.Fatalf("Git discovery: %v", err)
		}
		current, err := manager.InstallRepositorySkill(t.Context(), discovery.Source, path, discovery.Skills[0].Name)
		if err != nil {
			t.Fatal(err)
		}
		root, err := manager.PackageRoot(PackageReference(current))
		if err != nil {
			t.Fatal(err)
		}
		if license, err := os.ReadFile(filepath.Join(root, "SOURCE-LICENSE")); err != nil || string(license) != "Repository license" {
			t.Fatalf("repository-root license: %q %v", license, err)
		}
		if metadata, err := os.Stat(filepath.Join(root, "skills/review/scripts/check")); err != nil || runtime.GOOS != "windows" && metadata.Mode().Perm()&0o111 == 0 {
			t.Fatalf("Git executable: %v", err)
		}
		report, err := manager.CheckUpdate(t.Context(), current, nil, true)
		if err != nil || report.Manual || report.Available != nil {
			t.Fatalf("unchanged Git source: %#v %v", report, err)
		}
		packageFixture(t, source, "nested/review/SKILL.md", "---\nname: review\ndescription: Review changes\n---\nSecond body")
		commit("Update skill")
		report, err = manager.CheckUpdate(t.Context(), current, nil, true)
		if err != nil || report.Manual || report.Available == nil {
			t.Fatalf("Git source update: %#v %v", report, err)
		}
		var candidate protocol.PluginInfo
		if err := decodePackageData(report.Available.Data, &candidate); err != nil || candidate.Summary.Digest == current.Summary.Digest {
			t.Fatalf("Git digest did not advance: %v", err)
		}
		if body, err := os.ReadFile(filepath.Join(root, "skills/review/SKILL.md")); err != nil || !bytes.Contains(body, []byte("First body")) {
			t.Fatalf("inspection changed the installed capture: %v", err)
		}
		// The captured commit stays readable after its branch moved on.
		selected, err := ParseRepositorySource(protocol.PluginSkillsSource{Repository: source, Path: &path})
		if err != nil {
			t.Fatal(err)
		}
		resolved, entries, _, err := acquireRepository(t.Context(), selected, discovery.Source.Commit)
		if err != nil || resolved.Commit != discovery.Source.Commit {
			t.Fatalf("captured commit: %#v %v", resolved, err)
		}
		if index := slices.IndexFunc(entries, func(entry packageEntry) bool { return entry.name == "SKILL.md" }); index < 0 || !bytes.Contains(entries[index].data, []byte("First body")) {
			t.Fatalf("captured commit followed its branch: %v", entries)
		}
		missing := "missing"
		unknown, err := ParseRepositorySource(protocol.PluginSkillsSource{Repository: source, GitRef: &missing})
		if err != nil {
			t.Fatal(err)
		}
		var fault *protocol.Fault
		if _, _, _, err := acquireRepository(t.Context(), unknown, ""); !errors.As(err, &fault) || fault.Message != "plugin repository revision is unavailable" {
			t.Fatalf("unknown revision: %v", err)
		}
		// One revision is transferred within a byte allowance: a large file in
		// the newest commit exceeds it, the same file left in history does not.
		large := make([]byte, 64*1024)
		_, _ = rand.NewChaCha8([32]byte{1}).Read(large)
		packageFixture(t, source, "history.bin", string(large))
		packageFixture(t, source, "pack/plugin.json", `{"$schema":"`+ManifestSchema+`","name":"repository-fixture","version":"0.1.0"}`)
		commit("Add history")
		bounded := repositoryCaptureOptions{maximum: MaxPackageEntries, transfer: 16 * 1024}
		if _, _, _, err := acquireRepository(t.Context(), selected, "", bounded); !errors.As(err, &fault) || fault.Code != protocol.ErrorCodeInvalidRequest || fault.Message != "Git transfer exceeds its size limit" {
			t.Fatalf("oversized transfer: %v", err)
		}
		if _, err := worktree.Remove("history.bin"); err != nil {
			t.Fatal(err)
		}
		commit("Drop history")
		if _, _, _, err := acquireRepository(t.Context(), selected, "", bounded); err != nil {
			t.Fatalf("history was transferred with the newest revision: %v", err)
		}
		// An inspected package is held only until it is installed.
		pack := "pack"
		selection, err := manager.InspectRepository(t.Context(), protocol.PluginSkillsSource{Repository: source, Path: &pack})
		if err != nil {
			t.Fatal(err)
		}
		if _, err := manager.InspectRepository(t.Context(), protocol.PluginSkillsSource{Repository: source, Path: &pack}); err != nil || len(manager.repositories) != 1 {
			t.Fatalf("inspections retained: %d %v", len(manager.repositories), err)
		}
		if info, err := manager.InstallRepository(t.Context(), selection); err != nil || info.Summary.Name != "repository-fixture" || len(manager.repositories) != 0 {
			t.Fatalf("repository install: %#v %d %v", info.Summary, len(manager.repositories), err)
		}
	})
}

func skillSourceArchive(t *testing.T, format protocol.PluginSkillsArchiveFormat) []byte {
	t.Helper()
	type sourceEntry struct {
		name, body string
		mode       int64
		directory  bool
	}
	entries := []sourceEntry{
		{name: "LICENSE", body: "Source license", mode: 0600},
		{name: "nested/review/SKILL.md", body: "---\nname: review\ndescription: Review changes\n---\nBody", mode: 0600},
		{name: "nested/review/scripts/check", body: "fixture executable", mode: 0700},
		{name: "nested/review/empty/", mode: 0700, directory: true},
	}
	var output bytes.Buffer
	if format == protocol.PluginSkillsArchiveFormatZip {
		archive := zip.NewWriter(&output)
		for _, entry := range entries {
			header := &zip.FileHeader{Name: entry.name}
			mode := fs.FileMode(entry.mode)
			if entry.directory {
				mode |= fs.ModeDir
			}
			header.SetMode(mode)
			writer, err := archive.CreateHeader(header)
			if err != nil {
				t.Fatal(err)
			}
			if _, err := writer.Write([]byte(entry.body)); err != nil {
				t.Fatal(err)
			}
		}
		if err := archive.Close(); err != nil {
			t.Fatal(err)
		}
		return output.Bytes()
	}
	var destination io.Writer = &output
	var compressed *gzip.Writer
	if format == protocol.PluginSkillsArchiveFormatTarGz {
		compressed = gzip.NewWriter(&output)
		destination = compressed
	}
	archive := tar.NewWriter(destination)
	for _, entry := range entries {
		header := &tar.Header{Name: entry.name, Mode: entry.mode, Size: int64(len(entry.body)), Typeflag: tar.TypeReg}
		if entry.directory {
			header.Typeflag = tar.TypeDir
		}
		if err := archive.WriteHeader(header); err != nil {
			t.Fatal(err)
		}
		if _, err := archive.Write([]byte(entry.body)); err != nil {
			t.Fatal(err)
		}
	}
	if err := archive.Close(); err != nil {
		t.Fatal(err)
	}
	if compressed != nil {
		if err := compressed.Close(); err != nil {
			t.Fatal(err)
		}
	}
	return output.Bytes()
}

func TestPackageCatalogAndSources(t *testing.T) {
	t.Parallel()
	for _, repository := range []string{
		"https://example.test/team/skills.git", "git://example.test/team/skills.git",
		"ssh://git@example.test/team/skills.git", "ssh+git://git@example.test/team/skills.git",
		"git+ssh://git@example.test/team/skills.git", "git@example.test:team/skills.git", "file:///tmp/skills.git",
	} {
		selected, err := ParseRepositorySource(protocol.PluginSkillsSource{Repository: repository})
		if err != nil || selected.Repository != repository {
			t.Errorf("Git transport changed: %q %#v %v", repository, selected, err)
		}
	}
	reference := "release/v1"
	selected, err := ParseRepositorySource(protocol.PluginSkillsSource{Repository: "https://github.com/Owner/Repo/tree/release/v1/skills/review", GitRef: &reference})
	if err != nil || selected.Repository != "https://github.com/owner/repo.git" || selected.Path != "skills/review" {
		t.Fatalf("GitHub tree selection changed: %#v %v", selected, err)
	}
	for _, repository := range []string{
		"https://user:secret@example.test/repo.git", "https://user@example.test/repo.git",
		"ssh://user:secret@example.test/repo.git", "git://user@example.test/repo.git",
		"https://example.test/repo.git?token=value", "file://server/path/repo.git", "ext::command",
	} {
		if _, err := ParseRepositorySource(protocol.PluginSkillsSource{Repository: repository}); err == nil {
			t.Errorf("unsafe Git URL accepted: %s", repository)
		}
	}
	server := httptest.NewServer(http.HandlerFunc(func(writer http.ResponseWriter, request *http.Request) {
		if request.URL.Path == "/unavailable" {
			writer.WriteHeader(http.StatusServiceUnavailable)
			return
		}
		_, _ = writer.Write([]byte(`{"version":1,"source_commit":"` + strings.Repeat("a", 40) + `","packages":[{"id":"catalog-fixture","manifest":{"$schema":"` + ManifestSchema + `","name":"catalog-fixture","version":"0.1.0"}}]}`))
	}))
	defer server.Close()
	manager := NewManager(t.TempDir())
	manager.OfficialURL = server.URL
	page, err := manager.Catalog(context.Background(), protocol.PluginCatalogSourceOfficial, "catalog-fixture", 1, true)
	if err != nil || len(page.Entries) != 1 || page.Entries[0].Commit == nil {
		t.Fatalf("official captured selection missing: %#v %v", page, err)
	}
	// Skill catalog pages are bounded: the least recently used page gives way.
	key := func(index int) string {
		return fmt.Sprintf("%s\x00query-%d\x00%d", protocol.PluginSkillcatalogSourceOfficial, index, 1)
	}
	for index := range maxSkillPages + 1 {
		if index == maxSkillPages {
			if _, err := manager.SkillCatalog(t.Context(), protocol.PluginSkillcatalogSourceOfficial, "query-0", 1, false); err != nil {
				t.Fatal(err)
			}
		}
		if _, err := manager.SkillCatalog(t.Context(), protocol.PluginSkillcatalogSourceOfficial, fmt.Sprintf("query-%d", index), 1, false); err != nil {
			t.Fatal(err)
		}
	}
	_, reused := manager.skillPages[key(0)]
	_, evicted := manager.skillPages[key(1)]
	_, newest := manager.skillPages[key(maxSkillPages)]
	if len(manager.skillPages) != maxSkillPages || !reused || evicted || !newest {
		t.Fatalf("skill catalog pages: %d %v %v %v", len(manager.skillPages), reused, evicted, newest)
	}
	manager.OfficialURL = server.URL + "/unavailable"
	if entries, err := manager.OfficialEntries(t.Context(), false); err != nil || len(entries) == 0 {
		t.Fatalf("shipped catalog fallback missing: %v", err)
	}
	if _, err := manager.OfficialEntries(t.Context(), true); err == nil {
		t.Fatal("forced catalog refresh hid its failure")
	}
}
