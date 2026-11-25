package live

import (
	"context"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type liveSettingsPage struct {
	section settings.Section
	window  settings.WindowBinding
	local   settings.PageBinding
	hide    context.CancelFunc
	active  *liveSettingsTarget
	blocked string
}

type liveSettingsTarget struct {
	owner    *Runtime
	user     protocol.UserID
	target   settings.PageTarget
	retained func() bool
	cancel   context.CancelFunc
	binding  settings.PageBinding
}

func (r *Runtime) bindSettingsPages(pages []settings.NativePage) {
	seen := make(map[settings.Section]bool, len(pages))
	for _, page := range pages {
		if !seen[page.Section] && page.BindWindow != nil && r.App.Settings.HasNativePage(page.Section) {
			r.settingsPages = append(r.settingsPages, liveSettingsPage{section: page.Section, window: page.BindWindow(r)})
		}
		seen[page.Section] = true
	}
}

func (r *Runtime) settingsSectionVisible(section settings.Section) bool {
	return r.App.Page == shell.SettingsPage && r.App.Settings.Section == section && !r.App.DemoOn()
}

func (p *liveSettingsPage) visible(r *Runtime) bool {
	return r.ctx.Err() == nil && !r.App.DemoOn() && r.App.Settings.HasNativePage(p.section) &&
		(r.settingsSectionVisible(p.section) || p.window.AlsoVisible != nil && p.window.AlsoVisible())
}

func settingsTargetUser(owner *Runtime) protocol.UserID {
	if user := owner.snapshot().User; user != nil {
		return user.ID
	}
	return protocol.UserID{}
}

func (p *liveSettingsPage) status(owner *Runtime) string {
	if owner == nil || !owner.controller.Connected() {
		if owner != nil && faultCode(owner.controller.Err()) == protocol.ErrorCodePermissionDenied {
			return "permission_denied"
		}
		return "connections_offline"
	}
	if p.window.Administrator {
		return owner.machineSettingsStatus()
	}
	return ""
}

func (p *liveSettingsPage) release() {
	if active := p.active; active != nil {
		p.active = nil
		active.cancel()
		if active.binding.Close != nil {
			active.binding.Close()
		}
	}
	p.blocked = ""
}

func (p *liveSettingsPage) hidePage() {
	p.release()
	if p.hide != nil {
		p.hide()
		p.hide = nil
		if p.local.Close != nil {
			p.local.Close()
		}
		p.local = settings.PageBinding{}
	}
}

func (p *liveSettingsPage) transientOutage(owner *Runtime, status string) bool {
	if status != "connections_offline" || owner == nil || !p.window.KeepOnOutage {
		return false
	}
	switch faultCode(owner.controller.Err()) {
	case protocol.ErrorCodeUnavailable, protocol.ErrorCodeBusy:
		return true
	}
	return false
}

func (r *Runtime) syncSettingsPages() {
	for i := range r.settingsPages {
		p := &r.settingsPages[i]
		if !p.visible(r) {
			p.hidePage()
			continue
		}
		if p.window.Show != nil && p.hide == nil {
			ctx, hide := context.WithCancel(r.ctx)
			p.hide, p.local = hide, p.window.Show(ctx)
		}
		if p.local.Follow != nil {
			p.local.Follow()
		}
		if p.window.Mount == nil {
			continue
		}
		owner := r.runtimeFor(r.App.Settings.ManagedNode)
		var user protocol.UserID
		if owner != nil {
			user = settingsTargetUser(owner)
		}
		if active := p.active; active != nil && (active.owner != owner || active.user != user) {
			p.release()
		}
		status := p.status(owner)
		if status != "" {
			if p.active != nil && p.transientOutage(owner, status) {
				if p.blocked != status && p.active.binding.Suspend != nil {
					p.active.binding.Suspend(status)
				}
				p.blocked = status
				continue
			}
			if p.active != nil {
				p.release()
			}
			if status != p.blocked && p.window.Blocked != nil {
				p.window.Blocked(status)
			}
			p.blocked = status
			continue
		}
		p.blocked = ""
		if p.active == nil {
			ctx, cancel := context.WithCancel(owner.ctx)
			active := &liveSettingsTarget{owner: owner, user: user, cancel: cancel}
			sameBinding := func() bool {
				return ctx.Err() == nil && p.active == active && p.visible(r) &&
					r.runtimeFor(r.App.Settings.ManagedNode) == owner && settingsTargetUser(owner) == user
			}
			active.retained = func() bool {
				status := p.status(owner)
				return sameBinding() && (status == "" || p.transientOutage(owner, status))
			}
			active.target = settings.PageTarget{Node: owner.controller.Target().String(), Context: ctx, Current: func() bool {
				return sameBinding() && p.status(owner) == ""
			}}
			p.active = active
			active.binding = p.window.Mount(active.target)
		}
		if follow := p.active.binding.Follow; follow != nil {
			follow()
		}
	}
}

func (r *Runtime) settingsPage(section settings.Section) *liveSettingsTarget {
	root := r.localRuntime()
	for i := range root.settingsPages {
		p := &root.settingsPages[i]
		if p.section == section {
			return p.active
		}
	}
	return nil
}

func (r *Runtime) currentSettingsPage(section settings.Section) *liveSettingsTarget {
	if active := r.settingsPage(section); active != nil && active.owner == r && active.target.Current() {
		return active
	}
	return nil
}

func (r *Runtime) readPageCommand(target settings.PageTarget, command protocol.Command, done func(*ui.Context, protocol.Output, error)) {
	r.async(func() {
		output, err := r.controller.Do(target.Context, command)
		r.enqueue(func(c *ui.Context) {
			if target.Current() {
				done(c, output, err)
			}
		})
	})
}

func pageAction[T any](target settings.PageTarget, action func(T)) func(T) {
	return func(value T) {
		if target.Current() {
			action(value)
		}
	}
}

func (r *Runtime) recoverSettingsPages(refresh bool) {
	root := r.localRuntime()
	for i := range root.settingsPages {
		p := &root.settingsPages[i]
		if active := p.active; active != nil && (active.owner == r || refresh) && active.target.Current() && active.binding.Recover != nil {
			active.binding.Recover()
		}
	}
}

func (r *Runtime) closeSettingsPages() {
	for i := range r.settingsPages {
		p := &r.settingsPages[i]
		p.hidePage()
		if p.window.Close != nil {
			p.window.Close()
		}
	}
	r.settingsPages = nil
}
