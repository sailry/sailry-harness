package plugins

import (
	"encoding/json"
	"errors"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"sync/atomic"
	"testing"
	"time"

	git "github.com/go-git/go-git/v5"
	"github.com/go-git/go-git/v5/plumbing/object"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type officialCatalogFixture struct {
	Version  uint32 `json:"version"`
	Commit   string `json:"source_commit"`
	Packages []struct {
		ID                  string                                `json:"id"`
		Manifest            json.RawMessage                       `json:"manifest"`
		Versions            []protocol.PluginCatalogVersion       `json:"versions"`
		VersionRequirements map[string]catalogRuntimeRequirements `json:"version_requirements,omitempty"`
	} `json:"packages"`
}

func repositoryVersionFixture(t *testing.T, directory, name string) func(version, extension string) (string, []byte) {
	t.Helper()
	repository, err := git.PlainInit(directory, false)
	if err != nil {
		t.Fatal(err)
	}
	worktree, err := repository.Worktree()
	if err != nil {
		t.Fatal(err)
	}
	return func(version, extension string) (string, []byte) {
		t.Helper()
		manifest := []byte(`{"$schema":"` + ManifestSchema + `","name":"` + name + `","version":"` + version + `","extensions":{"dev.sailry.platform":` + extension + `}}`)
		packageFixture(t, directory, name+"/plugin.json", string(manifest))
		packageFixture(t, directory, name+"/CHANGELOG.md", "## "+version+"\nNotes for "+version+".\n")
		if _, err := worktree.Add("."); err != nil {
			t.Fatal(err)
		}
		commit, err := worktree.Commit("Release "+version, &git.CommitOptions{Author: &object.Signature{Name: "Fixture", Email: "fixture@example.test", When: time.Unix(0, 0)}})
		if err != nil {
			t.Fatal(err)
		}
		return commit.String(), manifest
	}
}

func TestOfficialSelectionAndInstallation(t *testing.T) {
	t.Parallel()
	const name = "official-fixture"
	directory := t.TempDir()
	commit := repositoryVersionFixture(t, directory, name)
	feed := officialCatalogFixture{Version: 1}
	feed.Packages = append(feed.Packages, struct {
		ID                  string                                `json:"id"`
		Manifest            json.RawMessage                       `json:"manifest"`
		Versions            []protocol.PluginCatalogVersion       `json:"versions"`
		VersionRequirements map[string]catalogRuntimeRequirements `json:"version_requirements,omitempty"`
	}{ID: name, VersionRequirements: map[string]catalogRuntimeRequirements{}})
	for _, version := range []string{"1.0.0", "1.1.0", "1.2.0-beta.1", "2.0.0", "3.0.0"} {
		extension := `{"api_version":"v1","min_sailry_version":"0.0.1"}`
		item := catalogVersionFixture(version)
		item.APIVersion, item.MinSailryVersion = versionText("v1"), versionText("0.0.1")
		if version == "2.0.0" {
			extension = `{"api_version":"v1","min_sailry_version":"999.0.0"}`
			item.MinSailryVersion = versionText("999.0.0")
		}
		if version == "3.0.0" {
			extension = `{"api_version":"v2","min_sailry_version":"999.0.0","future_feature":true}`
			item.APIVersion, item.MinSailryVersion = versionText("v2"), versionText("999.0.0")
		}
		item.Commit, feed.Packages[0].Manifest = commit(version, extension)
		item.ReleaseNotes = &protocol.PluginCatalogReleaseNotes{Text: "Catalog notes for " + version}
		feed.Commit = item.Commit
		feed.Packages[0].Versions = append(feed.Packages[0].Versions, item)
		feed.Packages[0].VersionRequirements[version] = catalogRuntimeRequirements{}
	}
	withdrawn := catalogVersionFixture("1.9.0")
	withdrawn.Withdrawn = true
	feed.Packages[0].Versions = append(feed.Packages[0].Versions, withdrawn)
	feed.Packages[0].VersionRequirements[withdrawn.Version] = catalogRuntimeRequirements{}
	invalid := catalogVersionFixture("1.8.0")
	invalid.Commit = "unavailable-revision"
	feed.Packages[0].Versions = append(feed.Packages[0].Versions, invalid)
	feed.Packages[0].VersionRequirements[invalid.Version] = catalogRuntimeRequirements{}
	var body atomic.Value
	var requests atomic.Int32
	publishFeed := func() {
		t.Helper()
		encoded, err := json.Marshal(feed)
		if err != nil {
			t.Fatal(err)
		}
		body.Store(encoded)
	}
	publishFeed()
	server := httptest.NewServer(http.HandlerFunc(func(writer http.ResponseWriter, request *http.Request) {
		requests.Add(1)
		_, _ = writer.Write(body.Load().([]byte))
	}))
	defer server.Close()
	manager := NewManager(t.TempDir())
	manager.OfficialURL = server.URL
	source, err := ParseRepositorySource(protocol.PluginSkillsSource{Repository: directory})
	if err != nil {
		t.Fatal(err)
	}
	manager.officialRepository = source.Repository
	entries, err := manager.OfficialEntries(t.Context(), true)
	if err != nil {
		t.Fatal(err)
	}
	index := slices.IndexFunc(entries, func(entry protocol.PluginCatalogEntry) bool { return entry.ID == name })
	if index < 0 || *entries[index].Version != "3.0.0" || entries[index].Compatibility.Kind != "unsupported" || entries[index].CompatibleVersion == nil || entries[index].CompatibleVersion.Version != "1.1.0" || len(entries[index].Versions) != len(feed.Packages[0].Versions) {
		t.Fatalf("latest metadata and compatible history: %+v", entries)
	}
	catalogSource, err := manager.CatalogSource(t.Context(), protocol.PluginCatalogSourceOfficial, name)
	if err != nil || catalogSource.GitRef == nil || *catalogSource.GitRef != feed.Packages[0].Versions[1].Commit {
		t.Fatalf("default catalog source: %+v %v", catalogSource, err)
	}
	selected, err := manager.InspectOfficial(t.Context(), name, nil, true)
	if err != nil || selected.Info.Summary.Version == nil || *selected.Info.Summary.Version != "1.1.0" || selected.Info.UpdateSource == nil || selected.Info.UpdateSource.Kind != "official" || selected.Info.Origin == nil || selected.Info.Origin.Kind != "online" || len(selected.Info.Versions) != len(feed.Packages[0].Versions) {
		t.Fatalf("default selection: %+v %v", selected, err)
	}
	if selected.Source.Commit != feed.Packages[0].Versions[1].Commit || selected.Source.GitRef != selected.Source.Commit || selected.Info.ReleaseNotes == nil || selected.Info.ReleaseNotes.Text != "Catalog notes for 1.1.0" {
		t.Fatalf("selected content and notes: %+v", selected)
	}
	installed, err := manager.InstallOfficial(t.Context(), selected)
	if err != nil || installed.UpdateSource.Kind != "official" || installed.Summary.Digest != selected.Info.Summary.Digest || len(installed.Versions) != len(feed.Packages[0].Versions) {
		t.Fatalf("official install: %+v %v", installed, err)
	}
	for _, version := range []string{"1.0.0", "1.2.0-beta.1"} {
		selection, err := manager.InspectOfficial(t.Context(), name, &version, true)
		if err != nil || *selection.Info.Summary.Version != version {
			t.Fatalf("historical selection %s: %+v %v", version, selection, err)
		}
	}
	for _, version := range []string{"1.8.0", "1.9.0", "2.0.0", "3.0.0"} {
		if _, err := manager.InspectOfficial(t.Context(), name, &version, true); err == nil {
			t.Errorf("incompatible or withdrawn version %s was inspected", version)
		}
	}
	for _, current := range []struct {
		version, want string
	}{
		{"1.0.0", "1.1.0"}, {"1.1.0", ""}, {"1.1.0+local", ""},
		{"1.2.0-beta.0", "1.2.0-beta.1"}, {"3.0.0", ""},
	} {
		info := installed
		info.Summary.Version = &current.version
		report, err := manager.CheckUpdate(t.Context(), info, nil, true)
		if err != nil || report.Blocked != nil || report.Manual || (report.Available == nil) != (current.want == "") {
			t.Fatalf("official update from %s: %+v %v", current.version, report, err)
		}
		if report.Available != nil {
			var candidate protocol.PluginCatalogEntry
			if err := decodePackageData(report.Available.Data, &candidate); err != nil || candidate.CompatibleVersion == nil || candidate.CompatibleVersion.Version != current.want || *candidate.Version != "3.0.0" {
				t.Fatalf("candidate lost latest metadata: %+v %v", candidate, err)
			}
		}
	}
	pinned := installed
	pinned.Summary.Pinned = true
	before := requests.Load()
	if report, err := manager.CheckUpdate(t.Context(), pinned, nil, true); err != nil || report.Available != nil || report.Manual || requests.Load() != before {
		t.Fatalf("pinned package checked its source: %+v %v", report, err)
	}
	// An older official online record without update_source still follows the
	// official package identity, rather than its captured source commit.
	legacy := installed
	legacy.UpdateSource, legacy.Summary.Version = nil, versionText("1.0.0")
	if report, err := manager.CheckUpdate(t.Context(), legacy, nil, true); err != nil || report.Available == nil || report.Available.Kind != "official" {
		t.Fatalf("legacy official source: %+v %v", report, err)
	}
	historical, err := manager.InspectOfficial(t.Context(), name, versionText("1.0.0"), true)
	if err != nil {
		t.Fatal(err)
	}
	feed.Packages[0].Versions[0].Withdrawn = true
	publishFeed()
	if _, err := manager.InstallOfficial(t.Context(), historical); err == nil {
		t.Fatal("a withdrawn selection was installed from the inspection cache")
	}
	if _, err := os.Stat(filepath.Join(manager.profile, "plugins/packages", historical.Info.Summary.Digest)); !errors.Is(err, os.ErrNotExist) {
		t.Fatalf("withdrawn bytes were published: %v", err)
	}
	feed.Packages[0].Versions[0].Withdrawn = false
	feed.Packages[0].Versions[0].Commit = feed.Packages[0].Versions[1].Commit
	publishFeed()
	var fault *protocol.Fault
	if _, err := manager.InstallOfficial(t.Context(), historical); !errors.As(err, &fault) || fault.Code != protocol.ErrorCodeRevisionConflict {
		t.Fatalf("a reassigned catalog version was installed: %v", err)
	}
	if _, err := manager.InspectOfficial(t.Context(), name, versionText("1.0.0"), true); err == nil {
		t.Fatal("catalog and captured package versions differed")
	}
	tampered := selected
	tampered.Info.Summary.Digest = strings.Repeat("f", 64)
	if _, err := manager.InstallOfficial(t.Context(), tampered); !errors.As(err, &fault) || fault.Code != protocol.ErrorCodeRevisionConflict {
		t.Fatalf("an unchecked digest was installed: %v", err)
	}
	// Catalog requirements are hints for candidate selection; the captured
	// manifest is checked again before the actual content is accepted.
	feed.Packages[0].Versions[3].MinSailryVersion = nil
	publishFeed()
	if _, err := manager.InspectOfficial(t.Context(), name, versionText("2.0.0"), true); err == nil {
		t.Fatal("a catalog hint bypassed the captured minimum Node version")
	}
}

func TestBundledOfficialSelection(t *testing.T) {
	t.Parallel()
	server := httptest.NewServer(http.HandlerFunc(func(writer http.ResponseWriter, request *http.Request) {
		writer.WriteHeader(http.StatusServiceUnavailable)
	}))
	defer server.Close()
	manager := NewManager(t.TempDir())
	manager.OfficialURL = server.URL
	selection, err := manager.InspectOfficial(t.Context(), "reminders", nil, false)
	if err != nil || selection.Info.Origin == nil || selection.Info.Origin.Kind != "bundled" || selection.Source != (protocol.PluginSkillsResolved{}) || selection.Path != "" {
		t.Fatalf("bundled fallback selection: %+v %v", selection, err)
	}
	info, err := manager.InstallOfficial(t.Context(), selection)
	if err != nil || info.UpdateSource == nil || info.UpdateSource.Kind != "official" || info.Summary.Digest != selection.Info.Summary.Digest {
		t.Fatalf("bundled official install: %+v %v", info, err)
	}
	selection.Info.Summary.Version = versionText("999.0.0")
	if _, err := manager.InstallOfficial(t.Context(), selection); err == nil {
		t.Fatal("bundled identity was taken from an unverified selection")
	}
}

func TestBundledOfficialSelectionRevalidatesCatalog(t *testing.T) {
	t.Parallel()
	for _, current := range []struct {
		name      string
		withdrawn bool
		code      protocol.ErrorCode
	}{
		{"withdrawn after inspection", true, protocol.ErrorCodeInvalidRequest},
		{"online revision after inspection", false, protocol.ErrorCodeRevisionConflict},
	} {
		t.Run(current.name, func(t *testing.T) {
			t.Parallel()
			var body atomic.Value
			body.Store([]byte(nil))
			server := httptest.NewServer(http.HandlerFunc(func(writer http.ResponseWriter, request *http.Request) {
				data := body.Load().([]byte)
				if data == nil {
					writer.WriteHeader(http.StatusServiceUnavailable)
					return
				}
				_, _ = writer.Write(data)
			}))
			defer server.Close()
			manager := NewManager(t.TempDir())
			manager.OfficialURL = server.URL
			selection, err := manager.InspectOfficial(t.Context(), "reminders", nil, false)
			if err != nil || selection.Info.Origin == nil || selection.Info.Origin.Kind != "bundled" {
				t.Fatalf("offline inspection: %+v %v", selection, err)
			}
			version := *selection.Info.Summary.Version
			commit := strings.Repeat("a", 40)
			encoded, err := json.Marshal(map[string]any{
				"version": 1, "source_commit": commit,
				"packages": []any{map[string]any{
					"id":       "reminders",
					"manifest": map[string]any{"$schema": ManifestSchema, "name": "reminders", "version": version},
					"versions": []any{map[string]any{"version": version, "commit": commit, "withdrawn": current.withdrawn}},
				}},
			})
			if err != nil {
				t.Fatal(err)
			}
			body.Store(encoded)
			var fault *protocol.Fault
			if _, err := manager.InstallOfficial(t.Context(), selection); !errors.As(err, &fault) || fault.Code != current.code {
				t.Fatalf("stale bundled selection: %v", err)
			}
			if _, err := os.Stat(filepath.Join(manager.profile, "plugins/packages", selection.Info.Summary.Digest)); !errors.Is(err, os.ErrNotExist) {
				t.Fatalf("stale bundled content was published: %v", err)
			}
		})
	}
}

func TestOnlineCatalogKeepsFullHistoryWithNewerBundle(t *testing.T) {
	t.Parallel()
	commit := strings.Repeat("a", 40)
	server := httptest.NewServer(http.HandlerFunc(func(writer http.ResponseWriter, request *http.Request) {
		_, _ = writer.Write([]byte(`{"version":1,"source_commit":"` + commit + `","packages":[{"id":"reminders","manifest":{"$schema":"` + ManifestSchema + `","name":"reminders","version":"0.0.1"},"versions":[{"version":"0.0.1","commit":"` + commit + `"},{"version":"0.0.0","commit":"` + strings.Repeat("b", 40) + `"}]}]}`))
	}))
	defer server.Close()
	manager := NewManager(t.TempDir())
	manager.OfficialURL = server.URL
	entries, err := manager.OfficialEntries(t.Context(), true)
	if err != nil {
		t.Fatal(err)
	}
	index := slices.IndexFunc(entries, func(entry protocol.PluginCatalogEntry) bool { return entry.ID == "reminders" })
	if index < 0 || entries[index].Bundled || len(entries[index].Versions) != 2 || *entries[index].Version != "0.0.1" {
		t.Fatalf("bundled fallback replaced online history: %+v", entries)
	}
}

func TestRepositoryUpdatesRetainSelectedRef(t *testing.T) {
	t.Parallel()
	const name = "repository-update-fixture"
	directory := t.TempDir()
	commit := repositoryVersionFixture(t, directory, name)
	first, _ := commit("1.0.0", `{"api_version":"v1"}`)
	manager := NewManager(t.TempDir())
	selection, err := manager.InspectRepository(t.Context(), protocol.PluginSkillsSource{Repository: directory, Path: versionText(name)})
	if err != nil {
		t.Fatal(err)
	}
	installed, err := manager.InstallRepository(t.Context(), selection)
	if err != nil {
		t.Fatal(err)
	}
	var origin, upstream protocol.PluginOriginOnlinePayload
	if decodePackageData(installed.Origin.Data, &origin) != nil || decodePackageData(installed.UpdateSource.Data, &upstream) != nil || origin.Source.Commit != first || origin.Source.GitRef != first || upstream.Source.GitRef != "HEAD" {
		t.Fatalf("provenance and selected ref: %+v %+v", origin, upstream)
	}
	commit("1.1.0", `{"api_version":"v1","min_sailry_version":"999.0.0"}`)
	if report, err := manager.CheckUpdate(t.Context(), installed, nil, true); err != nil || report.Available != nil || report.Blocked != nil {
		t.Fatalf("incompatible ordinary update was offered: %+v %v", report, err)
	}
	commit("1.1.1", `{"api_version":"v1"}`)
	report, err := manager.CheckUpdate(t.Context(), installed, nil, true)
	if err != nil || report.Available == nil || report.Available.Kind != "package" {
		t.Fatalf("selected ref stopped following updates: %+v %v", report, err)
	}
	commit("1.2.0-beta.2", `{"api_version":"v1"}`)
	for _, current := range []struct {
		version   string
		available bool
	}{
		{"1.0.0", false}, {"1.2.0-beta.1", true},
	} {
		info := installed
		info.Summary.Version = &current.version
		report, err := manager.CheckUpdate(t.Context(), info, nil, true)
		if err != nil || report.Blocked != nil || (report.Available != nil) != current.available {
			t.Fatalf("ordinary prerelease update from %s: %+v %v", current.version, report, err)
		}
	}
	commit("1.2.0", `{"api_version":"v1"}`)
	prerelease := installed
	prerelease.Summary.Version = versionText("1.2.0-beta.2")
	if report, err := manager.CheckUpdate(t.Context(), prerelease, nil, true); err != nil || report.Available == nil {
		t.Fatalf("prerelease did not advance to stable: %+v %v", report, err)
	}
}
