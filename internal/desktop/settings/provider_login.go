package settings

func (m *Model) BeginProviderAuthorization(id, title string, callback bool) {
	m.loginFor = id
	m.login = Authorization{Open: true, Title: title, Flow: FlowStarting, Callback: callback}
	m.login.Cancel = m.ProviderActions.CancelLogin
}

func (m *Model) ProviderAuthorization() *Authorization {
	return &m.login
}

// loginWarning is a sign-in's warning, asked before the sign-in starts.
type loginWarning struct {
	open        bool
	title, text string
	proceed     func()
}

// WarnProviderAuthorization asks the user to accept text before the sign-in
// of the provider named title starts; proceed runs when they continue.
func (m *Model) WarnProviderAuthorization(title, text string, proceed func()) {
	m.loginWarning = loginWarning{open: true, title: title, text: text, proceed: proceed}
}
