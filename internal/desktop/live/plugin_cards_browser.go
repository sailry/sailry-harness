package live

import (
	"encoding/json"
	"errors"
	"net/url"
	"runtime"
	"slices"
	"strings"
	"time"

	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"
	"github.com/kaptinlin/jsonpointer"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type pluginCardBrowserWindow struct {
	window *mygo.Window
}

func (browser *pluginCardBrowserWindow) close() {
	if browser.window != nil {
		browser.window.Destroy()
		browser.window = nil
	}
}

func pluginCardBrowserAllowed(value string, target protocol.PluginCardTarget, redirect *string) bool {
	parsed, err := url.Parse(value)
	if err != nil || parsed.Hostname() == "" || parsed.User != nil || parsed.Opaque != "" {
		return false
	}
	origin := strings.ToLower(parsed.Scheme) + "://" + strings.ToLower(parsed.Host)
	if parsed.Scheme == "https" {
		if parsed.Port() == "443" {
			origin = "https://" + strings.ToLower(parsed.Hostname())
		}
		if slices.Contains(target.Origins, origin) {
			return true
		}
	}
	if redirect == nil || parsed.Fragment != "" {
		return false
	}
	expected, err := url.Parse(*redirect)
	return err == nil && expected.Scheme == "http" && expected.Hostname() == "127.0.0.1" &&
		parsed.Scheme == expected.Scheme && parsed.Host == expected.Host && parsed.Path == expected.Path
}

func (r *Runtime) openPluginCardBrowser(view *ui.Context, card *pluginCardInstance, name string, mode protocol.PluginCardMode) {
	if card.browser || card.host == nil {
		return
	}
	target, valid := card.host.Target(name, mode)
	if !valid {
		return
	}
	var address string
	switch target.Source.Kind {
	case "props":
		pointer, valid := detail[protocol.PluginCardSourcePropsPayload](target.Source.Data)
		var props any
		if valid && json.Unmarshal(card.host.State().Props, &props) == nil {
			value, _ := jsonpointer.Value(props, pointer.Pointer)
			address, _ = value.(string)
		}
	case "mcp_authorization":
		if !card.login.canOpen(time.Now()) {
			r.pluginCardToast(view, i18n.T(view, "plugins_failed"), true)
			return
		}
		address = card.login.url
	}
	if address == "" || !pluginCardBrowserAllowed(address, target, nil) {
		r.pluginCardToast(view, i18n.T(view, "plugins_failed"), true)
		return
	}
	card.browser = true
	generation, ctx := card.generation, card.ctx
	reference, receipt := card.summary.Package, card.summary.Receipt
	if !card.owner.async(func() {
		output, err := card.owner.controller.Do(ctx, protocol.Command{Kind: "read_plugin_view", Data: protocol.CommandReadPluginViewPayload{
			Package: reference, Surface: protocol.PluginDesktopSurface{Kind: "card", Data: receipt},
		}})
		if err == nil && output.Kind != "plugin_view" {
			err = errors.New("card browser validation response expected")
		}
		r.enqueue(func(current *ui.Context) {
			if !r.pluginCardLive(card, generation) {
				return
			}
			card.browser = false
			authMatches := target.Source.Kind != "mcp_authorization" || card.login.url == address && card.login.canOpen(time.Now())
			if err != nil || !authMatches {
				r.pluginCardToast(current, i18n.T(current, "plugins_failed"), true)
				return
			}
			if mode == protocol.PluginCardModeExternal {
				current.OpenURL(address)
				return
			}
			if err := r.createPluginCardBrowser(card, target, address); err != nil {
				r.pluginCardToast(current, i18n.T(current, "plugins_failed"), true)
			}
		})
	}) {
		card.browser = false
	}
}

func (r *Runtime) createPluginCardBrowser(card *pluginCardInstance, target protocol.PluginCardTarget, address string) error {
	if runtime.GOOS != "darwin" {
		return pluginNativeFault(protocol.ErrorCodeNotConfigured, "isolated embedded browser is unavailable")
	}
	if card.popup != nil {
		card.popup.close()
	}
	generation, ctx := card.generation, card.ctx
	var redirect *string
	if card.login.redirect != nil {
		redirect = new(*card.login.redirect)
	}
	window := mygo.NewWindow(mygo.WindowOptions{
		Title: pluginLabel(card.summary.Fallback, card.locale), Width: 900, Height: 700, Hidden: true,
		Page: mygo.PageOptions{Untrusted: true, Session: mygo.NewEphemeralSession(), DevTools: mygo.DevToolsDisabled},
	})
	if window == nil {
		return pluginNativeFault(protocol.ErrorCodeUnavailable, "isolated browser could not be created")
	}
	page := window.Page()
	page.SetPermissionHandler(func(mygo.PermissionRequest) bool { return false })
	page.SetWindowOpenHandler(func(mygo.WindowOpenRequest) *mygo.WindowOptions { return nil })
	page.OnWillDownload(func(event *mygo.DownloadEvent) { event.PreventDefault() })
	page.OnWillNavigate(func(event *mygo.NavigateEvent) {
		if ctx.Err() != nil || !r.pluginCardLive(card, generation) || !pluginCardBrowserAllowed(event.URL, target, redirect) {
			event.PreventDefault()
		}
	})
	if err := page.LoadURL(address); err != nil {
		window.Destroy()
		return err
	}
	card.popup = &pluginCardBrowserWindow{window: window}
	window.Show()
	return nil
}

func (r *Runtime) openPluginCardSettings(card *pluginCardInstance) {
	if card.settings || card.host == nil {
		return
	}
	card.settings = true
	generation, ctx, key := card.generation, card.ctx, card.declaration.Key
	if !card.owner.async(func() {
		status, err := card.status(ctx, key)
		var info protocol.PluginInfo
		if err == nil {
			output, failure := card.owner.controller.Do(ctx, protocol.Command{Kind: "read_plugin", Data: protocol.CommandReadPluginPayload{Name: status.Package.Name}})
			err = failure
			var valid bool
			info, valid = detail[protocol.PluginInfo](output.Data)
			if err == nil && (output.Kind != "plugin" || !valid || !info.Summary.Enabled || packageReference(info.Summary) != status.Package) {
				err = pluginNativeFault(protocol.ErrorCodeRevisionConflict, "plugin configuration changed")
			}
		}
		r.enqueue(func(view *ui.Context) {
			if !r.pluginCardLive(card, generation) {
				return
			}
			card.settings = false
			if err != nil {
				r.pluginCardToast(view, i18n.T(view, "plugins_failed"), true)
				return
			}
			r.App.Host = card.owner.controller.Target().String()
			r.App.Settings.Open(settings.Plugins)
			r.App.Navigate(shell.SettingsPage)
			r.packagesFrame(view)
			r.App.Settings.ShowDetails(settings.DetailPlugin, info.Summary.Name)
		})
	}) {
		card.settings = false
	}
}
