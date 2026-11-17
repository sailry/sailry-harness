package live

import (
	"errors"
	"slices"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// Node-only packages run on their execution Node. Desktop declarations also
// need to load in this process before replacing its currently usable UI.
func ensureDesktopPackage(info protocol.PluginInfo) error {
	if info.Extension == nil || info.Extension.Desktop == nil {
		return nil
	}
	return plugins.EnsureCompatible(plugins.InfoCompatibility(info))
}

func (r *Runtime) inspectOfficialPackage(name string, version *string) (protocol.PluginCatalogSelection, error) {
	out, err := r.controller.Do(r.ctx, protocol.Command{Kind: "read_catalog_plugin_info", Data: protocol.CommandReadCatalogPluginInfoPayload{
		Source: protocol.PluginCatalogSourceOfficial, ID: name, Version: version,
	}})
	if err != nil {
		return protocol.PluginCatalogSelection{}, err
	}
	value, valid := detail[protocol.PluginCatalogDetails](out.Data)
	info, validInfo := detail[protocol.PluginInfo](value.Data)
	if !valid || out.Kind != "catalog_details" || value.Kind != "plugin" || !validInfo || info.Summary.Name != name ||
		version != nil && stringValue(info.Summary.Version) != *version {
		return protocol.PluginCatalogSelection{}, errors.New("unexpected official plugin details response")
	}
	return officialPackageSelection(info)
}

// Previously reviewed immutable selections are safe to reuse when a subsequent
// catalog check recommends the same version and commit. Installation rechecks it.
func (r *Runtime) packageUpdateCache() map[string]protocol.PluginInfo {
	cache := map[string]protocol.PluginInfo{}
	for name, report := range r.packages.updates {
		if !packageUpdateAvailable(report) {
			continue
		}
		if info, valid := detail[protocol.PluginInfo](report.Available.Data); valid && info.UpdateSource != nil && info.UpdateSource.Kind == "official" {
			cache[name] = info
		}
	}
	return cache
}

func officialUpdateMatches(info protocol.PluginInfo, entry protocol.PluginCatalogEntry) bool {
	selection, err := officialPackageSelection(info)
	return err == nil && entry.CompatibleVersion != nil && info.Summary.Name == entry.ID &&
		stringValue(info.Summary.Version) == entry.CompatibleVersion.Version && selection.Source.Commit == entry.CompatibleVersion.Commit
}

// Resolve only actual update candidates, rather than downloading every market
// package. Lists, update review and batch actions share the inspected manifest.
func (r *Runtime) resolvePackageUpdates(batch *protocol.PluginUpdatesBatch, cache map[string]protocol.PluginInfo) map[string]protocol.PluginCatalogEntry {
	catalogs := map[string]protocol.PluginCatalogEntry{}
	for index := range batch.Reports {
		report := &batch.Reports[index]
		if report.Available == nil || report.Blocked != nil || report.Current.Pinned {
			continue
		}
		var info protocol.PluginInfo
		var err error
		switch report.Available.Kind {
		case "official":
			entry, valid := detail[protocol.PluginCatalogEntry](report.Available.Data)
			if !valid || entry.ID != report.Current.Name || entry.CompatibleVersion == nil {
				err = errors.New("unexpected official plugin update candidate")
				break
			}
			catalogs[entry.ID] = entry
			info = cache[entry.ID]
			if !officialUpdateMatches(info, entry) {
				var selection protocol.PluginCatalogSelection
				selection, err = r.inspectOfficialPackage(entry.ID, &entry.CompatibleVersion.Version)
				info = selection.Info
			}
			if err == nil && !officialUpdateMatches(info, entry) {
				err = &protocol.Fault{Code: protocol.ErrorCodeRevisionConflict, Message: "official plugin update changed during inspection"}
			}
			if err == nil {
				info.Versions = entry.Versions
				if entry.CompatibleVersion.ReleaseNotes != nil {
					info.ReleaseNotes = entry.CompatibleVersion.ReleaseNotes
				}
			}
		case "package":
			var valid bool
			info, valid = detail[protocol.PluginInfo](report.Available.Data)
			if !valid || info.Summary.Name != report.Current.Name {
				err = errors.New("unexpected plugin update candidate")
			}
		default:
			err = errors.New("unexpected plugin update candidate kind")
		}
		report.Available = nil
		if err != nil {
			batch.Failures = append(batch.Failures, protocol.PluginUpdatesFailure{Name: report.Current.Name, Error: protocol.Fault{Code: faultCode(err), Message: err.Error()}})
			continue
		}
		if ensureDesktopPackage(info) == nil {
			report.Available = &protocol.PluginUpdatesCandidate{Kind: "package", Data: info}
		}
	}
	return catalogs
}

func sameOfficialUpdate(first, second protocol.PluginCatalogSelection) bool {
	var firstActions, secondActions []protocol.PluginAction
	if first.Info.Extension != nil {
		firstActions = first.Info.Extension.Actions
	}
	if second.Info.Extension != nil {
		secondActions = second.Info.Extension.Actions
	}
	return first.Info.Summary.Name == second.Info.Summary.Name && stringValue(first.Info.Summary.Version) == stringValue(second.Info.Summary.Version) &&
		first.Info.Summary.Digest == second.Info.Summary.Digest && first.Source == second.Source && first.Path == second.Path && slices.Equal(firstActions, secondActions)
}

func (r *Runtime) updateOfficialPackage(report protocol.PluginUpdatesReport, reviewed protocol.PluginInfo, finish func()) {
	expected, err := officialPackageSelection(reviewed)
	if err != nil {
		finish()
		return
	}
	r.async(func() {
		selected, err := r.inspectOfficialPackage(reviewed.Summary.Name, reviewed.Summary.Version)
		if err == nil {
			err = ensureDesktopPackage(selected.Info)
		}
		changed := err == nil && !sameOfficialUpdate(expected, selected)
		if changed {
			err = &protocol.Fault{Code: protocol.ErrorCodeRevisionConflict, Message: "plugin update changed; review the selected version again"}
		}
		r.enqueue(func(c *ui.Context) {
			if err != nil {
				if samePackageVersion(report.Current, r.packages.infos[report.Current.Name].Summary) {
					report.Available = nil
					if changed {
						report.Available = &protocol.PluginUpdatesCandidate{Kind: "package", Data: selected.Info}
					}
					r.packages.updates[report.Current.Name] = report
					r.projectPackages()
				}
				finish()
				r.packageReport(c, err)
				return
			}
			r.installOfficialSelection(selected, report.Current.Revision, protocol.PluginInstallModeUpdate, finish)
		})
	})
}
