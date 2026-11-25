package live

import (
	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type computerSettingsState struct {
	loaded      bool
	loading     bool
	loadFailed  bool
	observedRev uint64
	enabled     settings.Stored[bool]
	available   bool
	checking    bool
	permsLoaded bool
	permissions []preview.Permission
}

var computerSettingsMessages = settings.Messages{
	Done:      "computer_settings_saved",
	Failed:    "computer_settings_failed",
	Conflict:  "computer_settings_conflict",
	Uncertain: "computer_settings_unknown",
}

func (r *Runtime) BindComputerSettings() settings.WindowBinding {
	m := r.App.Settings
	return settings.WindowBinding{
		Administrator: true,
		Blocked:       func(status string) { m.Computer = settings.ComputerSettingsState{Status: status} },
		Mount: func(target settings.PageTarget) settings.PageBinding {
			owner := r.runtimeFor(target.Node)
			st := &owner.computerSettings
			st.loaded, st.loadFailed, st.permsLoaded = false, false, false
			st.permissions = nil
			m.ComputerActions = &settings.ComputerActions{
				Save: func() {
					if target.Current() {
						st.enabled.Draft = m.Computer.Enabled.Draft
						owner.saveComputerSettings()
					}
				},
				CheckPermissions: func() {
					if target.Current() {
						owner.readComputerPermissions()
					}
				},
				RequestPermission: pageAction(target, owner.requestComputerPermission),
				OpenSettings:      pageAction(target, owner.openComputerPermissionSettings),
				ShowApp: func() {
					if target.Current() {
						r.showPermissionApp()
					}
				},
			}
			return settings.PageBinding{
				Follow: func() {
					if ((!st.loaded && !st.loadFailed) || owner.snapshot().ComputerSettingsRevision > st.observedRev) && !st.loading && !st.enabled.Pending {
						owner.readComputerSettings()
					}
					if st.loaded && st.enabled.Saved && !st.permsLoaded && !st.checking {
						owner.readComputerPermissions()
					}
					m.Computer = settings.ComputerSettingsState{
						Enabled: st.enabled, Loaded: st.loaded, Available: st.available, Checking: st.checking,
						HostReadOnly: owner != r, Permissions: st.permissions,
					}
				},
				Recover: func() { st.loadFailed = false },
				Close:   func() { m.ComputerActions, m.Computer = nil, settings.ComputerSettingsState{} },
			}
		},
	}
}

func (r *Runtime) readComputerSettings() {
	st := &r.computerSettings
	page := r.currentSettingsPage(settings.Computer)
	if page == nil || st.loading {
		return
	}
	st.loading = true
	st.loadFailed = false
	st.observedRev = r.snapshot().ComputerSettingsRevision
	r.async(func() {
		out, err := r.controller.Do(page.target.Context, protocol.Command{Kind: "read_computer_settings"})
		r.enqueue(func(_ *ui.Context) {
			st.loading = false
			if !page.target.Current() {
				return
			}
			if err != nil {
				st.loadFailed = true
				return
			}
			value, ok := detail[protocol.ComputerSettings](out.Data)
			if !ok || out.Kind != "computer_settings" {
				st.loadFailed = true
				return
			}
			st.loaded = true
			st.available = value.Available
			if st.enabled.Draft == st.enabled.Saved {
				st.enabled.Draft = value.Enabled
			}
			st.enabled.Saved = value.Enabled
			st.enabled.Revision = value.Revision
			st.observedRev = value.Revision
		})
	})
}

func (r *Runtime) saveComputerSettings() {
	st := &r.computerSettings
	if st.enabled.Pending {
		return
	}
	st.enabled.Pending = true
	req := protocol.ComputerSettings{Revision: st.enabled.Revision, Enabled: st.enabled.Draft}
	r.saveSettingReread(protocol.Command{Kind: "save_computer_settings", Data: req}, computerSettingsMessages, r.readComputerSettings, func(out protocol.Output, ok bool) {
		st.enabled.Pending = false
		if !ok {
			st.enabled.Draft = st.enabled.Saved
			return
		}
		if value, valid := detail[protocol.ComputerSettings](out.Data); valid && out.Kind == "computer_settings" {
			st.loaded = true
			st.loadFailed = false
			st.available = value.Available
			st.enabled.Saved = value.Enabled
			st.enabled.Draft = value.Enabled
			st.enabled.Revision = value.Revision
			st.observedRev = value.Revision
			if value.Enabled {
				st.permsLoaded = false
				r.readComputerPermissions()
			} else {
				st.permissions = nil
				st.permsLoaded = false
			}
		}
	})
}

func computerPermissionRows(perms protocol.ComputerPermissions) []preview.Permission {
	darwinLocal := perms.Local && perms.Platform == "macos"
	statusFor := func(granted *bool) string {
		if perms.Platform != "macos" {
			return "permission_not_needed"
		}
		if granted == nil {
			return "permission_unknown"
		}
		if *granted {
			return "permission_granted"
		}
		return "permission_required"
	}
	row := func(resource string, granted *bool) preview.Permission {
		status := statusFor(granted)
		return preview.Permission{
			Resource: resource,
			Status:   status,
			Settings: darwinLocal && status == "permission_granted",
		}
	}
	return []preview.Permission{
		row("permission_accessibility", perms.Accessibility),
		row("permission_screen", perms.ScreenCapture),
	}
}

func (r *Runtime) readComputerPermissions() {
	st := &r.computerSettings
	page := r.currentSettingsPage(settings.Computer)
	if page == nil || st.checking {
		return
	}
	st.checking = true
	r.async(func() {
		out, err := r.controller.Do(page.target.Context, protocol.Command{Kind: "read_computer_permissions"})
		r.enqueue(func(c *ui.Context) {
			st.checking = false
			if !page.target.Current() {
				return
			}
			st.permsLoaded = true
			if err != nil {
				st.permissions = []preview.Permission{
					{Resource: "permission_accessibility", Status: "permission_unknown"},
					{Resource: "permission_screen", Status: "permission_unknown"},
				}
				kit.Toast(c, kit.Failed, i18n.T(c, "permission_failed"))
				return
			}
			perms, ok := detail[protocol.ComputerPermissions](out.Data)
			if !ok || out.Kind != "computer_permissions" {
				kit.Toast(c, kit.Failed, i18n.T(c, "permission_failed"))
				return
			}
			st.permissions = computerPermissionRows(perms)
		})
	})
}

func (r *Runtime) requestComputerPermission(resource string) {
	root := r.localRuntime()
	if r != root || r.computerSettings.checking {
		return
	}
	var perm protocol.ComputerPermission
	switch resource {
	case "permission_accessibility":
		perm = protocol.ComputerPermissionAccessibility
	case "permission_screen":
		perm = protocol.ComputerPermissionScreenCapture
	default:
		return
	}
	r.computerSettings.checking = true
	r.async(func() {
		out, err := r.controller.Do(r.ctx, protocol.Command{
			Kind: "request_computer_permission",
			Data: protocol.CommandRequestComputerPermissionPayload{Permission: perm},
		})
		r.enqueue(func(c *ui.Context) {
			r.computerSettings.checking = false
			r.computerSettings.permsLoaded = true
			if err != nil {
				kit.Toast(c, kit.Failed, i18n.T(c, "permission_failed"))
				return
			}
			if perms, ok := detail[protocol.ComputerPermissions](out.Data); ok && out.Kind == "computer_permissions" {
				r.computerSettings.permissions = computerPermissionRows(perms)
			}
		})
	})
}

func (r *Runtime) openComputerPermissionSettings(resource string) {
	root := r.localRuntime()
	if r != root {
		return
	}
	if root.permissions.probe.settings == nil {
		root.permissions.probe = systemPermissions()
	}
	kind, known := root.permissionKind(resource)
	if !known {
		return
	}
	probe := root.permissions.probe
	r.async(func() {
		if err := probe.settings(permissionKinds[kind].flow); err != nil {
			r.enqueue(func(c *ui.Context) {
				kit.Toast(c, kit.Failed, i18n.T(c, "permission_failed"))
			})
		}
	})
}
