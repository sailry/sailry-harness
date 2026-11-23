package live

import (
	"encoding/json"
	"errors"
	"maps"
	"slices"
	"strings"

	"github.com/egoist/mygo/ui"
	"github.com/sailry/sailry-harness/internal/client"
	"github.com/sailry/sailry-harness/internal/desktop/i18n"
	"github.com/sailry/sailry-harness/internal/desktop/kit"
	"github.com/sailry/sailry-harness/internal/desktop/settings"
	"github.com/sailry/sailry-harness/internal/desktop/shell"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type searchSettingsState struct {
	loaded            bool
	loading           bool
	loadFailed        bool
	pending           bool
	refreshQueued     bool
	observedRev       uint64
	observedProviders uint64
	shown             *Runtime
	saved             *protocol.SearchSettings
	models            []kit.ModelOption
	pendingRequest    *pendingSearchRequest
	actions           settings.SearchActions
}

type pendingSearchRequest struct {
	request protocol.Request
	writing bool
	testing bool
	done    func(ok bool, retry bool)
}

var searchDriverOrder = []protocol.SearchDriver{
	protocol.SearchDriverExa,
	protocol.SearchDriverTinyfish,
	protocol.SearchDriverTavily,
	protocol.SearchDriverBrave,
	protocol.SearchDriverSearxng,
	protocol.SearchDriverBocha,
	protocol.SearchDriverVolcengine,
	protocol.SearchDriverDeepseek,
	protocol.SearchDriverModel,
}

func searchErrorKey(err error, writing, testing bool) string {
	var fault *protocol.Fault
	if errors.As(err, &fault) && fault != nil {
		if fault.Details != nil {
			if fault.Details.Kind == "search_unsupported" {
				return "search_not_supported"
			}
			if testing && fault.Details.Kind == "model_request" {
				if raw, mErr := json.Marshal(fault.Details.Data); mErr == nil {
					var info struct {
						Reason string `json:"reason"`
					}
					if json.Unmarshal(raw, &info) == nil && info.Reason == "unauthorized" {
						return "search_test_auth_failed"
					}
				}
			}
		}
		switch fault.Code {
		case protocol.ErrorCodeExpired:
			return "search_sign_in"
		case protocol.ErrorCodeRevisionConflict:
			return "search_settings_conflict"
		case protocol.ErrorCodeInvalidRequest:
			if !testing {
				return "search_settings_invalid"
			}
		case protocol.ErrorCodeNotConfigured:
			return "search_not_configured"
		case protocol.ErrorCodePermissionDenied:
			if testing {
				return "search_test_auth_failed"
			}
		case protocol.ErrorCodeUnavailable, protocol.ErrorCodeOutcomeUnknown:
			if writing {
				return "search_save_unknown"
			}
		}
	}
	if writing && (faultCode(err) == protocol.ErrorCodeUnavailable || faultCode(err) == protocol.ErrorCodeOutcomeUnknown) {
		return "search_save_unknown"
	}
	return "search_request_failed"
}

func cloneSearchSettings(src protocol.SearchSettings) protocol.SearchSettings {
	out := src
	if src.Driver != nil {
		sel := *src.Driver
		if sel.Kind == "driver" {
			switch v := sel.Data.(type) {
			case *protocol.SearchDriver:
				if v != nil {
					d := *v
					sel.Data = &d
				}
			case protocol.SearchDriver:
				d := v
				sel.Data = &d
			}
		}
		out.Driver = &sel
	}
	out.Order = slices.Clone(src.Order)
	out.Providers = make(map[protocol.SearchDriver]protocol.SearchConfig, len(src.Providers))
	for k, cfg := range src.Providers {
		copied := cfg
		if cfg.Endpoint != nil {
			ep := *cfg.Endpoint
			copied.Endpoint = &ep
		}
		if cfg.Model != nil {
			mod := *cfg.Model
			copied.Model = &mod
		}
		out.Providers[k] = copied
	}
	out.Capabilities = maps.Clone(src.Capabilities)
	return out
}

func searchSelectionName(sel *protocol.SearchSelection) string {
	if sel == nil {
		return "none"
	}
	if sel.Kind == "auto" {
		return "auto"
	}
	if sel.Kind == "driver" {
		switch v := sel.Data.(type) {
		case *protocol.SearchDriver:
			if v != nil {
				return string(*v)
			}
		case protocol.SearchDriver:
			return string(v)
		case string:
			return v
		}
	}
	return "none"
}

func buildSearchDriverViews(saved *protocol.SearchSettings) []settings.SearchDriver {
	if saved == nil {
		return nil
	}
	order := saved.Order
	if len(order) == 0 {
		order = searchDriverOrder
	}
	out := make([]settings.SearchDriver, 0, len(order))
	for _, driver := range order {
		if !slices.Contains(searchDriverOrder, driver) {
			continue
		}
		cfg, exists := saved.Providers[driver]
		if !exists {
			continue
		}
		capInfo := saved.Capabilities[driver]
		row := settings.SearchDriver{
			ID:              string(driver),
			Enabled:         cfg.Enabled,
			KeyConfigured:   cfg.KeyConfigured,
			KeyRequired:     capInfo.KeyRequired,
			BasicConfigured: cfg.BasicConfigured,
			Quota:           string(capInfo.Quota),
		}
		if cfg.Endpoint != nil {
			row.Endpoint = *cfg.Endpoint
		}
		if cfg.Model != nil {
			row.ModelProvider = cfg.Model.Provider.String()
			row.ModelID = cfg.Model.Model
		}
		out = append(out, row)
	}
	return out
}

func filterSearchModels(catalog protocol.PluginModelsCatalog) []kit.ModelOption {
	var out []kit.ModelOption
	for _, item := range catalog.Models {
		if item.Kind != protocol.PluginModelsKindProvider || item.Api == nil {
			continue
		}
		api := string(*item.Api)
		if api != "responses" && api != "anthropic" && api != "gemini" {
			continue
		}
		if string(item.Authentication) == "copilot" {
			continue
		}
		_, modelName, ok := strings.Cut(item.ID, "/")
		if !ok || modelName == "" {
			continue
		}
		out = append(out, kit.ModelOption{
			ID:       item.ID,
			Name:     modelName,
			Provider: item.Provider,
		})
	}
	return out
}

func (r *Runtime) searchSettingsFrame() {
	root := r.localRuntime()
	m := root.App.Settings
	visible := root.App.Page == shell.SettingsPage && m.Section == settings.Search
	var owner *Runtime
	if visible {
		owner = root.managedSettingsNode()
	}
	if root.searchSettings.shown != owner {
		if prev := root.searchSettings.shown; prev != nil {
			if m.SearchPageState != nil {
				m.SearchPageState.CloseDialogs()
			}
			if !prev.searchSettings.pending {
				prev.searchSettings.pendingRequest = nil
			}
		}
		root.searchSettings.shown = owner
	}
	if !visible {
		return
	}
	if owner == nil {
		m.Search = settings.SearchSettingsState{Status: "connections_offline"}
		m.SearchActions = nil
		return
	}
	st := &owner.searchSettings
	if st.actions.Reload == nil {
		st.actions = settings.SearchActions{
			Reload:      owner.loadSearchSettings,
			Retry:       owner.retrySearchRequest,
			SetDefault:  owner.setSearchDefaultDriver,
			SetFallback: owner.setSearchFallback,
			SetEnabled:  owner.setSearchDriverEnabled,
			Reorder:     owner.reorderSearchDrivers,
			Test:        owner.testSearchDriver,
			SaveDriver:  owner.saveSearchDriver,
			ForgetPending: func() {
				st.pendingRequest = nil
			},
		}
	}
	snap := owner.snapshot()
	providersRev := owner.controller.ProviderRevision()
	if ((!st.loaded && !st.loadFailed) || snap.SearchSettingsRevision > st.observedRev || (st.loaded && providersRev != st.observedProviders)) && !st.loading && !st.pending && st.pendingRequest == nil {
		owner.loadSearchSettings()
	}

	view := settings.SearchSettingsState{
		Loaded:       st.loaded,
		Loading:      st.loading,
		Pending:      st.pending,
		Connected:    owner.controller.Connected(),
		Retryable:    st.pendingRequest != nil && st.pendingRequest.writing,
		ModelOptions: st.models,
	}
	if st.saved != nil {
		view.Revision = st.saved.Revision
		view.DefaultDriver = searchSelectionName(st.saved.Driver)
		view.Fallback = st.saved.Fallback
		view.Drivers = buildSearchDriverViews(st.saved)
	}
	m.Search = view
	m.SearchActions = &st.actions
}

func (r *Runtime) loadSearchSettings() {
	st := &r.searchSettings
	if !r.controller.Connected() {
		return
	}
	if st.loading || st.pending || st.pendingRequest != nil {
		st.refreshQueued = true
		return
	}
	st.loading = true
	st.loadFailed = false
	st.observedRev = r.snapshot().SearchSettingsRevision
	providersRev := r.controller.ProviderRevision()
	r.async(func() {
		out, err := r.controller.Do(r.ctx, protocol.Command{Kind: "read_search_settings"})
		modelsOut, modelsErr := r.controller.Do(r.ctx, protocol.Command{Kind: "list_plugin_models"})
		r.enqueue(func(c *ui.Context) {
			st.loading = false
			defer func() {
				if st.refreshQueued {
					st.refreshQueued = false
					r.loadSearchSettings()
				}
			}()
			if err != nil {
				st.loadFailed = true
				kit.Toast(c, kit.Failed, i18n.T(c, searchErrorKey(err, false, false)))
				return
			}
			val, ok := detail[protocol.SearchSettings](out.Data)
			if !ok || out.Kind != "search_settings" {
				st.loadFailed = true
				kit.Toast(c, kit.Failed, i18n.T(c, "search_request_failed"))
				return
			}
			copied := cloneSearchSettings(val)
			st.saved = &copied
			st.loaded = true
			st.loadFailed = false
			st.observedRev = val.Revision
			st.observedProviders = providersRev
			if modelsErr == nil && modelsOut.Kind == "plugin_models" {
				if cat, valid := detail[protocol.PluginModelsCatalog](modelsOut.Data); valid {
					st.models = filterSearchModels(cat)
				}
			}
		})
	})
}

func (r *Runtime) searchBusy() bool {
	st := &r.searchSettings
	return st.saved == nil || !r.controller.Connected() || st.loading || st.pending || st.pendingRequest != nil
}

func (r *Runtime) startSearchRequest(cmd protocol.Command, writing, testing bool, done func(ok bool, retry bool)) {
	st := &r.searchSettings
	req := r.controller.Prepare(cmd)
	st.pendingRequest = &pendingSearchRequest{request: req, writing: writing, testing: testing, done: done}
	r.retrySearchRequest()
}

func (r *Runtime) retrySearchRequest() bool {
	st := &r.searchSettings
	if st.pendingRequest == nil || st.pending || !r.controller.Connected() {
		return false
	}
	pending := st.pendingRequest
	st.pending = true
	r.async(func() {
		var out protocol.Output
		var err error
		if pending.writing {
			out, err = r.controller.Settle(r.ctx, pending.request)
		} else {
			out, err = r.controller.Execute(r.ctx, pending.request)
		}
		r.enqueue(func(c *ui.Context) {
			st.pending = false
			if err != nil {
				code := faultCode(err)
				key := searchErrorKey(err, pending.writing, pending.testing)
				kit.Toast(c, kit.Failed, i18n.T(c, key))
				if pending.writing && !client.Released(err) && (code == protocol.ErrorCodeOutcomeUnknown || code == protocol.ErrorCodeUnavailable) {
					if pending.done != nil {
						pending.done(false, true)
					}
					return
				}
				st.pendingRequest = nil
				if pending.done != nil {
					pending.done(false, false)
				}
				if pending.writing {
					r.loadSearchSettings()
				}
				return
			}
			updated := false
			if val, ok := detail[protocol.SearchSettings](out.Data); ok && out.Kind == "search_settings" {
				copied := cloneSearchSettings(val)
				st.saved = &copied
				st.observedRev = val.Revision
				updated = true
			}
			toastKey := "search_saved"
			if pending.testing {
				toastKey = "search_test_passed"
			}
			kit.Toast(c, kit.Done, i18n.T(c, toastKey))
			st.pendingRequest = nil
			if pending.done != nil {
				pending.done(true, false)
			}
			if pending.writing && !updated {
				r.loadSearchSettings()
			}
		})
	})
	return true
}

func (r *Runtime) setSearchDefaultDriver(choice string) {
	if r.searchBusy() {
		return
	}
	next := cloneSearchSettings(*r.searchSettings.saved)
	switch choice {
	case "none", "":
		next.Driver = nil
	case "auto":
		next.Driver = &protocol.SearchSelection{Kind: "auto"}
	default:
		driver := protocol.SearchDriver(choice)
		cfg, ok := next.Providers[driver]
		if !ok {
			return
		}
		cfg.Enabled = true
		next.Providers[driver] = cfg
		next.Driver = &protocol.SearchSelection{Kind: "driver", Data: &driver}
	}
	r.startSearchRequest(protocol.Command{
		Kind: "save_search_settings",
		Data: protocol.SearchUpdate{Settings: next, Credentials: map[protocol.SearchDriver]protocol.SearchCredentials{}},
	}, true, false, nil)
}

func (r *Runtime) setSearchFallback(enabled bool) {
	if r.searchBusy() {
		return
	}
	next := cloneSearchSettings(*r.searchSettings.saved)
	next.Fallback = enabled
	r.startSearchRequest(protocol.Command{
		Kind: "save_search_settings",
		Data: protocol.SearchUpdate{Settings: next, Credentials: map[protocol.SearchDriver]protocol.SearchCredentials{}},
	}, true, false, nil)
}

func (r *Runtime) setSearchDriverEnabled(driver string, enabled bool) {
	if r.searchBusy() {
		return
	}
	next := cloneSearchSettings(*r.searchSettings.saved)
	key := protocol.SearchDriver(driver)
	cfg, ok := next.Providers[key]
	if !ok {
		return
	}
	cfg.Enabled = enabled
	next.Providers[key] = cfg
	r.startSearchRequest(protocol.Command{
		Kind: "save_search_settings",
		Data: protocol.SearchUpdate{Settings: next, Credentials: map[protocol.SearchDriver]protocol.SearchCredentials{}},
	}, true, false, nil)
}

func (r *Runtime) reorderSearchDrivers(order []string) {
	if r.searchBusy() || len(order) == 0 {
		return
	}
	next := cloneSearchSettings(*r.searchSettings.saved)
	drivers := make([]protocol.SearchDriver, 0, len(order))
	for _, id := range order {
		drivers = append(drivers, protocol.SearchDriver(id))
	}
	next.Order = drivers
	// Reflect dropped order immediately; reload on error restores saved order.
	r.searchSettings.saved.Order = slices.Clone(drivers)
	r.startSearchRequest(protocol.Command{
		Kind: "save_search_settings",
		Data: protocol.SearchUpdate{Settings: next, Credentials: map[protocol.SearchDriver]protocol.SearchCredentials{}},
	}, true, false, nil)
}

func (r *Runtime) testSearchDriver(driver string) {
	if r.searchBusy() {
		return
	}
	r.startSearchRequest(protocol.Command{
		Kind: "test_search_driver",
		Data: protocol.CommandTestSearchDriverPayload{Driver: protocol.SearchDriver(driver)},
	}, false, true, nil)
}

func (r *Runtime) saveSearchDriver(draft settings.SearchDriverDraft, done func(ok bool, retry bool)) {
	if r.searchBusy() {
		done(false, false)
		return
	}
	driver := protocol.SearchDriver(draft.Driver)
	next := cloneSearchSettings(*r.searchSettings.saved)
	cfg, ok := next.Providers[driver]
	if !ok {
		done(false, false)
		return
	}
	creds := map[protocol.SearchDriver]protocol.SearchCredentials{}
	if driver == protocol.SearchDriverModel {
		if draft.ModelProvider != "" && draft.ModelID != "" {
			provID, err := protocol.ParseProviderID(draft.ModelProvider)
			if err != nil {
				r.enqueue(func(c *ui.Context) {
					kit.Toast(c, kit.Failed, i18n.T(c, "search_settings_invalid"))
				})
				done(false, false)
				return
			}
			cfg.Model = &protocol.SearchModelConfig{Provider: provID, Model: draft.ModelID}
		} else {
			cfg.Model = nil
		}
	} else {
		if driver == protocol.SearchDriverSearxng {
			if ep := strings.TrimSpace(draft.Endpoint); ep != "" {
				cfg.Endpoint = &ep
			} else {
				cfg.Endpoint = nil
			}
		}
		keyUpd := protocol.PluginSettingsSecretUpdate{Kind: "keep"}
		switch draft.Key.Action {
		case "set":
			keyUpd = protocol.PluginSettingsSecretUpdate{Kind: "replace", Data: protocol.NewSecret(draft.Key.Value)}
		case "clear":
			keyUpd = protocol.PluginSettingsSecretUpdate{Kind: "clear"}
		}
		basicUpd := protocol.PluginSettingsSecretUpdate{Kind: "keep"}
		switch draft.Basic.Action {
		case "set":
			if draft.Basic.Username == "" || strings.Contains(draft.Basic.Username, ":") || draft.Basic.Password == "" {
				r.enqueue(func(c *ui.Context) {
					kit.Toast(c, kit.Failed, i18n.T(c, "search_basic_invalid"))
				})
				done(false, false)
				return
			}
			basicUpd = protocol.PluginSettingsSecretUpdate{Kind: "replace", Data: protocol.NewSecret(draft.Basic.Username + ":" + draft.Basic.Password)}
		case "clear":
			basicUpd = protocol.PluginSettingsSecretUpdate{Kind: "clear"}
		}
		creds[driver] = protocol.SearchCredentials{Key: keyUpd, Basic: basicUpd}
	}
	next.Providers[driver] = cfg
	r.startSearchRequest(protocol.Command{
		Kind: "save_search_settings",
		Data: protocol.SearchUpdate{Settings: next, Credentials: creds},
	}, true, false, done)
}
