package node

import (
	"bytes"
	"database/sql"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"reflect"
	"strings"
	"testing"
	"time"

	git "github.com/go-git/go-git/v5"
	"github.com/go-git/go-git/v5/plumbing/object"
	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/node/store"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestOfficialVersionOperationsUseRequestLedger(t *testing.T) {
	node := nodeFixture(t)
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) { w.WriteHeader(http.StatusServiceUnavailable) }))
	t.Cleanup(server.Close)
	node.plugins.manager.OfficialURL = server.URL
	inspect := protocol.NewRequest(node.ID(), protocol.Command{Kind: "read_catalog_plugin_info", Data: protocol.CommandReadCatalogPluginInfoPayload{Source: protocol.PluginCatalogSourceOfficial, ID: "gomoku"}})
	inspected := dispatchFixture(t, node, node.ID(), inspect)
	if inspected.Fault != nil {
		t.Fatal(inspected.Fault)
	}
	details, valid := dispatchPayload[protocol.PluginCatalogDetails](inspected.Output.Data)
	info, described := dispatchPayload[protocol.PluginInfo](details.Data)
	if !valid || !described || details.Kind != "plugin" || info.UpdateSource == nil || info.UpdateSource.Kind != "official" {
		t.Fatalf("official inspection lost source identity: %+v", inspected.Output)
	}
	install := protocol.NewRequest(node.ID(), protocol.Command{Kind: "install_catalog_plugin", Data: protocol.CommandInstallCatalogPluginPayload{
		Selection: protocol.PluginCatalogSelection{Info: info}, Mode: protocol.PluginInstallModeInstall,
	}})
	var installed protocol.PluginInfo
	var installedWire []byte
	for attempt := range 2 {
		result := dispatchFixture(t, node, node.ID(), install)
		if result.Fault != nil {
			t.Fatal(result.Fault)
		}
		value, valid := dispatchPayload[protocol.PluginInfo](result.Output.Data)
		if !valid || result.Output.Kind != "plugin" {
			t.Fatalf("install returned no plugin: %+v", result.Output)
		}
		wire, err := json.Marshal(value)
		if err != nil {
			t.Fatal(err)
		}
		if attempt == 1 && !bytes.Equal(installedWire, wire) {
			t.Fatal("replayed installation changed its recorded result")
		}
		installed = value
		installedWire = wire
	}
	pin := protocol.NewRequest(node.ID(), protocol.Command{Kind: "set_plugin_pinned", Data: protocol.CommandSetPluginPinnedPayload{Name: "gomoku", ExpectedRevision: installed.Summary.Revision, Pinned: true}})
	var pinned protocol.PluginInfo
	var pinnedWire []byte
	for attempt := range 2 {
		result := dispatchFixture(t, node, node.ID(), pin)
		if result.Fault != nil {
			t.Fatal(result.Fault)
		}
		value, valid := dispatchPayload[protocol.PluginInfo](result.Output.Data)
		if !valid || !value.Summary.Pinned || value.Summary.Revision != installed.Summary.Revision+1 {
			t.Fatalf("pin was applied more than once: %+v", result.Output)
		}
		wire, err := json.Marshal(value)
		if err != nil {
			t.Fatal(err)
		}
		if attempt == 1 && !bytes.Equal(pinnedWire, wire) {
			t.Fatal("replayed pin changed its recorded result")
		}
		pinned = value
		pinnedWire = wire
	}
	stale := protocol.NewRequest(node.ID(), protocol.Command{Kind: "install_catalog_plugin", Data: protocol.CommandInstallCatalogPluginPayload{
		Selection: protocol.PluginCatalogSelection{Info: info}, ExpectedRevision: installed.Summary.Revision, Mode: protocol.PluginInstallModeSwitch,
	}})
	if result := dispatchFixture(t, node, node.ID(), stale); result.Fault == nil || result.Fault.Code != protocol.ErrorCodeRevisionConflict {
		t.Fatalf("stale version selection was accepted: %+v", result)
	}
	if err := node.profile.Store.Node.Read(t.Context(), func(db *sql.Conn) error {
		current, err := store.ReadPlugin(t.Context(), db, "gomoku")
		if err == nil && (current == nil || !reflect.DeepEqual(current.Summary, pinned.Summary)) {
			t.Error("revision conflict changed the installed package")
		}
		return err
	}); err != nil {
		t.Fatal(err)
	}
}

func TestUploadedPluginUpdateUsesExecutingNode(t *testing.T) {
	node := nodeFixture(t)
	current := protocol.DefaultPluginInfo()
	version := "1.0.0"
	current.Summary.Name, current.Summary.Version, current.Summary.Digest = "uploaded-version-fixture", &version, strings.Repeat("a", 64)
	current.Origin = &protocol.PluginOrigin{Kind: "directory"}
	if err := node.profile.Store.Node.Write(t.Context(), func(db *sql.Conn) error {
		var err error
		current, err = store.InstallPlugin(t.Context(), db, current, 0, true)
		return err
	}); err != nil {
		t.Fatal(err)
	}
	for _, scenario := range []struct {
		name, version, api, minimum string
		invalid, available, fault   bool
	}{
		{name: "compatible ordinary package", version: "1.1.0", available: true},
		{name: "compatible platform", version: "1.1.0", api: "v1", minimum: "0.0.1", available: true},
		{name: "same precedence", version: "1.0.0+build.2"},
		{name: "older", version: "0.9.0"},
		{name: "prerelease", version: "1.1.0-rc.1", api: "v1"},
		{name: "unsupported API", version: "1.1.0", api: "v999"},
		{name: "newer Node required", version: "1.1.0", api: "v1", minimum: "999.0.0"},
		{name: "invalid declaration", version: "1.1.0", api: "v1", invalid: true},
		{name: "different identity", version: "1.1.0", fault: true},
	} {
		t.Run(scenario.name, func(t *testing.T) {
			manifest := map[string]any{"$schema": plugins.ManifestSchema, "name": current.Summary.Name, "version": scenario.version}
			if scenario.fault {
				manifest["name"] = "another-fixture"
			}
			if scenario.api != "" {
				extension := map[string]any{"api_version": scenario.api}
				if scenario.minimum != "" {
					extension["min_sailry_version"] = scenario.minimum
				}
				if scenario.invalid {
					extension["host"] = false
				}
				manifest["extensions"] = map[string]any{"dev.sailry.platform": extension}
			}
			data, err := json.Marshal(manifest)
			if err != nil {
				t.Fatal(err)
			}
			directory := t.TempDir()
			if err := os.WriteFile(filepath.Join(directory, "plugin.json"), data, 0o600); err != nil {
				t.Fatal(err)
			}
			data, _, _, err = plugins.NewManager(t.TempDir()).SnapshotSkills(t.Context(), directory, "")
			if err != nil {
				t.Fatal(err)
			}
			upload, err := node.transfers.StagePluginUpload(t.Context(), node.ID(), data)
			if err != nil {
				t.Fatal(err)
			}
			request := protocol.NewRequest(node.ID(), protocol.Command{Kind: "check_plugin_update", Data: protocol.CommandCheckPluginUpdatePayload{
				Name: current.Summary.Name, ExpectedRevision: current.Summary.Revision, Upload: &upload.Stream,
			}})
			result := dispatchFixture(t, node, node.ID(), request)
			if scenario.fault {
				if result.Fault == nil || result.Fault.Code != protocol.ErrorCodeInvalidRequest {
					t.Fatalf("different uploaded identity was accepted: %+v", result)
				}
				return
			}
			report, valid := dispatchPayload[protocol.PluginUpdatesReport](result.Output.Data)
			if result.Fault != nil || !valid || (report.Available != nil) != scenario.available || report.Blocked != nil {
				t.Fatalf("incorrect executing-Node assessment: %+v", result)
			}
		})
	}
}

func TestPluginNodeInstallRetainsGitUpdateBranch(t *testing.T) {
	node := nodeFixture(t)
	directory := t.TempDir()
	repository, err := git.PlainInit(directory, false)
	if err != nil {
		t.Fatal(err)
	}
	worktree, err := repository.Worktree()
	if err != nil {
		t.Fatal(err)
	}
	publish := func(version string) {
		t.Helper()
		manifest := `{"$schema":"` + plugins.ManifestSchema + `","name":"git-version-fixture","version":"` + version + `"}`
		if err := os.WriteFile(filepath.Join(directory, "plugin.json"), []byte(manifest), 0o600); err != nil {
			t.Fatal(err)
		}
		if _, err := worktree.Add("plugin.json"); err != nil {
			t.Fatal(err)
		}
		if _, err := worktree.Commit("Release "+version, &git.CommitOptions{Author: &object.Signature{Name: "Fixture", Email: "fixture@example.test", When: time.Unix(0, 0)}}); err != nil {
			t.Fatal(err)
		}
	}
	publish("1.0.0")
	head, err := repository.Head()
	if err != nil {
		t.Fatal(err)
	}
	branch := head.Name().String()
	inspect := protocol.NewRequest(node.ID(), protocol.Command{Kind: "inspect_plugin_source", Data: protocol.CommandInspectPluginSourcePayload{
		Source: protocol.PluginSkillsSource{Repository: directory, GitRef: &branch},
	}})
	result := dispatchFixture(t, node, node.ID(), inspect)
	selection, valid := dispatchPayload[protocol.PluginCatalogSelection](result.Output.Data)
	if result.Fault != nil || !valid {
		t.Fatalf("inspect fixture repository: %v", result.Fault)
	}
	install := protocol.NewRequest(node.ID(), protocol.Command{Kind: "install_plugin_source", Data: protocol.CommandInstallPluginSourcePayload{Source: selection.Source, Path: selection.Path, Name: "git-version-fixture"}})
	result = dispatchFixture(t, node, node.ID(), install)
	installed, valid := dispatchPayload[protocol.PluginInfo](result.Output.Data)
	if result.Fault != nil || !valid {
		t.Fatalf("install fixture repository: %v", result.Fault)
	}
	for _, version := range []string{"1.1.0", "1.2.0"} {
		update, valid := dispatchPayload[protocol.PluginOriginOnlinePayload](installed.UpdateSource.Data)
		origin, captured := dispatchPayload[protocol.PluginOriginOnlinePayload](installed.Origin.Data)
		if !valid || !captured || update.Source.GitRef != branch || origin.Source.GitRef != origin.Source.Commit {
			t.Fatalf("installed tracking branch became a content commit: %+v %+v", update, origin)
		}
		publish(version)
		check := protocol.NewRequest(node.ID(), protocol.Command{Kind: "check_plugin_update", Data: protocol.CommandCheckPluginUpdatePayload{Name: installed.Summary.Name, ExpectedRevision: installed.Summary.Revision, Force: true}})
		result = dispatchFixture(t, node, node.ID(), check)
		report, valid := dispatchPayload[protocol.PluginUpdatesReport](result.Output.Data)
		if result.Fault != nil || !valid || report.Available == nil {
			t.Fatalf("branch update %s was not discovered: %v", version, result.Fault)
		}
		candidate, valid := dispatchPayload[protocol.PluginInfo](report.Available.Data)
		if !valid || candidate.Summary.Version == nil || *candidate.Summary.Version != version {
			t.Fatalf("unexpected branch update: %+v", report)
		}
		upstream, valid := dispatchPayload[protocol.PluginOriginOnlinePayload](candidate.UpdateSource.Data)
		if !valid {
			t.Fatal("branch update lost its source")
		}
		install = protocol.NewRequest(node.ID(), protocol.Command{Kind: "install_plugin_source", Data: protocol.CommandInstallPluginSourcePayload{Source: upstream.Source, Path: upstream.Path, Name: candidate.Summary.Name, ExpectedRevision: installed.Summary.Revision}})
		result = dispatchFixture(t, node, node.ID(), install)
		installed, valid = dispatchPayload[protocol.PluginInfo](result.Output.Data)
		if result.Fault != nil || !valid {
			t.Fatalf("install branch update: %v", result.Fault)
		}
	}
}
