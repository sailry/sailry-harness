package live

import (
	"context"
	"fmt"
	"html"
	"net"
	"net/http"
	"strconv"
	"strings"
	"sync"
	"time"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type authorizationState struct {
	provider                       *liveAuthorization
	connector                      *liveAuthorization
	credentials                    map[protocol.CredentialID]protocol.Credential
	credentialKey                  string
	credentialLoad                 bool
	savedProviders                 map[string]protocol.ConversationProvider
	types                          map[string]protocol.PluginConnectorType
	connections                    map[string]protocol.PluginConnectorConnection
	connectionsFailed              bool
	packages                       map[string]protocol.PluginInfo
	packagesObserved               map[string]uint64
	packagesReading, packagesStale bool
	quotaReset                     uint64
	quotaCancel                    context.CancelFunc
	mcpDrafts                      map[string]protocol.PluginMcpState
	mcpConfiguring                 string
	mcpChecking                    map[string]bool // package and server of each check in flight
	mcpFailed                      map[string]mcpFailure
	connectorBusy                  map[string]bool // services with a command under way
	loginRecoveries                map[string]*controllerLoginRecovery
	loginStarts                    uint64 // sign-in starts sent, one scope each
}

type liveAuthorization struct {
	id         protocol.RequestID
	cancel     context.CancelFunc
	provider   string
	connector  settings.ConnectorDraft
	title      string
	view       *settings.Authorization
	generation uint64
	ctx        context.Context
	recovery   *controllerLoginRecovery
	connected  bool
}

func localAuthorization(ctx context.Context, presentation *protocol.PluginModelAuthPresentation, locale string) (*string, <-chan string, bool, error) {
	path, port, manual, needed, localhost := "/auth/callback", uint16(0), false, false, false
	if presentation != nil {
		switch presentation.Kind {
		case "pkce":
			value, ok := detail[protocol.PluginModelAuthPresentationPkcePayload](presentation.Data)
			needed = ok && value.Redirect == protocol.PluginModelAuthRedirectLoopback
			manual = ok && value.Redirect == protocol.PluginModelAuthRedirectPaste
		case "script":
			value, ok := detail[protocol.PluginModelAuthPresentationScriptPayload](presentation.Data)
			if ok && value.Callback != nil {
				manual = value.Callback.Manual
				// A fixed loopback callback still listens when pasted input is supported.
				needed = !manual || value.Callback.Port != nil
				path = value.Callback.Path
				if value.Callback.Port != nil {
					port = *value.Callback.Port
					localhost = value.Callback.AllowLocalhost
				}
			}
		}
	}
	if !needed {
		return nil, nil, manual, nil
	}
	listener, err := net.Listen("tcp4", net.JoinHostPort("127.0.0.1", strconv.Itoa(int(port))))
	if err != nil {
		if manual {
			return nil, nil, true, nil
		}
		return nil, nil, false, err
	}
	address := listener.Addr().String()
	if localhost {
		address = net.JoinHostPort("localhost", strconv.Itoa(int(port)))
	}
	redirect := "http://" + address + path
	returned := make(chan string, 1)
	var once sync.Once
	server := &http.Server{MaxHeaderBytes: 16 * 1024}
	page := fmt.Sprintf(`<!doctype html>
<html lang="%s">
<head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>Sailry</title></head>
<body><main><h1>%s</h1><p>%s</p></main></body>
</html>`, html.EscapeString(i18n.Match(locale)),
		html.EscapeString(i18n.In(locale, "authorization_callback_received")),
		html.EscapeString(i18n.In(locale, "authorization_callback_return")))
	server.Handler = http.HandlerFunc(func(writer http.ResponseWriter, request *http.Request) {
		writer.Header().Set("Cache-Control", "no-store")
		writer.Header().Set("Referrer-Policy", "no-referrer")
		writer.Header().Set("Content-Security-Policy", "default-src 'none'; frame-ancestors 'none'")
		writer.Header().Set("Content-Type", "text/html; charset=utf-8")
		if request.Method != http.MethodGet || request.URL.Path != path || len(request.URL.RawQuery) > 16*1024 {
			writer.WriteHeader(http.StatusBadRequest)
			return
		}
		accepted := false
		once.Do(func() {
			if ctx.Err() != nil {
				return
			}
			accepted = true
			// Deliver the landing page before the authorization can complete
			// and cancel this listener.
			writer.Header().Set("Content-Length", strconv.Itoa(len(page)))
			_, _ = writer.Write([]byte(page))
			_ = http.NewResponseController(writer).Flush()
			select {
			case returned <- redirect + "?" + request.URL.RawQuery:
			case <-ctx.Done():
			}
		})
		if !accepted {
			writer.WriteHeader(http.StatusConflict)
		}
	})
	go func() { _ = server.Serve(listener) }()
	go func() {
		<-ctx.Done()
		_ = server.Close()
	}()
	return &redirect, returned, false, nil
}

// loginRequest sends a command that starts or abandons a sign-in attempt
// under a scope of its own. A sign-in closed and started again while its
// first start is in flight sends the same command, which execute would drop
// as already under way, leaving the new dialog on "starting" for good. The
// scope is never reused, so no request is kept for it.
func (r *Runtime) loginRequest(command protocol.Command, finished func(*ui.Context, protocol.Output, error)) protocol.RequestID {
	r.authorizations.loginStarts++
	scope := "login/" + strconv.FormatUint(r.authorizations.loginStarts, 10)
	return r.executeScoped(scope, command, func(c *ui.Context, output protocol.Output, err error) {
		r.forgetScoped(scope, command)
		finished(c, output, err)
	})
}

// abandonLogin cancels the Node's attempt of a start that was superseded
// before it answered: nothing here follows that attempt. An attempt carries
// its start request's identifier, so it is named even when the start's
// outcome is unknown. A start the Node refused began nothing.
func (r *Runtime) abandonLogin(kind string, data any, err error) {
	if err != nil && !client.Retained(err) {
		return
	}
	r.loginRequest(protocol.Command{Kind: kind, Data: data}, func(*ui.Context, protocol.Output, error) {})
}

func (r *Runtime) initAuthorization() {
	r.authorizations = &authorizationState{
		credentials: map[protocol.CredentialID]protocol.Credential{}, savedProviders: map[string]protocol.ConversationProvider{},
		types: map[string]protocol.PluginConnectorType{}, connections: map[string]protocol.PluginConnectorConnection{}, packages: map[string]protocol.PluginInfo{},
		mcpDrafts: map[string]protocol.PluginMcpState{}, mcpChecking: map[string]bool{}, mcpFailed: map[string]mcpFailure{}, connectorBusy: map[string]bool{},
	}
	r.initConnectorUI()
}

// loginEntry is the preset entry a provider was made from, which carries how
// its sign-in is presented.
func (r *Runtime) loginEntry(provider protocol.ConversationProvider) (protocol.ConversationPresetsEntry, bool) {
	for _, entry := range r.providerPresets {
		if provider.Preset != nil && provider.Preset.ID == entry.Preset.ID && samePresetSource(provider.Preset.Source, entry.Source) {
			return entry, true
		}
	}
	return protocol.ConversationPresetsEntry{}, false
}

func (r *Runtime) loginProvider(id string) (protocol.ConversationProvider, bool) {
	provider, ok := r.provider(id)
	if !ok {
		provider, ok = r.authorizations.savedProviders[id]
	}
	return provider, ok
}

// beginProviderLogin starts a provider's sign-in. A package that declares a
// warning for it is asked about first: nothing is sent until the user
// continues, once for each attempt.
func (r *Runtime) beginProviderLogin(id string) {
	r.beginProviderLoginFor(id, nil)
}

func (r *Runtime) beginProviderLoginFor(id string, recovery *controllerLoginRecovery) {
	providerUI := r.App.ProviderUI
	if providerUI == nil {
		return
	}
	provider, ok := r.loginProvider(id)
	if !ok {
		r.providerLoginFinished(providerIDFromString(id), false)
		return
	}
	if entry, found := r.loginEntry(provider); found && entry.LoginWarning != nil {
		providerUI.WarnProviderAuthorization(provider.Name, r.loginWarning(entry), func() { r.startProviderLoginFor(id, recovery) }, func() {
			r.providerLoginFinished(provider.ID, false)
		})
		return
	}
	r.startProviderLoginFor(id, recovery)
}

// loginWarning is an entry's warning in the package locale, "" for none.
func (r *Runtime) loginWarning(entry protocol.ConversationPresetsEntry) string {
	if entry.LoginWarning == nil {
		return ""
	}
	if translated := entry.LoginWarning.Locales[i18n.Match(r.packageLocale())]; translated != "" {
		return translated
	}
	return entry.LoginWarning.Text
}

func (r *Runtime) startProviderLoginFor(id string, recovery *controllerLoginRecovery) {
	provider, ok := r.loginProvider(id)
	if !ok {
		r.providerLoginFinished(providerIDFromString(id), false)
		return
	}
	if previous := r.authorizations.provider; previous != nil {
		r.cancelProviderLogin()
		previous.cancel()
	}
	var presentation *protocol.PluginModelAuthPresentation
	if entry, found := r.loginEntry(provider); found {
		presentation = entry.Login
	}
	ctx, cancel := context.WithCancel(r.ctx)
	redirect, returned, manual, err := localAuthorization(ctx, presentation, r.packageLocale())
	if err != nil {
		cancel()
		r.providerLoginFinished(provider.ID, false)
		r.enqueue(func(c *ui.Context) {
			r.reportKeys(c, err, "provider_login_failed", "provider_revision_changed", "provider_login_unknown")
		})
		return
	}
	attempt := &liveAuthorization{provider: id, ctx: ctx, cancel: cancel, title: provider.Name, recovery: recovery}
	r.authorizations.provider = attempt
	view := &settings.Authorization{Title: provider.Name, Flow: settings.FlowStarting, Callback: manual}
	attempt.view = view
	view.OpenURL = func(value string) { r.enqueue(func(c *ui.Context) { c.OpenURL(value) }) }
	view.Submit = func(callback string) { r.submitProviderLogin(attempt, callback) }
	view.Cancel = func() {
		if r.authorizations.provider == attempt {
			r.cancelProviderLogin()
		}
	}
	view.Retry = func() {
		if r.authorizations.provider == attempt {
			r.beginProviderLoginFor(id, recovery)
		}
	}
	r.App.ProviderUI.ShowProviderAuthorization(r.controller.Target().String(), id, view)
	var started protocol.RequestID
	started = r.loginRequest(protocol.Command{Kind: "begin_provider_login", Data: protocol.CommandBeginProviderLoginPayload{Provider: provider.ID, ExpectedRevision: provider.Revision, Redirect: redirect}}, func(c *ui.Context, output protocol.Output, err error) {
		if r.authorizations.provider != attempt {
			r.abandonLogin("cancel_provider_login", protocol.CommandCancelProviderLoginPayload{Attempt: started}, err)
			return
		}
		if err != nil {
			cancel()
			view.Flow = settings.FlowFailed
			r.reportKeys(c, err, "provider_login_failed", "provider_revision_changed", "provider_login_unknown")
			if faultCode(err) != protocol.ErrorCodeOutcomeUnknown {
				r.providerLoginFinished(provider.ID, false)
			}
			return
		}
		value, ok := detail[protocol.ConversationLoginAttempt](output.Data)
		if !ok {
			cancel()
			r.providerLoginFinished(provider.ID, false)
			return
		}
		attempt.id = value.ID
		r.watchProviderLogin(ctx, attempt)
		if returned != nil {
			r.observe(func() {
				select {
				case <-ctx.Done():
				case callback := <-returned:
					r.enqueue(func(*ui.Context) {
						r.submitProviderLogin(attempt, callback)
					})
				}
			})
		}
	})
}

func (r *Runtime) watchProviderLogin(ctx context.Context, attempt *liveAuthorization) {
	r.observe(func() {
		stream, err := r.controller.Subscribe(ctx, protocol.Topic{Kind: "provider_login", Data: attempt.id})
		if err != nil {
			r.enqueue(func(c *ui.Context) {
				if r.authorizations.provider == attempt && ctx.Err() == nil {
					r.reportKeys(c, err, "provider_login_failed", "provider_revision_changed", "provider_login_unknown")
				}
			})
			return
		}
		defer stream.Close()
		for {
			update, err := stream.Next(ctx)
			if err != nil {
				return
			}
			value, ok := detail[protocol.ConversationLoginUpdate](update.Data)
			if !ok {
				continue
			}
			r.enqueue(func(c *ui.Context) {
				if r.authorizations.provider != attempt {
					return
				}
				r.applyProviderLogin(c, attempt, value)
			})
			if value.State.Kind == "connected" || value.State.Kind == "failed" || value.State.Kind == "cancelled" {
				return
			}
		}
	})
}

func projectAuthorization(view *settings.Authorization, state protocol.ConversationLoginState) (uint64, bool) {
	view.Busy = false
	switch state.Kind {
	case "starting":
		view.Flow = settings.FlowStarting
	case "pending":
		value, ok := detail[protocol.ConversationLoginStatePendingPayload](state.Data)
		if ok {
			view.Flow, view.Code, view.URL = settings.FlowDevice, value.UserCode, value.VerificationUrl
			return value.ExpiresAtMs, false
		}
	case "browser":
		value, ok := detail[protocol.ConversationLoginStateBrowserPayload](state.Data)
		if ok {
			view.Flow, view.URL = settings.FlowBrowser, value.AuthorizationUrl
			return value.ExpiresAtMs, false
		}
	case "exchanging":
		view.Busy = true
	case "failed":
		view.Flow = settings.FlowFailed
		view.Cancelling = false
		return 0, true
	case "connected", "cancelled":
		view.Open, view.Cancelling = false, false
		return 0, true
	}
	return 0, false
}

func (r *Runtime) applyProviderLogin(c *ui.Context, attempt *liveAuthorization, value protocol.ConversationLoginUpdate) {
	view := attempt.view
	if value.State.Kind == "exchanging" && value.Credential != nil {
		// The sign-in is durable while model discovery is still in flight.
		view.Open, view.Busy, view.Cancelling = false, false, false
		r.applyProviderCredential(c, attempt, value.Credential)
		r.refreshProviderSnapshot()
		return
	}
	expiry, terminal := projectAuthorization(view, value.State)
	if providerUI := r.App.ProviderUI; value.State.Kind == "browser" && view.URL != "" && providerUI != nil && providerUI.ProviderAuthorization() == view {
		c.OpenURL(view.URL)
	}
	if value.State.Kind == "failed" {
		failure, _ := detail[protocol.Fault](value.State.Data)
		r.reportKeys(c, &failure, "provider_login_failed", "provider_revision_changed", "provider_login_unknown")
		if attempt.recovery == nil {
			r.providerLoginFinished(providerIDFromString(attempt.provider), false)
		}
	}
	if terminal {
		attempt.cancel()
		if value.State.Kind == "cancelled" && attempt.recovery == nil {
			r.providerLoginFinished(providerIDFromString(attempt.provider), false)
		}
	}
	if value.State.Kind == "connected" {
		if value.ModelError != nil {
			c.AddToast(ui.Toast{Type: "warning", Title: i18n.T(c, "provider_login_models_failed"), Timeout: kit.ToastDuration(c)})
		} else {
			kit.Toast(c, kit.Done, i18n.T(c, "authorization_connected"))
		}
		r.applyProviderCredential(c, attempt, value.Credential)
		r.refreshProviderSnapshot()
	}
	if expiry != 0 {
		r.authorizationCountdown(attempt, view, expiry, true)
	}
}

func (r *Runtime) applyProviderCredential(c *ui.Context, attempt *liveAuthorization, credential *protocol.CredentialRef) {
	if attempt.connected {
		return
	}
	attempt.connected = true
	r.authorizations.credentialKey = ""
	r.readProviders()
	if attempt.recovery != nil {
		r.completeControllerLogin(c, attempt, credential)
	}
	r.providerLoginFinished(providerIDFromString(attempt.provider), true)
}

func (r *Runtime) authorizationCountdown(attempt *liveAuthorization, view *settings.Authorization, expiry uint64, provider bool) {
	attempt.generation++
	generation := attempt.generation
	r.observe(func() {
		ticker := time.NewTicker(time.Second)
		defer ticker.Stop()
		for {
			remaining := time.Until(time.UnixMilli(int64(expiry)))
			r.enqueue(func(*ui.Context) {
				current := r.authorizations.connector
				if provider {
					current = r.authorizations.provider
				}
				if current != attempt || attempt.generation != generation || !view.Open {
					return
				}
				if remaining <= 0 {
					view.Flow = settings.FlowExpired
				} else {
					seconds := int(remaining.Seconds())
					view.Expires = fmt.Sprintf("%d:%02d", seconds/60, seconds%60)
				}
			})
			if remaining <= 0 {
				return
			}
			select {
			case <-attempt.ctx.Done():
				return
			case <-ticker.C:
			}
		}
	})
}

func (r *Runtime) submitProviderLogin(attempt *liveAuthorization, callback string) {
	if r.authorizations.provider != attempt || attempt.id == (protocol.RequestID{}) || attempt.ctx.Err() != nil {
		return
	}
	view := attempt.view
	view.Busy = true
	r.execute(protocol.Command{Kind: "complete_provider_login", Data: protocol.CommandCompleteProviderLoginPayload{Attempt: attempt.id, Callback: protocol.NewSecret(callback)}}, func(c *ui.Context, _ protocol.Output, err error) {
		if r.authorizations.provider != attempt {
			return
		}
		view.Busy = false
		r.reportKeys(c, err, "provider_login_failed", "provider_revision_changed", "provider_login_unknown")
		if err == nil {
			view.Pasted = ""
		}
	})
}

func (r *Runtime) cancelProviderLogin() {
	attempt := r.authorizations.provider
	if attempt == nil {
		return
	}
	view := attempt.view
	if attempt.connected {
		attempt.cancel()
		view.Open, view.Cancelling = false, false
		r.authorizations.provider = nil
		return
	}
	view.Cancelling = true
	// An attempt the Node never began or has already ended (its context is
	// over) has nothing to cancel there: the dialog closes by itself.
	if attempt.id == (protocol.RequestID{}) || attempt.ctx.Err() != nil {
		attempt.cancel()
		view.Open, view.Cancelling = false, false
		r.authorizations.provider = nil
		if attempt.recovery == nil {
			r.providerLoginFinished(providerIDFromString(attempt.provider), false)
		}
		return
	}
	r.execute(protocol.Command{Kind: "cancel_provider_login", Data: protocol.CommandCancelProviderLoginPayload{Attempt: attempt.id}}, func(c *ui.Context, _ protocol.Output, err error) {
		if r.authorizations.provider != attempt {
			return
		}
		view.Cancelling = false
		r.reportKeys(c, err, "provider_login_failed", "provider_revision_changed", "provider_login_unknown")
		if err == nil {
			view.Open = false
			if attempt.recovery == nil {
				r.providerLoginFinished(providerIDFromString(attempt.provider), false)
			}
			attempt.cancel()
		}
	})
}

func (r *Runtime) providerCredentialKey(providers []protocol.ConversationProvider) string {
	var key strings.Builder
	for _, provider := range providers {
		if provider.Authentication != protocol.AuthenticationApiKey && provider.Credential != nil {
			key.WriteString(provider.Credential.ID.String())
			key.WriteByte(':')
			key.WriteString(strconv.FormatUint(provider.Revision, 10))
			key.WriteByte('/')
		}
	}
	if key.Len() != 0 {
		key.WriteString(strconv.FormatUint(r.controller.CredentialMetadataRevision(), 10))
	}
	return key.String()
}

func (r *Runtime) readProviderCredentials() {
	if r.authorizations == nil || r.authorizations.credentialLoad || !r.controller.Connected() {
		return
	}
	expected := r.providerCredentialKey(r.snapshot().Providers)
	if expected == "" || expected == r.authorizations.credentialKey {
		return
	}
	r.authorizations.credentialLoad = true
	r.async(func() {
		output, err := r.controller.Do(r.ctx, protocol.Command{Kind: "list_credentials"})
		r.enqueue(func(*ui.Context) {
			r.authorizations.credentialLoad = false
			values, ok := detail[[]protocol.Credential](output.Data)
			if err != nil || !ok {
				return
			}
			if expected == r.providerCredentialKey(r.snapshot().Providers) {
				r.authorizations.credentials = make(map[protocol.CredentialID]protocol.Credential, len(values))
				for _, value := range values {
					r.authorizations.credentials[value.ID] = value
				}
				r.authorizations.credentialKey = expected
			}
			r.projectProviderSnapshot()
		})
	})
}

func quotaRemaining(window *protocol.ConversationQuotaWindow, now uint64) float64 {
	if window == nil || window.ResetAtMs != nil && *window.ResetAtMs <= now {
		return -1
	}
	return float64(10000-min(window.UsedBasisPoints, 10000)) / 100
}

func (r *Runtime) refreshQuotaReset(now uint64, quotas []protocol.ConversationQuotaSnapshot) {
	if r.authorizations == nil {
		return
	}
	var reset uint64
	for _, quota := range quotas {
		for _, window := range []*protocol.ConversationQuotaWindow{quota.FiveHour, quota.SevenDay} {
			if window != nil && window.ResetAtMs != nil && *window.ResetAtMs > now && (reset == 0 || *window.ResetAtMs < reset) {
				reset = *window.ResetAtMs
			}
		}
	}
	state := r.authorizations
	if state.quotaReset == reset {
		return
	}
	if state.quotaCancel != nil {
		state.quotaCancel()
		state.quotaCancel = nil
	}
	state.quotaReset = reset
	if reset == 0 {
		return
	}
	ctx, cancel := context.WithCancel(r.ctx)
	state.quotaCancel = cancel
	r.observe(func() {
		timer := time.NewTimer(max(0, time.Until(time.UnixMilli(int64(reset)))))
		defer timer.Stop()
		select {
		case <-ctx.Done():
			return
		case <-timer.C:
			r.enqueue(func(*ui.Context) {
				if r.authorizations == state && state.quotaReset == reset {
					state.quotaReset, state.quotaCancel = 0, nil
					r.projectProviderSnapshot()
				}
			})
		}
	})
}

func callbackPresentation() *protocol.PluginModelAuthPresentation {
	return &protocol.PluginModelAuthPresentation{Kind: "pkce", Data: protocol.PluginModelAuthPresentationPkcePayload{Redirect: protocol.PluginModelAuthRedirectLoopback}}
}
