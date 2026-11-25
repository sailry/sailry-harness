package plugins

import (
	"archive/zip"
	"encoding/json"
	"io"
	"io/fs"
	"net/http"
	"net/http/httptest"
	"slices"
	"strings"
	"testing"
	"testing/fstest"

	"github.com/sailry/sailry-harness/internal/protocol"
)

type legacyOfficialFixtureRelease struct {
	Name, Version, Commit, Digest, BundledDigest string
}

func legacyOfficialFixture(t *testing.T) (*zip.ReadCloser, []legacyOfficialFixtureRelease) {
	t.Helper()
	archive, err := zip.OpenReader("testdata/official-legacy.zip")
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = archive.Close() })
	metadata, err := fs.ReadFile(archive, "releases.json")
	if err != nil {
		t.Fatal(err)
	}
	var releases []legacyOfficialFixtureRelease
	if err := json.Unmarshal(metadata, &releases); err != nil || len(releases) != 5 {
		t.Fatalf("legacy release fixture: %v", err)
	}
	return archive, releases
}

func TestOfficialLegacyIdentitiesDoNotConstrainThirdPartyNames(t *testing.T) {
	t.Parallel()
	for _, release := range legacyOfficialReleases {
		if !ValidateName(release.name) || !validPackageVersion(release.version) || !commitHash(release.commit) {
			t.Fatalf("invalid immutable release identity: %+v", release)
		}
		for _, digest := range []string{release.digest, release.bundledDigest} {
			info := protocol.DefaultPluginInfo()
			info.Summary.Name, info.Summary.Version, info.Summary.Digest = release.name, &release.version, digest
			if InfoCompatibility(info, Runtime{ProductID: "harness"}).Kind != "unsupported" {
				t.Fatalf("known undeclared native dependency was accepted: %+v", release)
			}
			if InfoCompatibility(info, Runtime{ProductID: "harness", Modules: []string{release.name}}).Kind != "compatible" {
				t.Fatalf("exact historical module requirement was not satisfied: %+v", release)
			}
			info.Summary.Digest = strings.Repeat("f", 64)
			info.Origin = &protocol.PluginOrigin{Kind: "official"}
			if InfoCompatibility(info, Runtime{ProductID: "harness"}).Kind != "compatible" {
				t.Fatalf("a same-name/version package inherited an official content exception: %+v", release)
			}
		}
		version := catalogVersionFixture(release.version)
		version.Commit = release.commit
		entry := protocol.DefaultPluginCatalogEntry()
		entry.ID, entry.Versions = release.name, []protocol.PluginCatalogVersion{version}
		if selected := SelectCompatibleVersion(entry, nil, Runtime{ProductID: "harness"}); selected != nil {
			t.Fatalf("known official history became a broken fallback: %+v", release)
		}
		entry.Versions[0].Commit = strings.Repeat("f", 40)
		if selected := SelectCompatibleVersion(entry, nil, Runtime{ProductID: "harness"}); selected == nil {
			t.Fatalf("an unrelated source inherited an official commit exception: %+v", release)
		}
		entry.Repository = versionText(OfficialRepository + ".git")
		if selected := SelectCompatibleVersion(entry, nil, Runtime{ProductID: "harness"}); selected != nil {
			t.Fatalf("another official commit disguised known legacy dependencies: %+v", release)
		}
		entry.Repository = versionText("https://example.test/third-party.git")
		if selected := SelectCompatibleVersion(entry, nil, Runtime{ProductID: "harness"}); selected == nil {
			t.Fatalf("third-party source inherited official version requirements: %+v", release)
		}
	}
}

func TestRealOfficialLegacyContentRequiresItsNativeModule(t *testing.T) {
	t.Parallel()
	archive, releases := legacyOfficialFixture(t)
	for _, release := range releases {
		t.Run(release.Name, func(t *testing.T) {
			tree, err := fs.Sub(archive, release.Name)
			if err != nil {
				t.Fatal(err)
			}
			entries, issues, err := capturePackage(t.Context(), tree, func(name string) (io.ReadCloser, error) { return tree.Open(name) })
			if err != nil || digestPackage(entries) != release.Digest {
				t.Fatalf("real release digest changed: %s %s %v", release.Name, release.Version, err)
			}
			allowed := WithRuntime(t.Context(), Runtime{ProductID: "harness", Modules: []string{release.Name}})
			blocked := WithRuntime(t.Context(), Runtime{ProductID: "harness"})
			info, err := describePackage(entries, issues, release.Name, RuntimeFromContext(allowed))
			if err != nil || info.Requirements == nil || info.Requirements.Products != nil || info.Requirements.RequiresModules != nil || info.Summary.Compatibility.Kind != "compatible" {
				t.Fatalf("actual undeclared release: %+v %v", info, err)
			}
			if InfoCompatibility(info, RuntimeFromContext(blocked)).Kind != "unsupported" {
				t.Fatal("actual legacy content bypassed module matching")
			}
			manager := NewManager(t.TempDir())
			if _, err := manager.publish(blocked, entries, issues, release.Name, protocol.PluginOrigin{Kind: "directory"}); err == nil {
				t.Fatal("actual undeclared legacy content installed without its module")
			}
			installed, err := manager.publish(allowed, entries, issues, release.Name, protocol.PluginOrigin{Kind: "directory"})
			if err != nil {
				t.Fatal(err)
			}
			if _, err := manager.CapturedInfo(blocked, PackageReference(installed)); err == nil {
				t.Fatal("actual captured legacy content bypassed module matching")
			}
			if _, err := manager.CapturedInfo(allowed, PackageReference(installed)); err != nil {
				t.Fatal(err)
			}
			for index := range entries {
				entries[index].executable = false
			}
			if digestPackage(entries) != release.BundledDigest {
				t.Fatal("historical bundled digest changed")
			}
			if release.Name == "files" && InfoCompatibility(installed, Runtime{ProductID: "code", Modules: []string{"files"}}).Kind != "compatible" {
				t.Fatal("optional Git integration became mandatory for the real files package")
			}
		})
	}
}

func TestRealOfficialLegacyCatalogAndBundledFallback(t *testing.T) {
	t.Parallel()
	archive, releases := legacyOfficialFixture(t)
	bundled := fstest.MapFS{"plugins/catalog.json": {Data: []byte(`{"version":1,"packages":[]}`)}}
	for _, file := range archive.File {
		if !slices.ContainsFunc(releases, func(release legacyOfficialFixtureRelease) bool { return strings.HasPrefix(file.Name, release.Name+"/") }) {
			continue
		}
		if file.FileInfo().IsDir() {
			bundled["plugins/"+strings.TrimSuffix(file.Name, "/")] = &fstest.MapFile{Mode: fs.ModeDir | 0o755}
			continue
		}
		data, err := fs.ReadFile(archive, file.Name)
		if err != nil {
			t.Fatal(err)
		}
		bundled["plugins/"+file.Name] = &fstest.MapFile{Data: data, Mode: file.Mode()}
	}
	blocked := WithRuntime(t.Context(), Runtime{ProductID: "harness", Modules: []string{"files"}})
	for _, release := range releases {
		manifest, err := fs.ReadFile(archive, release.Name+"/plugin.json")
		if err != nil {
			t.Fatal(err)
		}
		item := map[string]any{
			"id": release.Name, "manifest": json.RawMessage(manifest),
			"versions": []any{map[string]any{"version": release.Version, "commit": release.Commit, "api_version": "v1", "min_sailry_version": "0.1.6"}},
		}
		manager, _ := runtimeCatalogFixture(t, item)
		entry, err := manager.officialEntry(blocked, release.Name, true)
		if err != nil || (entry.CompatibleVersion != nil) != (release.Name == "files") {
			t.Fatalf("actual legacy source record was treated as unrestricted: %+v %v", entry, err)
		}
	}
	server := httptest.NewServer(http.HandlerFunc(func(writer http.ResponseWriter, request *http.Request) {
		writer.WriteHeader(http.StatusServiceUnavailable)
	}))
	t.Cleanup(server.Close)
	manager := NewManager(t.TempDir())
	manager.OfficialURL, manager.bundled = server.URL, bundled
	entries, err := manager.OfficialEntries(blocked, false)
	if err != nil || len(entries) != len(releases) {
		t.Fatalf("offline bundled history: %+v %v", entries, err)
	}
	for _, entry := range entries {
		if (entry.CompatibleVersion != nil) != (entry.ID == "files") {
			t.Fatalf("offline fallback recommended unavailable native content: %+v", entry)
		}
	}
	if _, err := manager.InspectOfficial(blocked, "git", nil, false); err == nil {
		t.Fatal("offline inspection admitted git 0.1.40 without Git")
	}
	if _, err := manager.InstallBundled(blocked, "worktrees"); err == nil {
		t.Fatal("offline installation admitted worktrees 0.1.20 without worktrees")
	}
	if _, err := manager.InstallBundled(blocked, "files"); err != nil {
		t.Fatalf("offline files package required optional Git: %v", err)
	}
}
