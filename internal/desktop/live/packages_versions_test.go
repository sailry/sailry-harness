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
	if settings.Update(&row) {
		t.Fatal("an uninspected official recommendation became actionable")
	}
	selected := info
	selected.Summary.Version = &compatible.Version
	selected.ReleaseNotes, selected.Versions = compatible.ReleaseNotes, entry.Versions
	report.Available = &protocol.PluginUpdatesCandidate{Kind: "package", Data: selected}
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
					r.packages.updates["notes"] = protocol.PluginUpdatesReport{Current: current.Summary, Available: &protocol.PluginUpdatesCandidate{Kind: "package", Data: selected}}
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
				// Only queued command projections run here. Shared raster workers
				// cannot join a synctest bubble, so keep this surface unrendered.
				tester := ui.NewTesterWith(func(c *ui.Context) {
					r.mu.Lock()
					queue := r.queue
					r.queue = nil
					r.mu.Unlock()
					for _, run := range queue {
						run(c)
					}
				}, 1, 1, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true}})
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

func TestOnlinePluginUpdateKeepsMatchingUpstream(t *testing.T) {
	for _, test := range []struct {
		name, mismatch  string
		skill, reimport bool
	}{
		{name: "plugin branch"},
		{name: "skill branch", skill: true},
		{name: "skill reimport branch", skill: true, reimport: true},
		{name: "different repository", mismatch: "repository"},
		{name: "different commit", mismatch: "commit"},
		{name: "different path", mismatch: "path"},
		{name: "missing update source", mismatch: "missing"},
	} {
		t.Run(test.name, func(t *testing.T) {
			synctest.Test(t, func(t *testing.T) {
				r, transport := terminalTabsFixture(t)
				current := protocol.PluginInfo{Summary: protocol.PluginSummary{Name: "notes", Digest: "current", Version: new("1.0.0"), Revision: 7, Compatibility: protocol.PluginCompatibility{Kind: "compatible"}}}
				origin := protocol.PluginOriginOnlinePayload{Source: protocol.PluginSkillsResolved{Repository: "https://github.com/example/plugins", GitRef: "selected-commit", Commit: "selected-commit"}, Path: "notes"}
				candidate := current
				candidate.Summary.Version = new("1.1.0")
				candidate.Origin = &protocol.PluginOrigin{Kind: "online", Data: origin}
				upstream := origin
				upstream.Source.GitRef = "release"
				switch test.mismatch {
				case "repository":
					upstream.Source.Repository = "https://github.com/other/plugins"
				case "commit":
					upstream.Source.Commit = "different-commit"
				case "path":
					upstream.Path = "other"
				}
				if test.mismatch != "missing" {
					candidate.UpdateSource = &protocol.PluginOrigin{Kind: "online", Data: upstream}
				}
				if test.skill {
					candidate.Skill = &protocol.PluginSkillsProvenance{Kind: "Git", Data: protocol.PluginSkillsGitProvenance{Source: origin.Source, Path: origin.Path}}
				}
				if test.reimport {
					r.packages.infos["notes"] = candidate
				} else {
					r.packages.infos["notes"] = current
				}
				r.packages.updates["notes"] = protocol.PluginUpdatesReport{Current: current.Summary, Available: &protocol.PluginUpdatesCandidate{Kind: "package", Data: candidate}}
				transport.respond = func(request protocol.Request) link.Response {
					if request.Command.Kind == "discover_skills" {
						return link.Response{Output: protocol.Output{Kind: "skill_discovery", Data: protocol.PluginSkillsDiscovery{Source: origin.Source}}}
					}
					return link.Response{Output: protocol.Output{Kind: "plugin", Data: current}}
				}
				if test.reimport {
					r.reimportSkill("notes")
				} else {
					r.updatePackage("notes", func() {})
				}
				synctest.Wait()
				if test.reimport {
					commands := transport.sent("discover_skills")
					if len(commands) != 1 {
						t.Fatalf("skill reimport commands: %+v", commands)
					}
					input, _ := detail[protocol.CommandDiscoverSkillsPayload](commands[0].Data)
					if input.Source.Repository != origin.Source.Repository || stringValue(input.Source.GitRef) != upstream.Source.GitRef || stringValue(input.Source.Path) != origin.Path {
						t.Fatalf("reimport lost its original upstream: %+v", input.Source)
					}
					return
				}
				kind := "install_plugin_source"
				if test.skill {
					kind = "install_skill"
				}
				commands := transport.sent(kind)
				if len(commands) != 1 {
					t.Fatalf("online update commands: %+v", commands)
				}
				var source protocol.PluginSkillsResolved
				var path string
				var revision uint64
				if test.skill {
					input, _ := detail[protocol.CommandInstallSkillPayload](commands[0].Data)
					source, path, revision = input.Source, input.Path, input.ExpectedRevision
				} else {
					input, _ := detail[protocol.CommandInstallPluginSourcePayload](commands[0].Data)
					source, path, revision = input.Source, input.Path, input.ExpectedRevision
				}
				expected := origin.Source
				if test.mismatch == "" {
					expected.GitRef = upstream.Source.GitRef
				}
				if source != expected || path != origin.Path || revision != current.Summary.Revision {
					t.Fatalf("update changed its inspected identity or lost its matching branch: source %+v, path %q, revision %d", source, path, revision)
				}
			})
		})
	}
}

func TestInstalledPluginDetailsKeepReleaseMetadata(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		r, source := terminalTabsFixture(t)
		notes := &protocol.PluginCatalogReleaseNotes{Text: "Installed release notes", Locales: map[string]string{"zh-CN": "已安装版本说明"}}
		info := protocol.PluginInfo{Summary: protocol.PluginSummary{Name: "notes", Digest: "installed", Version: new("1.0.0"), Revision: 7, Pinned: true, Compatibility: protocol.PluginCompatibility{Kind: "compatible"}},
			Origin: &protocol.PluginOrigin{Kind: "bundled"}, UpdateSource: &protocol.PluginOrigin{Kind: "official"}, ReleaseNotes: notes,
			Versions: []protocol.PluginCatalogVersion{{Version: "1.0.0", ReleaseNotes: notes, Compatibility: protocol.PluginCompatibility{Kind: "compatible"}}}}
		r.packages.infos[info.Summary.Name] = info
		latest := protocol.PluginCatalogVersion{Version: "2.0.0", Compatibility: protocol.PluginCompatibility{Kind: "compatible"}}
		source.respond = func(request protocol.Request) link.Response {
			switch request.Command.Kind {
			case "read_plugin_version":
				captured := info
				captured.Summary.Revision, captured.Summary.Pinned = 0, false
				captured.ReleaseNotes, captured.Versions = nil, nil
				captured.Skills = []protocol.PluginSkill{{Name: "packaged-skill"}}
				return link.Response{Output: protocol.Output{Kind: "plugin", Data: captured}}
			case "search_plugin_catalog":
				return link.Response{Output: protocol.Output{Kind: "plugin_catalog", Data: protocol.PluginCatalogPage{Page: 1, Pages: 1,
					Entries: []protocol.PluginCatalogEntry{{ID: info.Summary.Name, Version: &latest.Version, CompatibleVersion: &latest, Compatibility: latest.Compatibility, Versions: []protocol.PluginCatalogVersion{latest}}}}}}
			default:
				return link.Response{Fault: &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "unexpected fixture command"}}
			}
		}
		done := false
		r.readInstalledPackageDetails(info, func() { done = true })
		synctest.Wait()
		r.mu.Lock()
		queue := r.queue
		r.queue = nil
		r.mu.Unlock()
		for _, run := range queue {
			run(nil)
		}
		loaded := r.packages.infos[info.Summary.Name]
		if !done || !loaded.Summary.Pinned || loaded.Summary.Revision != info.Summary.Revision || loaded.ReleaseNotes == nil || loaded.ReleaseNotes.Text != notes.Text || len(loaded.Versions) != 1 || len(loaded.Skills) != 1 {
			t.Fatalf("immutable details lost installed metadata or package content: %+v", loaded)
		}
		view := packageView(loaded, "zh-CN")
		applyPackageCatalog(&view, r.packages.catalog[info.Summary.Name], "zh-CN")
		last := view.Versions[len(view.Versions)-1]
		if last.Version != "1.0.0" || last.Notes != "已安装版本说明" {
			t.Fatalf("current version disappeared with its catalog entry: %+v", view.Versions)
		}
	})
}
