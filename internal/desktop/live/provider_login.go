package live

import (
	"context"
	"fmt"
	"net"
	"net/http"
	"reflect"
	"strconv"
	"strings"
	"sync"
	"time"

	"github.com/egoist/mygo/ui"
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
	packages                       map[string]protocol.PluginInfo
	packagesReading, packagesStale bool
	quotaReset                     uint64
	quotaCancel                    context.CancelFunc
	mcpDrafts                      map[string]protocol.PluginMcpState
	mcpConfiguring                 string
	mcpChecking                    map[string]bool // package and server of each check in flight
	mcpFailed                      map[string]mcpFailure
	connectorBusy                  map[string]bool // services with a command under way
}

type liveAuthorization struct {
	id         protocol.RequestID
	cancel     context.CancelFunc
	provider   string
	connector  settings.ConnectorDraft
	title      string
	generation uint64
	ctx        context.Context
}

func localAuthorization(ctx context.Context, presentation *protocol.PluginModelAuthPresentation) (*string, <-chan string, bool, error) {
	path, port, manual, needed := "/auth/callback", uint16(0), false, false
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
				needed = !manual
				path = value.Callback.Path
				if value.Callback.Port != nil {
					port = *value.Callback.Port
				}
			}
		}
	}
	if !needed {
		return nil, nil, manual, nil
	}
	listener, err := net.Listen("tcp4", net.JoinHostPort("127.0.0.1", strconv.Itoa(int(port))))
	if err != nil {
		return nil, nil, false, err
	}
	redirect := "http://" + listener.Addr().String() + path
	returned := make(chan string, 1)
	var once sync.Once
	server := &http.Server{MaxHeaderBytes: 16 * 1024}
	server.Handler = http.HandlerFunc(func(writer http.ResponseWriter, request *http.Request) {
		if request.Method != http.MethodGet || request.URL.Path != path || len(request.URL.RawQuery) > 16*1024 {
			writer.WriteHeader(http.StatusBadRequest)
			return
		}
		accepted := false
		once.Do(func() {
			select {
			case returned <- redirect + "?" + request.URL.RawQuery:
				accepted = true
			case <-ctx.Done():
			}
		})
		if !accepted {
			writer.WriteHeader(http.StatusConflict)
			return
		}
		writer.WriteHeader(http.StatusNoContent)
	})
	go func() { _ = server.Serve(listener) }()
	go func() {
		<-ctx.Done()
		_ = server.Close()
	}()
	return &redirect, returned, false, nil
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
		if provider.Preset != nil && provider.Preset.ID == entry.Preset.ID && reflect.DeepEqual(provider.Preset.Source, entry.Source) {
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
	provider, ok := r.loginProvider(id)
	if !ok {
		return
	}
	if entry, found := r.loginEntry(provider); found && entry.LoginWarning != nil {
		r.App.Settings.WarnProviderAuthorization(provider.Name, r.loginWarning(entry), func() { r.startProviderLogin(id) })
		return
	}
	r.startProviderLogin(id)
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

func (r *Runtime) startProviderLogin(id string) {
	provider, ok := r.loginProvider(id)
	if !ok {
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
	redirect, returned, manual, err := localAuthorization(ctx, presentation)
	if err != nil {
		cancel()
		r.enqueue(func(c *ui.Context) {
			r.reportKeys(c, err, "provider_login_failed", "provider_revision_changed", "provider_login_unknown")
		})
		return
	}
	attempt := &liveAuthorization{provider: id, ctx: ctx, cancel: cancel, title: provider.Name}
	r.authorizations.provider = attempt
	r.App.Settings.BeginProviderAuthorization(id, provider.Name, manual)
	view := r.App.Settings.ProviderAuthorization()
	view.OpenURL = func(value string) { r.enqueue(func(c *ui.Context) { c.OpenURL(value) }) }
	r.execute(protocol.Command{Kind: "begin_provider_login", Data: protocol.CommandBeginProviderLoginPayload{Provider: provider.ID, ExpectedRevision: provider.Revision, Redirect: redirect}}, func(c *ui.Context, output protocol.Output, err error) {
		if r.authorizations.provider != attempt {
			return
		}
		if err != nil {
			cancel()
			view.Flow = settings.FlowFailed
			r.reportKeys(c, err, "provider_login_failed", "provider_revision_changed", "provider_login_unknown")
			return
		}
		value, ok := detail[protocol.ConversationLoginAttempt](output.Data)
		if !ok {
			cancel()
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
						if r.authorizations.provider == attempt {
							r.submitProviderLogin(callback)
						}
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
				r.reportKeys(c, err, "provider_login_failed", "provider_revision_changed", "provider_login_unknown")
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
	view := r.App.Settings.ProviderAuthorization()
	expiry, terminal := projectAuthorization(view, value.State)
	if value.State.Kind == "browser" && view.URL != "" {
		c.OpenURL(view.URL)
	}
	if value.State.Kind == "failed" {
		failure, _ := detail[protocol.Fault](value.State.Data)
		r.reportKeys(c, &failure, "provider_login_failed", "provider_revision_changed", "provider_login_unknown")
	}
	if terminal {
		attempt.cancel()
	}
	if value.State.Kind == "connected" {
		kit.Toast(c, kit.Done, i18n.T(c, "authorization_connected"))
		r.authorizations.credentialKey = ""
		r.readProviders()
		if value.ModelError != nil {
			r.reportKeys(c, value.ModelError, "provider_query_failed", "provider_revision_changed", "provider_login_unknown")
		}
	}
	if expiry != 0 {
		r.authorizationCountdown(attempt, view, expiry, true)
	}
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

func (r *Runtime) submitProviderLogin(callback string) {
	attempt := r.authorizations.provider
	if attempt == nil || attempt.id == (protocol.RequestID{}) {
		return
	}
	view := r.App.Settings.ProviderAuthorization()
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
	view := r.App.Settings.ProviderAuthorization()
	view.Cancelling = true
	// An attempt the Node never began or has already ended (its context is
	// over) has nothing to cancel there: the dialog closes by itself.
	if attempt.id == (protocol.RequestID{}) || attempt.ctx.Err() != nil {
		attempt.cancel()
		view.Open, view.Cancelling = false, false
		r.authorizations.provider = nil
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
			attempt.cancel()
		}
	})
}

func (r *Runtime) readProviderCredentials() {
	if r.authorizations == nil || r.authorizations.credentialLoad {
		return
	}
	var key strings.Builder
	for _, provider := range r.snapshot().Providers {
		if provider.Credential != nil {
			key.WriteString(provider.Credential.ID.String())
			key.WriteString(strconv.FormatUint(provider.Revision, 10))
		}
	}
	if key.String() == r.authorizations.credentialKey {
		return
	}
	r.authorizations.credentialLoad = true
	expected := key.String()
	r.execute(protocol.Command{Kind: "list_credentials"}, func(c *ui.Context, output protocol.Output, err error) {
		r.authorizations.credentialLoad = false
		if err != nil {
			r.reportKeys(c, err, "provider_login_failed", "provider_revision_changed", "provider_login_unknown")
			return
		}
		values, ok := detail[[]protocol.Credential](output.Data)
		if !ok {
			return
		}
		r.authorizations.credentials = map[protocol.CredentialID]protocol.Credential{}
		for _, value := range values {
			r.authorizations.credentials[value.ID] = value
		}
		r.authorizations.credentialKey = expected
		r.projectProviderSnapshot()
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
