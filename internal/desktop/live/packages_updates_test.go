package live

import (
	"strings"
	"testing"
	"testing/synctest"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/link"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func officialUpdateFixture() (protocol.PluginInfo, protocol.PluginInfo, protocol.PluginCatalogEntry) {
	current := protocol.DefaultPluginInfo()
	current.Summary.Name, current.Summary.Version, current.Summary.Digest, current.Summary.Revision = "notes", new("1.0.0"), "installed", 7
	current.UpdateSource, current.Origin = &protocol.PluginOrigin{Kind: "official"}, &protocol.PluginOrigin{Kind: "bundled"}
	current.Extension = &protocol.PluginExtension{ApiVersion: "v1", Actions: []protocol.PluginAction{protocol.PluginActionReadFiles}}
	selected := current
	selected.Summary.Version, selected.Summary.Digest, selected.Summary.Revision = new("1.5.0"), "selected", 0
	selected.Extension = &protocol.PluginExtension{ApiVersion: "v1", Actions: []protocol.PluginAction{protocol.PluginActionReadFiles, protocol.PluginActionWriteFiles}}
	commit := strings.Repeat("a", 40)
	selected.Origin = &protocol.PluginOrigin{Kind: "online", Data: protocol.PluginOriginOnlinePayload{
		Source: protocol.PluginSkillsResolved{Repository: "https://github.com/sailry/sailry-plugins.git", GitRef: commit, Commit: commit}, Path: "notes",
	}}
	selected.ReleaseNotes = &protocol.PluginCatalogReleaseNotes{Text: "Selected release notes"}
	entry := protocol.PluginCatalogEntry{ID: "notes", Version: new("2.0.0"), Permissions: []protocol.PluginAction{protocol.PluginActionReadFiles},
		CompatibleVersion: &protocol.PluginCatalogVersion{Version: "1.5.0", Commit: commit, Compatibility: protocol.PluginCompatibility{Kind: "compatible"}}}
	return current, selected, entry
}

func TestResolvedPluginUpdateUsesSelectedManifest(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		r, source := terminalTabsFixture(t)
		current, selected, entry := officialUpdateFixture()
		source.respond = func(request protocol.Request) link.Response {
			if request.Command.Kind != "read_catalog_plugin_info" {
				t.Fatalf("unexpected command %s", request.Command.Kind)
			}
			input, _ := detail[protocol.CommandReadCatalogPluginInfoPayload](request.Command.Data)
			if input.ID != selected.Summary.Name || stringValue(input.Version) != "1.5.0" {
				t.Fatalf("inspection did not select the compatible release: %+v", input)
			}
			return link.Response{Output: protocol.Output{Kind: "catalog_details", Data: protocol.PluginCatalogDetails{Kind: "plugin", Data: selected}}}
		}
		report := protocol.PluginUpdatesReport{Current: current.Summary, Available: &protocol.PluginUpdatesCandidate{Kind: "official", Data: entry}}
		batch := protocol.PluginUpdatesBatch{Reports: []protocol.PluginUpdatesReport{report, {Current: protocol.PluginSummary{Name: "no-update"}}}}
		r.resolvePackageUpdates(&batch, nil)
		if len(batch.Failures) != 0 || len(source.sent("read_catalog_plugin_info")) != 1 {
			t.Fatalf("only the actual candidate should be inspected: %+v", batch)
		}
		row := packageView(current, "en")
		applyPackageUpdate(&row, current, batch.Reports[0], "en")
		if !settings.Update(&row) || row.Latest != "1.5.0" || row.Notes != selected.ReleaseNotes.Text ||
			len(row.AddedPermissions) != 1 || row.AddedPermissions[0] != packagePermissionLabels[protocol.PluginActionWriteFiles] {
			t.Fatalf("update review used the market latest rather than the inspected release: %+v", row)
		}
		r.packages.updates[current.Summary.Name] = batch.Reports[0]
		batch.Reports[0] = report
		r.resolvePackageUpdates(&batch, r.packageUpdateCache())
		if len(source.sent("read_catalog_plugin_info")) != 1 || !packageUpdateAvailable(batch.Reports[0]) {
			t.Fatal("the same immutable recommendation was downloaded again or lost")
		}
	})
}

func TestPluginUpdatesRespectLocalDesktopRequirements(t *testing.T) {
	for _, desktop := range []bool{false, true} {
		for _, official := range []bool{false, true} {
			t.Run(strings.Join([]string{map[bool]string{false: "Node", true: "Desktop"}[desktop], map[bool]string{false: "captured", true: "official"}[official]}, "/"), func(t *testing.T) {
				synctest.Test(t, func(t *testing.T) {
					r, source := terminalTabsFixture(t)
					current, selected, entry := officialUpdateFixture()
					selected.Requirements = &protocol.PluginRequirements{APIVersion: "v1", MinSailryVersion: new("999.0.0")}
					if desktop {
						selected.Extension.Desktop = &protocol.PluginDesktopManifest{Entry: new("desktop/main.js")}
					}
					source.respond = func(request protocol.Request) link.Response {
						return link.Response{Output: protocol.Output{Kind: "catalog_details", Data: protocol.PluginCatalogDetails{Kind: "plugin", Data: selected}}}
					}
					available := &protocol.PluginUpdatesCandidate{Kind: "package", Data: selected}
					if official {
						available = &protocol.PluginUpdatesCandidate{Kind: "official", Data: entry}
					}
					batch := protocol.PluginUpdatesBatch{Reports: []protocol.PluginUpdatesReport{{Current: current.Summary, Available: available}}}
					r.resolvePackageUpdates(&batch, nil)
					if len(batch.Failures) != 0 || packageUpdateAvailable(batch.Reports[0]) == desktop {
						t.Fatalf("local UI compatibility must only constrain Desktop declarations: %+v", batch)
					}
					row := packageView(current, "en")
					applyPackageUpdate(&row, current, batch.Reports[0], "en")
					if settings.Update(&row) == desktop {
						t.Fatalf("update badge disagrees with the reviewed candidate: %+v", row)
					}
				})
			})
		}
	}
}

func TestOfficialUpdateRechecksReviewedContent(t *testing.T) {
	for _, change := range []string{"none", "digest", "permissions", "desktop requirements"} {
		t.Run(change, func(t *testing.T) {
			synctest.Test(t, func(t *testing.T) {
				r, source := terminalTabsFixture(t)
				current, reviewed, _ := officialUpdateFixture()
				selected := reviewed
				extension := *reviewed.Extension
				selected.Extension = &extension
				switch change {
				case "digest":
					selected.Summary.Digest = "changed"
				case "permissions":
					selected.Extension.Actions = []protocol.PluginAction{protocol.PluginActionReadFiles}
				case "desktop requirements":
					selected.Extension.Desktop = &protocol.PluginDesktopManifest{Entry: new("desktop/main.js")}
					selected.Requirements = &protocol.PluginRequirements{APIVersion: "v1", MinSailryVersion: new("999.0.0")}
				}
				r.packages.infos[current.Summary.Name] = current
				r.packages.updates[current.Summary.Name] = protocol.PluginUpdatesReport{Current: current.Summary, Available: &protocol.PluginUpdatesCandidate{Kind: "package", Data: reviewed}}
				source.respond = func(request protocol.Request) link.Response {
					switch request.Command.Kind {
					case "read_catalog_plugin_info":
						return link.Response{Output: protocol.Output{Kind: "catalog_details", Data: protocol.PluginCatalogDetails{Kind: "plugin", Data: selected}}}
					case "install_catalog_plugin":
						installed := selected
						installed.Summary.Revision = current.Summary.Revision + 1
						return link.Response{Output: protocol.Output{Kind: "plugin", Data: installed}}
					default:
						return link.Response{Fault: &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "unexpected fixture command"}}
					}
				}
				tester := pluginUpdateQueueTester(r)
				r.updatePackages([]string{current.Summary.Name})
				synctest.Wait()
				tester.Frame()
				synctest.Wait()
				tester.Frame()
				installed := len(source.sent("install_catalog_plugin")) != 0
				if installed != (change == "none") || r.App.Settings.Busy[settings.BusyPackage+current.Summary.Name] {
					t.Fatalf("changed content was installed or operation stayed busy: installed=%v", installed)
				}
				if change != "none" && r.packages.infos[current.Summary.Name].Summary.Digest != current.Summary.Digest {
					t.Fatal("a refused update replaced the installed package")
				}
				if change == "digest" || change == "permissions" {
					row := packageView(current, "en")
					applyPackageUpdate(&row, current, r.packages.updates[current.Summary.Name], "en")
					if !settings.Update(&row) || change == "permissions" && len(row.AddedPermissions) != 0 {
						t.Fatalf("changed content was not returned for a fresh review: %+v", row)
					}
				}
			})
		})
	}
}

func TestDesktopRequirementsFenceExplicitInstallations(t *testing.T) {
	for _, route := range []string{"history", "Git", "upload", "bundled"} {
		t.Run(route, func(t *testing.T) {
			synctest.Test(t, func(t *testing.T) {
				r, source := terminalTabsFixture(t)
				current, selected, entry := officialUpdateFixture()
				selected.Requirements = &protocol.PluginRequirements{APIVersion: "v1", MinSailryVersion: new("999.0.0")}
				selected.Extension.Desktop = &protocol.PluginDesktopManifest{Entry: new("desktop/main.js")}
				r.packages.infos[current.Summary.Name] = current
				r.packages.catalog[entry.ID], r.packages.catalogSources[entry.ID] = entry, protocol.PluginCatalogSourceOfficial
				source.respond = func(request protocol.Request) link.Response {
					switch request.Command.Kind {
					case "read_catalog_plugin_info":
						return link.Response{Output: protocol.Output{Kind: "catalog_details", Data: protocol.PluginCatalogDetails{Kind: "plugin", Data: selected}}}
					case "inspect_plugin_source":
						selection, _ := officialPackageSelection(selected)
						return link.Response{Output: protocol.Output{Kind: "plugin_source", Data: selection}}
					case "cancel_file_transfer":
						return link.Response{Output: protocol.Output{Kind: "file_transfer_cancelled"}}
					default:
						return link.Response{Fault: &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "unexpected fixture command"}}
					}
				}
				done, succeeded := false, false
				complete := func(ok bool) { done, succeeded = true, ok }
				tester := pluginUpdateQueueTester(r)
				switch route {
				case "history":
					r.installPackageVersion(entry.ID, *selected.Summary.Version)
				case "Git":
					r.installGitPlugin("https://github.com/example/notes", "", "", complete)
				case "upload":
					r.installPackageUpload(localPackage{stream: protocol.StreamID{1}, info: selected}, complete)
				case "bundled":
					entry.Bundled, entry.Version = true, selected.Summary.Version
					r.packages.catalog[entry.ID] = entry
					r.installLocalPlugin(entry.ID, "", complete)
				}
				synctest.Wait()
				tester.Frame()
				synctest.Wait()
				tester.Frame()
				for _, kind := range []string{"install_catalog_plugin", "install_plugin_source", "install_plugin_upload", "install_bundled_plugin"} {
					if len(source.sent(kind)) != 0 {
						t.Fatalf("%s installed a package its Desktop cannot load", route)
					}
				}
				if route != "history" && (!done || succeeded) || r.App.Settings.Busy[settings.BusyPackage+entry.ID] {
					t.Fatal("refused installation did not complete as a failure")
				}
				if route == "upload" {
					cancelled := source.sent("cancel_file_transfer")
					if len(cancelled) != 1 {
						t.Fatal("refused installation retained its uploaded package")
					}
					input, _ := detail[protocol.CommandCancelFileTransferPayload](cancelled[0].Data)
					if input.Stream != (protocol.StreamID{1}) {
						t.Fatal("refused installation cancelled a different upload")
					}
				}
				if r.packages.infos[current.Summary.Name].Summary.Digest != current.Summary.Digest {
					t.Fatal("refused installation replaced the usable package")
				}
			})
		})
	}
}

func TestPluginUpdateInspectionFailureStaysFailure(t *testing.T) {
	for _, failure := range []string{"network", "different commit"} {
		t.Run(failure, func(t *testing.T) {
			synctest.Test(t, func(t *testing.T) {
				r, source := terminalTabsFixture(t)
				current, selected, entry := officialUpdateFixture()
				source.respond = func(request protocol.Request) link.Response {
					if failure == "network" {
						return link.Response{Fault: &protocol.Fault{Code: protocol.ErrorCodeUnavailable, Message: "fixture catalog unavailable"}}
					}
					origin, _ := detail[protocol.PluginOriginOnlinePayload](selected.Origin.Data)
					origin.Source.GitRef, origin.Source.Commit = strings.Repeat("b", 40), strings.Repeat("b", 40)
					selected.Origin = &protocol.PluginOrigin{Kind: "online", Data: origin}
					return link.Response{Output: protocol.Output{Kind: "catalog_details", Data: protocol.PluginCatalogDetails{Kind: "plugin", Data: selected}}}
				}
				batch := protocol.PluginUpdatesBatch{Reports: []protocol.PluginUpdatesReport{{Current: current.Summary,
					Available: &protocol.PluginUpdatesCandidate{Kind: "official", Data: entry}}}}
				catalogs := r.resolvePackageUpdates(&batch, nil)
				if len(batch.Failures) != 1 || batch.Failures[0].Name != current.Summary.Name || packageUpdateAvailable(batch.Reports[0]) {
					t.Fatalf("inspection failure became an actionable or successful update check: %+v", batch)
				}
				if stringValue(catalogs[entry.ID].Version) != "2.0.0" {
					t.Fatal("inspection lost independent market metadata")
				}
			})
		})
	}
}

func pluginUpdateQueueTester(r *Runtime) *ui.Tester {
	return ui.NewTesterWith(func(c *ui.Context) {
		r.mu.Lock()
		queue := r.queue
		r.queue = nil
		r.mu.Unlock()
		for _, run := range queue {
			run(c)
		}
	}, 1, 1, ui.TesterOptions{Preferences: ui.Preferences{ReduceTransparency: true}})
}
