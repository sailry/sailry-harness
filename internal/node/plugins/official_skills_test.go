package plugins

import (
	"context"
	"encoding/json"
	"errors"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strings"
	"sync/atomic"
	"testing"
	"testing/fstest"

	"github.com/sailry/sailry-harness/internal/protocol"
)

const officialSkillFixtureName = "skill-policy-fixture"
const officialSkillFixturePath = officialSkillFixtureName + "/skills/inspect"
const officialSkillFixtureDocument = "---\nname: inspect\ndescription: Inspect changes\n---\nPublished body\n"

func skillCatalogFixture(t *testing.T, feed *officialCatalogFeed) (*Manager, func()) {
	t.Helper()
	feed.Skills = json.RawMessage(`[{"package":"` + officialSkillFixtureName + `","path":"skills/inspect/SKILL.md","frontmatter":"---\nname: inspect\ndescription: Inspect changes\n---\n"}]`)
	var payload atomic.Value
	publish := func() {
		t.Helper()
		body, err := json.Marshal(feed)
		if err != nil {
			t.Fatal(err)
		}
		payload.Store(body)
	}
	publish()
	server := httptest.NewServer(http.HandlerFunc(func(writer http.ResponseWriter, _ *http.Request) {
		_, _ = writer.Write(payload.Load().([]byte))
	}))
	t.Cleanup(server.Close)
	manager := NewManager(t.TempDir(), fstest.MapFS{})
	manager.OfficialURL = server.URL
	t.Cleanup(manager.Close)
	return manager, publish
}

func TestOfficialSkillCatalogUsesCurrentReleaseRequirements(t *testing.T) {
	t.Parallel()
	current, older := catalogVersionFixture("2.0.0"), catalogVersionFixture("1.0.0")
	current.Commit, older.Commit = strings.Repeat("b", 40), strings.Repeat("c", 40)
	feed := officialCatalogFeed{Version: 1, Commit: strings.Repeat("a", 40), Packages: []officialCatalogPackage{{
		ID:                  officialSkillFixtureName,
		Manifest:            json.RawMessage(`{"$schema":"` + ManifestSchema + `","name":"` + officialSkillFixtureName + `","version":"2.0.0"}`),
		Versions:            []protocol.PluginCatalogVersion{older, current},
		VersionRequirements: json.RawMessage(`{"1.0.0":{},"2.0.0":{"products":["code"],"requires_modules":["files"]}}`),
	}}}
	manager, publish := skillCatalogFixture(t, &feed)
	search := func(runtime Runtime, force bool) (protocol.PluginSkillcatalogPage, error) {
		return manager.SkillCatalog(WithRuntime(t.Context(), runtime), protocol.PluginSkillcatalogSourceOfficial, "inspect", 1, force)
	}
	code := Runtime{ProductID: "code", Modules: []string{"files"}}
	page, err := search(code, false)
	if err != nil || len(page.Entries) != 1 || page.Total == nil || *page.Total != 1 || page.Entries[0].Source == nil {
		t.Fatalf("compatible release missing: %+v %v", page, err)
	}
	var source protocol.PluginSkillsSource
	if err := decodePackageData(page.Entries[0].Source.Data, &source); err != nil || source.GitRef == nil || *source.GitRef != current.Commit {
		t.Fatalf("source pins the catalog snapshot instead of the release: %+v %v", source, err)
	}
	for _, runtime := range []Runtime{{ProductID: "harness", Modules: []string{"files"}}, {ProductID: "code"}} {
		page, err := search(runtime, false)
		if err != nil || page.Unavailable || len(page.Entries) != 0 || page.Total == nil || *page.Total != 0 {
			t.Fatalf("incompatible runtime %+v got a cached card or an older release: %+v %v", runtime, page, err)
		}
	}
	feed.Packages[0].Versions[1].Withdrawn = true
	publish()
	if page, err := search(code, true); err != nil || len(page.Entries) != 0 {
		t.Fatalf("withdrawn latest skill was offered using old content: %+v %v", page, err)
	}
	feed.Packages[0].Versions = []protocol.PluginCatalogVersion{older}
	publish()
	if _, err := search(code, true); err == nil {
		t.Fatal("missing requirements-bearing release fell back to the index snapshot")
	}
}

func TestOfficialRepositorySkillChecksCapturedManifestAndReplay(t *testing.T) {
	t.Parallel()
	directory := t.TempDir()
	commit := repositoryVersionFixture(t, directory, officialSkillFixtureName)
	packageFixture(t, directory, officialSkillFixturePath+"/SKILL.md", officialSkillFixtureDocument)
	// The v1 projection lacks SKU information here, but the immutable package
	// that supplies the skill has an explicit declaration.
	current := catalogVersionFixture("1.0.0")
	current.Commit, _ = commit(current.Version, `{"api_version":"v1","products":["code"],"requires_modules":["files"]}`)
	snapshot, _ := commit("1.1.0", `{"api_version":"v1"}`)
	feed := officialCatalogFeed{Version: 1, Commit: snapshot, Packages: []officialCatalogPackage{{
		ID:       officialSkillFixtureName,
		Manifest: json.RawMessage(`{"$schema":"` + ManifestSchema + `","name":"` + officialSkillFixtureName + `","version":"1.0.0"}`),
		Versions: []protocol.PluginCatalogVersion{current},
	}}}
	manager, publish := skillCatalogFixture(t, &feed)
	repository, err := ParseRepositorySource(protocol.PluginSkillsSource{Repository: directory})
	if err != nil {
		t.Fatal(err)
	}
	manager.officialRepository = repository.Repository
	code := WithRuntime(t.Context(), Runtime{ProductID: "code", Modules: []string{"files"}})
	harness := WithRuntime(t.Context(), Runtime{ProductID: "harness", Modules: []string{"files"}})
	path := officialSkillFixturePath
	requested := protocol.PluginSkillsSource{Repository: repository.Repository, GitRef: &current.Commit, Path: &path}
	selection, err := manager.DiscoverRepositorySkills(code, requested)
	if err != nil || len(selection.Skills) != 1 || selection.Source.GitRef != current.Commit || selection.Source.Commit != current.Commit {
		t.Fatalf("compatible discovery failed: %+v %v", selection, err)
	}
	name := selection.Skills[0].Name
	documentSource := func(source protocol.PluginSkillsResolved) protocol.PluginSkillsDocumentSource {
		return protocol.PluginSkillsDocumentSource{Kind: "git", Data: protocol.PluginSkillsDocumentSourceGitPayload{Source: source, Path: path}}
	}
	if document, err := manager.ReadSkillDocument(code, documentSource(selection.Source)); err != nil || document.Content != officialSkillFixtureDocument {
		t.Fatalf("compatible document: %+v %v", document, err)
	}
	for _, operation := range []struct {
		name string
		run  func(context.Context) error
	}{
		{"discover", func(ctx context.Context) error {
			_, err := manager.DiscoverRepositorySkills(ctx, requested)
			return err
		}},
		{"inspect", func(ctx context.Context) error {
			_, err := manager.ReadSkillDocument(ctx, documentSource(selection.Source))
			return err
		}},
		{"replay install", func(ctx context.Context) error {
			_, err := manager.InstallRepositorySkill(ctx, selection.Source, path, name)
			return err
		}},
	} {
		if err := operation.run(harness); err == nil || !strings.Contains(err.Error(), "plugin does not support product harness") {
			t.Errorf("%s did not enforce requirements from the captured manifest: %v", operation.name, err)
		}
	}
	if _, err := os.Stat(filepath.Join(manager.profile, "plugins/packages")); !errors.Is(err, os.ErrNotExist) {
		t.Fatalf("rejected source published a package: %v", err)
	}
	stale := protocol.PluginSkillsResolved{Repository: repository.Repository, GitRef: snapshot, Commit: snapshot}
	var fault *protocol.Fault
	if _, err := manager.InstallRepositorySkill(harness, stale, path, name); !errors.As(err, &fault) || fault.Code != protocol.ErrorCodeRevisionConflict {
		t.Fatalf("old catalog snapshot became an unrestricted package: %v", err)
	}
	installed, err := manager.InstallRepositorySkill(code, selection.Source, path, name)
	if err != nil {
		t.Fatal(err)
	}
	feed.Packages[0].Versions[0].Withdrawn = true
	publish()
	if _, err := manager.InstallRepositorySkill(code, selection.Source, path, name); err == nil {
		t.Fatal("an old selection bypassed a release withdrawal")
	}
	installedDocument := protocol.PluginSkillsDocumentSource{Kind: "installed", Data: protocol.PluginSkillsDocumentSourceInstalledPayload{Package: PackageReference(installed), Skill: "inspect"}}
	if document, err := manager.ReadSkillDocument(harness, installedDocument); err != nil || document.Content != officialSkillFixtureDocument {
		t.Fatalf("history lost its captured skill: %+v %v", document, err)
	}
	if report, err := manager.CheckUpdate(harness, installed, nil, true); err != nil || report.Available != nil {
		t.Fatalf("immutable installed skill offered an update or lost its update status: %+v %v", report, err)
	}
	// The same document can still be intentionally imported as Markdown,
	// independently of its availability through the official catalog.
	local := t.TempDir()
	packageFixture(t, local, "SKILL.md", officialSkillFixtureDocument)
	data, format, kind, err := manager.SnapshotSkills(harness, "SKILL.md", local)
	if err != nil {
		t.Fatal(err)
	}
	imported, err := manager.DiscoverSkillImport(harness, data, protocol.NewStreamID(), format, kind)
	if err != nil || len(imported.Skills) != 1 {
		t.Fatalf("manual skill discovery was restricted: %+v %v", imported, err)
	}
	chosen := imported.Skills[0]
	if _, err := manager.InstallSkillImport(harness, data, format, protocol.PluginSkillsImported{Digest: imported.Digest, Path: chosen.Path, Kind: kind}, chosen.Name); err != nil {
		t.Fatalf("manual Markdown import was restricted: %v", err)
	}
}
