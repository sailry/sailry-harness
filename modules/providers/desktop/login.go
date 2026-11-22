package desktop

import (
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/preview"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
)

// ShowProviderAuthorization gives each attempt its own view, including when another Node opens a dialog.
func (m *Controller) ShowProviderAuthorization(node, id string, view *settings.Authorization) {
	m.loginFor = channelKey(node, id)
	m.login = view
	view.Open = true
}

func (m *Controller) ProviderAuthorization() *settings.Authorization {
	if m.login == nil {
		m.login = &settings.Authorization{}
	}
	return m.login
}

// loginWarning is a package's warning, asked before a sign-in starts or,
// for a method without one, before a channel is added with it. verb is the
// i18n key of what continuing does, said before title.
type loginWarning struct {
	open              bool
	verb, title, text string
	proceed           func()
	cancel            func()
}

// WarnProviderAuthorization asks the user to accept text before the sign-in
// of the provider named title starts; proceed runs when they continue.
func (m *Controller) WarnProviderAuthorization(title, text string, proceed, cancel func()) {
	m.loginWarning = loginWarning{open: true, verb: "provider_reconnect", title: title, text: text, proceed: proceed, cancel: cancel}
}

// configureProvider opens the add editor's form on a method. A method that
// has no sign-in and whose package declares a warning asks first, once for
// the pick: Continue opens the form as ever, Cancel leaves the editor on the
// step it was on and adds nothing.
func (m *Controller) configureProvider(x preview.Method) {
	e := &m.provider
	if x.Warning == "" || x.Login != "" {
		e.configure(x)
		return
	}
	generation := e.generation
	m.loginWarning = loginWarning{open: true, verb: "settings_add", title: x.Name, text: x.Warning, proceed: func() {
		if e.open && e.generation == generation && e.editing == "" {
			e.configure(x)
		}
	}}
}

// chooseProvider takes a choice of the brand step as providerEditor.choose
// does, asking a lone method's warning before its form.
func (m *Controller) chooseProvider(b *preview.Brand, methods []preview.Method) {
	if len(methods) != 1 {
		m.provider.choose(b, methods)
		return
	}
	m.provider.brand, m.provider.methods = b, methods
	m.configureProvider(methods[0])
}

// Picker is the providers' brand picker for the welcome guide
// (Workspace::provider_picker): a choice opens the add editor on that
// brand or login, then runs chosen, which shows Providers. back reopens
// the guide from the editor's first Back.
func (m *Controller) Picker(c *ui.Context, chosen, back func()) {
	var node string
	if m.Live && m.ProviderActions.Hosts != nil {
		for _, host := range m.ProviderActions.Hosts() {
			if host.Local {
				node = host.Node
				break
			}
		}
	}
	brandPicker(c, m, node, func() { m.provider.addOn(m, node) }, func() {
		m.ManagedNode = ""
		m.provider.returnTo = back
		chosen()
	})
}

// LoginDialog renders pending authorization, including sign-ins started outside settings.
func (m *Controller) LoginDialog(c *ui.Context) {
	view := m.ProviderAuthorization()
	settings.AuthorizationDialog(c, "login", view, func() {
		if ch := m.channel(m.loginFor); ch != nil {
			channelTile(c, ch, 48)
		}
	}, view.Retry, func() {
		if strings.TrimSpace(view.Pasted) == "" {
			kit.Toast(c, kit.Failed, i18n.T(c, "provider_login_callback_required"))
			return
		}
		if view.Submit != nil {
			view.Submit(view.Pasted)
		} else {
			settings.PreviewNotice(c)
		}
	})
	if w := &m.loginWarning; w.open {
		proceed, cancel := w.proceed, w.cancel
		confirmed := settings.Confirm(c, "login-warning", &w.open, i18n.T(c, w.verb)+" "+w.title, w.text, i18n.T(c, "provider_login_continue"))
		if confirmed && proceed != nil {
			proceed()
		} else if !w.open && cancel != nil {
			cancel()
		}
		if !w.open {
			*w = loginWarning{}
		}
	}
}
