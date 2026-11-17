package plugins

import (
	"encoding/json"
	"errors"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"testing"
	"testing/fstest"
)

func bundledWithdrawalFixture(t *testing.T, name, version string, withdrawn bool) fstest.MapFS {
	t.Helper()
	manifest := []byte(`{"$schema":"` + ManifestSchema + `","name":"withdrawal-fixture","version":"1.0.0"}`)
	catalog, err := json.Marshal(map[string]any{
		"version": 1,
		"packages": []any{map[string]any{
			"id": name, "versions": []any{map[string]any{"version": version, "withdrawn": withdrawn}},
		}},
	})
	if err != nil {
		t.Fatal(err)
	}
	return fstest.MapFS{
		"plugins/catalog.json":                   {Data: catalog},
		"plugins/withdrawal-fixture/plugin.json": {Data: manifest},
	}
}

func TestBundledWithdrawals(t *testing.T) {
	t.Parallel()
	server := httptest.NewServer(http.HandlerFunc(func(writer http.ResponseWriter, request *http.Request) {
		writer.WriteHeader(http.StatusServiceUnavailable)
	}))
	defer server.Close()
	for _, current := range []struct {
		label, name, version string
		withdrawn            bool
		blocked              bool
	}{
		{"matching release", "withdrawal-fixture", "1.0.0", true, true},
		{"available release", "withdrawal-fixture", "1.0.0", false, false},
		{"different version", "withdrawal-fixture", "0.9.0", true, false},
		{"different package", "another-fixture", "1.0.0", true, false},
	} {
		t.Run(current.label, func(t *testing.T) {
			manager := NewManager(t.TempDir())
			manager.OfficialURL = server.URL
			manager.bundled = bundledWithdrawalFixture(t, current.name, current.version, current.withdrawn)
			info, err := manager.InspectBundled(t.Context(), "withdrawal-fixture")
			if err != nil || IsWithdrawn(info) != current.blocked {
				t.Fatalf("bundled inspection: %+v %v", info, err)
			}
			if InfoCompatibility(info).Kind != "compatible" {
				t.Fatal("withdrawal disabled previously installed or captured content")
			}
			entries, err := manager.OfficialEntries(t.Context(), false)
			if err != nil || len(entries) != 1 || entries[0].Versions[0].Withdrawn != current.blocked || (entries[0].CompatibleVersion == nil) != current.blocked {
				t.Fatalf("offline catalog: %+v %v", entries, err)
			}
			for _, requested := range []*string{nil, versionText("1.0.0")} {
				_, err := manager.InspectOfficial(t.Context(), "withdrawal-fixture", requested, false)
				if (err != nil) != current.blocked {
					t.Fatalf("official selection: %v", err)
				}
			}
			installed, err := manager.InstallBundled(t.Context(), "withdrawal-fixture")
			if (err != nil) != current.blocked {
				t.Fatalf("direct bundled installation: %+v %v", installed, err)
			}
			if current.blocked {
				if _, err := os.Stat(filepath.Join(manager.profile, "plugins", "packages")); !errors.Is(err, os.ErrNotExist) {
					t.Fatalf("withdrawn content was published: %v", err)
				}
			} else if installed.Summary.Digest != info.Summary.Digest || IsWithdrawn(installed) {
				t.Fatalf("available content was changed: %+v", installed)
			}
		})
	}
}

func TestBundledSelectionRechecksLocalWithdrawal(t *testing.T) {
	t.Parallel()
	server := httptest.NewServer(http.HandlerFunc(func(writer http.ResponseWriter, request *http.Request) {
		writer.WriteHeader(http.StatusServiceUnavailable)
	}))
	defer server.Close()
	manager := NewManager(t.TempDir())
	manager.OfficialURL = server.URL
	manager.bundled = bundledWithdrawalFixture(t, "withdrawal-fixture", "1.0.0", false)
	selection, err := manager.InspectOfficial(t.Context(), "withdrawal-fixture", nil, false)
	if err != nil {
		t.Fatal(err)
	}
	manager.bundled = bundledWithdrawalFixture(t, "withdrawal-fixture", "1.0.0", true)
	if _, err := manager.InstallOfficial(t.Context(), selection); err == nil {
		t.Fatal("offline installation bypassed the embedded withdrawal")
	}
	if _, err := os.Stat(filepath.Join(manager.profile, "plugins", "packages")); !errors.Is(err, os.ErrNotExist) {
		t.Fatalf("withdrawn selection was published: %v", err)
	}
}
