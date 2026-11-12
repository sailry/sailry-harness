package settings

import "github.com/sailry/sailry-harness/internal/desktop/preview"

func (m *Model) BeginProviderAuthorization(id, title string, callback bool) {
	m.loginFor = id
	m.login = Authorization{Open: true, Title: title, Flow: FlowStarting, Callback: callback}
	m.login.Cancel = m.ProviderActions.CancelLogin
}

func (m *Model) ProviderAuthorization() *Authorization {
	return &m.login
}

// loginWarning is a package's warning, asked before a sign-in starts or,
// for a method without one, before a channel is added with it. verb is the
// i18n key of what continuing does, said before title.
type loginWarning struct {
	open              bool
	verb, title, text string
	proceed           func()
}

// WarnProviderAuthorization asks the user to accept text before the sign-in
// of the provider named title starts; proceed runs when they continue.
func (m *Model) WarnProviderAuthorization(title, text string, proceed func()) {
	m.loginWarning = loginWarning{open: true, verb: "provider_reconnect", title: title, text: text, proceed: proceed}
}

// configureProvider opens the add editor's form on a method. A method that
// has no sign-in and whose package declares a warning asks first, once for
// the pick: Continue opens the form as ever, Cancel leaves the editor on the
// step it was on and adds nothing.
func (m *Model) configureProvider(x preview.Method) {
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
func (m *Model) chooseProvider(b *preview.Brand, methods []preview.Method) {
	if len(methods) != 1 {
		m.provider.choose(b, methods)
		return
	}
	m.provider.brand, m.provider.methods = b, methods
	m.configureProvider(methods[0])
}
