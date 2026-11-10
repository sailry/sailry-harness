package settings

func (m *Model) BeginProviderAuthorization(id, title string, callback bool) {
	m.loginFor = id
	m.login = Authorization{Open: true, Title: title, Flow: FlowStarting, Callback: callback}
	m.login.Cancel = m.ProviderActions.CancelLogin
}

func (m *Model) ProviderAuthorization() *Authorization {
	return &m.login
}
