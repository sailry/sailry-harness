package live

import (
	"testing"
	"testing/synctest"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestPluginVersionProjection(t *testing.T) {
	t.Parallel()
	latest, current := "2.0.0", "1.0.0"
	compatible := protocol.PluginCatalogVersion{Version: "1.2.0", Commit: "compatible", Compatibility: protocol.PluginCompatibility{Kind: "compatible"},
		ReleaseNotes: &protocol.PluginCatalogReleaseNotes{Text: "Compatible notes", Locales: map[string]string{"zh-CN": "兼容版本说明"}}}
	entry := protocol.PluginCatalogEntry{ID: "notes", Name: "Notes", Version: &latest, CompatibleVersion: &compatible,
		Compatibility: protocol.PluginCompatibility{Kind: "unsupported", Data: protocol.PluginCompatibilityUnsupportedPayload{Reason: "Unsupported plugin API v2"}},
		Versions: []protocol.PluginCatalogVersion{
			{Version: latest, Compatibility: protocol.PluginCompatibility{Kind: "unsupported", Data: protocol.PluginCompatibilityUnsupportedPayload{Reason: "Unsupported plugin API v2"}}},
			compatible,
			{Version: "0.9.0", Withdrawn: true, Compatibility: protocol.PluginCompatibility{Kind: "compatible"}, ReleaseNotes: &protocol.PluginCatalogReleaseNotes{Text: "Historical notes"}},
		}}
	market := catalogPackageView(entry, "zh-CN", 0)
	if !market.Installable || market.Version != compatible.Version || market.MarketVersion != latest || market.Requires != "" || market.Unsupported != "" {
		t.Fatalf("compatible catalog default is %+v", market)
	}
	if market.Versions[1].Notes != "兼容版本说明" || market.Versions[2].Notes != "Historical notes" || !market.Versions[2].Withdrawn || market.Versions[0].Compatible || market.Versions[0].Unsupported == "" {
		t.Fatalf("version-specific catalog data is %+v", market.Versions)
	}
	info := protocol.PluginInfo{Summary: protocol.PluginSummary{Name: entry.ID, Version: &current, Digest: "installed", Revision: 7, SettingsRevision: 3, Compatibility: protocol.PluginCompatibility{Kind: "compatible"}},
		UpdateSource: &protocol.PluginOrigin{Kind: "official"}, Origin: &protocol.PluginOrigin{Kind: "online"}, ReleaseNotes: &protocol.PluginCatalogReleaseNotes{Text: "Current notes"}}
	row := packageView(info, "en")
	applyPackageCatalog(&row, entry, "en")
	if settings.Update(&row) || row.Latest != "" || row.MarketVersion != latest || !row.Official {
		t.Fatalf("catalog metadata creates an update: %+v", row)
	}
	report := protocol.PluginUpdatesReport{Current: info.Summary, Available: &protocol.PluginUpdatesCandidate{Kind: "official", Data: entry}}
	applyPackageUpdate(&row, info, report, "en")
	if !settings.Update(&row) || row.Latest != compatible.Version || row.Notes != "Compatible notes" || row.Unsupported != "" {
		t.Fatalf("Node's compatible update is %+v", row)
	}
	if tail := row.Versions[len(row.Versions)-1]; tail.Version != current || tail.Notes != "Current notes" {
		t.Fatalf("an unlisted current version lost its own release notes: %+v", tail)
	}
	for _, state := range []string{"pinned", "blocked", "stale"} {
		t.Run(state, func(t *testing.T) {
			candidateInfo, candidateReport := info, report
			switch state {
			case "pinned":
				candidateInfo.Summary.Pinned = true
			case "blocked":
				candidateReport.Blocked = &protocol.PluginUpdatesBlocked{Version: latest, Required: "9.0.0"}
			case "stale":
				candidateReport.Current.Revision--
			}
			view := packageView(candidateInfo, "en")
			applyPackageUpdate(&view, candidateInfo, candidateReport, "en")
			if settings.Update(&view) || view.Latest != "" {
				t.Fatalf("%s report remains actionable: %+v", state, view)
			}
		})
	}
	entry.CompatibleVersion = nil
	if view := catalogPackageView(entry, "en", 0); view.Installable || view.Unsupported == "" {
		t.Fatalf("unsupported catalog without compatible history is installable: %+v", view)
	}
}

func TestOfficialPluginVersionCommands(t *testing.T) {
	for _, test := range []struct {
		name    string
		version *string
		bundled bool
		current bool
		update  bool
	}{
		{name: "compatible default"},
		{name: "historical online switch while pinned", version: new("1.0.0"), current: true},
		{name: "historical bundled install", version: new("1.0.0"), bundled: true},
		{name: "recommended prerelease update", version: new("1.1.0-rc.1"), current: true, update: true},
	} {
		t.Run(test.name, func(t *testing.T) {
			synctest.Test(t, func(t *testing.T) {
				r, source := terminalTabsFixture(t)
				selectedVersion := "1.0.0"
				if test.version != nil {
					selectedVersion = *test.version
				}
				selected := protocol.PluginInfo{Summary: protocol.PluginSummary{Name: "notes", Digest: "selected-digest", Version: &selectedVersion, Compatibility: protocol.PluginCompatibility{Kind: "compatible"}},
					UpdateSource: &protocol.PluginOrigin{Kind: "official"}, Origin: &protocol.PluginOrigin{Kind: "bundled"}}
				resolved := protocol.PluginSkillsResolved{Repository: "https://github.com/sailry/plugins", GitRef: "release", Commit: "immutable-selected-commit"}
				if !test.bundled {
					selected.Origin = &protocol.PluginOrigin{Kind: "online", Data: protocol.PluginOriginOnlinePayload{Source: resolved, Path: "plugins/notes"}}
				}
				r.packages.catalog["notes"] = protocol.PluginCatalogEntry{ID: "notes", Version: new("2.0.0"),
					Compatibility:     protocol.PluginCompatibility{Kind: "requires_newer", Data: protocol.PluginCompatibilityRequiresNewerPayload{Required: "9.0.0"}},
					CompatibleVersion: &protocol.PluginCatalogVersion{Version: selectedVersion, Compatibility: protocol.PluginCompatibility{Kind: "compatible"}}, Bundled: test.bundled}
				r.packages.catalogSources["notes"] = protocol.PluginCatalogSourceOfficial
				if test.current {
					current := selected
					current.Summary.Version, current.Summary.Revision, current.Summary.Pinned = new("1.2.0"), 7, !test.update
					if test.update {
						current.Summary.Version = new("1.0.0-rc.1")
					}
					r.packages.infos["notes"] = current
					r.packages.updates["notes"] = protocol.PluginUpdatesReport{Current: current.Summary, Available: &protocol.PluginUpdatesCandidate{Kind: "official", Data: r.packages.catalog["notes"]}}
				}
				source.respond = func(request protocol.Request) link.Response {
					switch request.Command.Kind {
					case "read_catalog_plugin_info":
						return link.Response{Output: protocol.Output{Kind: "catalog_details", Data: protocol.PluginCatalogDetails{Kind: "plugin", Data: selected}}}
					case "install_catalog_plugin":
						installed := selected
						installed.Summary.Revision, installed.Summary.Pinned = 8, test.current && !test.update
						return link.Response{Output: protocol.Output{Kind: "plugin", Data: installed}}
					default:
						return link.Response{Fault: &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "unexpected fixture command"}}
					}
				}
				tester := ui.NewTester(func(c *ui.Context) {
					r.mu.Lock()
					queue := r.queue
					r.queue = nil
					r.mu.Unlock()
					for _, run := range queue {
						run(c)
					}
				}, 800, 600)
				if test.update {
					r.updatePackages([]string{"notes"})
				} else if test.version == nil {
					r.installMarketPlugin("notes")
				} else {
					if test.current {
						r.updatePackages([]string{"notes"})
						synctest.Wait()
						if len(source.sent("read_catalog_plugin_info")) != 0 {
							t.Fatal("a pinned package prepared an automatic update")
						}
					}
					r.installPackageVersion("notes", *test.version)
				}
				synctest.Wait()
				tester.Frame()
				synctest.Wait()
				tester.Frame()
				reads := source.sent("read_catalog_plugin_info")
				if len(reads) != 1 {
					t.Fatalf("catalog inspections: %+v", reads)
				}
				inspection, _ := detail[protocol.CommandReadCatalogPluginInfoPayload](reads[0].Data)
				if inspection.Source != protocol.PluginCatalogSourceOfficial || inspection.ID != "notes" || stringValue(inspection.Version) != stringValue(test.version) {
					t.Fatalf("inspection changed the requested release: %+v", inspection)
				}
				commands := source.sent("install_catalog_plugin")
				if len(commands) != 1 || len(source.sent("install_plugin_source")) != 0 || len(source.sent("install_bundled_plugin")) != 0 {
					t.Fatalf("official version bypassed catalog installation: %+v", commands)
				}
				install, _ := detail[protocol.CommandInstallCatalogPluginPayload](commands[0].Data)
				if install.Selection.Info.Summary.Digest != selected.Summary.Digest || stringValue(install.Selection.Info.Summary.Version) != selectedVersion {
					t.Fatalf("installation lost its inspected version: %+v", install)
				}
				mode := protocol.PluginInstallModeInstall
				revision := uint64(0)
				if test.current {
					mode, revision = protocol.PluginInstallModeSwitch, 7
				}
				if test.update {
					mode = protocol.PluginInstallModeUpdate
				}
				if install.Mode != mode || install.ExpectedRevision != revision {
					t.Fatalf("installation changed mode or revision: %+v", install)
				}
				if test.bundled {
					if install.Selection.Source != (protocol.PluginSkillsResolved{}) || install.Selection.Path != "" {
						t.Fatalf("bundled selection invented an online source: %+v", install.Selection)
					}
				} else if install.Selection.Source != resolved || install.Selection.Path != "plugins/notes" {
					t.Fatalf("online selection lost its immutable source: %+v", install.Selection)
				}
				if r.App.Settings.Busy[settings.BusyPackage+"notes"] || r.packages.infos["notes"].Summary.Pinned != (test.current && !test.update) {
					t.Fatal("installation lost the Node's pin choice or stayed busy")
				}
			})
		})
	}
}
