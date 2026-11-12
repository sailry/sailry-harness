// Code generated from the reviewed v1 protocol declarations. DO NOT EDIT.

package protocol

import (
	"encoding/json"
)

type PluginModelAuthBinding struct {
	Flow string `json:"flow"`
}

func DefaultPluginModelAuthBinding() PluginModelAuthBinding {
	return PluginModelAuthBinding{
		Flow: "",
	}
}
func (value *PluginModelAuthBinding) UnmarshalJSON(data []byte) error {
	type plain PluginModelAuthBinding
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"flow"}, true); err != nil {
		return err
	}
	*value = PluginModelAuthBinding(decoded)
	return nil
}
func (value PluginModelAuthBinding) MarshalJSON() ([]byte, error) {
	type plain PluginModelAuthBinding
	encoded := plain(value)
	return json.Marshal(encoded)
}

type PluginModelAuthDeclaration struct {
	ID             string                             `json:"id"`
	Authentication *Authentication                    `json:"authentication,omitempty"`
	Display        PluginDesktopNavigation            `json:"display"`
	Flow           PluginModelAuthFlow                `json:"flow"`
	Client         PluginModelAuthClient              `json:"client"`
	Scopes         []string                           `json:"scopes"`
	Token          *PluginModelAuthMapping            `json:"token,omitempty"`
	Inject         []PluginModelAuthInjection         `json:"inject"`
	Refresh        *PluginModelAuthRefresh            `json:"refresh"`
	Exchange       *PluginModelAuthTransformation     `json:"exchange"`
	Requests       map[string]PluginModelAuthEndpoint `json:"requests"`
	Mutations      map[string]PluginModelAuthEndpoint `json:"mutations"`
	Warning        *PluginModelAuthWarning            `json:"warning,omitempty"`
}

func DefaultPluginModelAuthDeclaration() PluginModelAuthDeclaration {
	return PluginModelAuthDeclaration{
		ID:             "",
		Authentication: nil,
		Display:        DefaultPluginDesktopNavigation(),
		Flow:           PluginModelAuthFlow{},
		Client:         DefaultPluginModelAuthClient(),
		Scopes:         []string{},
		Token:          nil,
		Inject:         []PluginModelAuthInjection{},
		Refresh:        nil,
		Exchange:       nil,
		Requests:       map[string]PluginModelAuthEndpoint{},
		Mutations:      map[string]PluginModelAuthEndpoint{},
		Warning:        nil,
	}
}
func (value *PluginModelAuthDeclaration) UnmarshalJSON(data []byte) error {
	type plain PluginModelAuthDeclaration
	decoded := plain{}
	decoded.Authentication = nil
	decoded.Client = DefaultPluginModelAuthClient()
	decoded.Scopes = []string{}
	decoded.Token = nil
	decoded.Inject = []PluginModelAuthInjection{}
	decoded.Refresh = nil
	decoded.Exchange = nil
	decoded.Requests = map[string]PluginModelAuthEndpoint{}
	decoded.Mutations = map[string]PluginModelAuthEndpoint{}
	decoded.Warning = nil
	if err := DecodeObject(data, &decoded, []string{"id", "display", "flow"}, true); err != nil {
		return err
	}
	*value = PluginModelAuthDeclaration(decoded)
	return nil
}
func (value PluginModelAuthDeclaration) MarshalJSON() ([]byte, error) {
	type plain PluginModelAuthDeclaration
	encoded := plain(value)
	encoded.Scopes = normalizeCollections(encoded.Scopes)
	encoded.Inject = normalizeCollections(encoded.Inject)
	encoded.Requests = normalizeCollections(encoded.Requests)
	encoded.Mutations = normalizeCollections(encoded.Mutations)
	return json.Marshal(encoded)
}

// PluginModelAuthWarning is what a controller asks the user to accept before
// a sign-in of this declaration starts: one text and its translations.
type PluginModelAuthWarning struct {
	Text    string            `json:"text"`
	Locales map[string]string `json:"locales"`
}

func DefaultPluginModelAuthWarning() PluginModelAuthWarning {
	return PluginModelAuthWarning{
		Text:    "",
		Locales: map[string]string{},
	}
}
func (value *PluginModelAuthWarning) UnmarshalJSON(data []byte) error {
	type plain PluginModelAuthWarning
	decoded := plain{}
	decoded.Locales = map[string]string{}
	if err := DecodeObject(data, &decoded, []string{"text"}, true); err != nil {
		return err
	}
	*value = PluginModelAuthWarning(decoded)
	return nil
}
func (value PluginModelAuthWarning) MarshalJSON() ([]byte, error) {
	type plain PluginModelAuthWarning
	encoded := plain(value)
	encoded.Locales = normalizeCollections(encoded.Locales)
	return json.Marshal(encoded)
}

type PluginModelAuthResource struct {
	Authentication Authentication                     `json:"authentication"`
	Requests       map[string]PluginModelAuthEndpoint `json:"requests"`
}

func DefaultPluginModelAuthResource() PluginModelAuthResource {
	return PluginModelAuthResource{
		Authentication: Authentication(""),
		Requests:       map[string]PluginModelAuthEndpoint{},
	}
}
func (value *PluginModelAuthResource) UnmarshalJSON(data []byte) error {
	type plain PluginModelAuthResource
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"authentication", "requests"}, true); err != nil {
		return err
	}
	*value = PluginModelAuthResource(decoded)
	return nil
}
func (value PluginModelAuthResource) MarshalJSON() ([]byte, error) {
	type plain PluginModelAuthResource
	encoded := plain(value)
	encoded.Requests = normalizeCollections(encoded.Requests)
	return json.Marshal(encoded)
}

type PluginModelAuthFlow struct {
	Kind string
	Data any
}
type PluginModelAuthFlowScriptPayload struct {
	Script   PluginModelAdapterScript           `json:"script"`
	Refresh  bool                               `json:"refresh,omitempty"`
	Handler  string                             `json:"handler"`
	Requests map[string]PluginModelAuthEndpoint `json:"requests"`
	Callback *PluginModelAuthCallback           `json:"callback"`
	Issuer   *string                            `json:"issuer"`
}

func DefaultPluginModelAuthFlowScriptPayload() PluginModelAuthFlowScriptPayload {
	return PluginModelAuthFlowScriptPayload{
		Script:   DefaultPluginModelAdapterScript(),
		Refresh:  false,
		Handler:  "",
		Requests: map[string]PluginModelAuthEndpoint{},
		Callback: nil,
		Issuer:   nil,
	}
}
func (value *PluginModelAuthFlowScriptPayload) UnmarshalJSON(data []byte) error {
	type plain PluginModelAuthFlowScriptPayload
	decoded := plain{}
	decoded.Refresh = false
	if err := DecodeObject(data, &decoded, []string{"script", "handler", "requests"}, true); err != nil {
		return err
	}
	*value = PluginModelAuthFlowScriptPayload(decoded)
	return nil
}
func (value PluginModelAuthFlowScriptPayload) MarshalJSON() ([]byte, error) {
	type plain PluginModelAuthFlowScriptPayload
	encoded := plain(value)
	encoded.Requests = normalizeCollections(encoded.Requests)
	return json.Marshal(encoded)
}

type PluginModelAuthFlowPollingPayload struct {
	Start           PluginModelAuthEndpoint  `json:"start"`
	State           string                   `json:"state"`
	VerificationUrl string                   `json:"verification_url"`
	Token           PluginModelAuthEndpoint  `json:"token"`
	Success         PluginModelAuthCondition `json:"success"`
	IntervalMs      *uint64                  `json:"interval_ms"`
}

func DefaultPluginModelAuthFlowPollingPayload() PluginModelAuthFlowPollingPayload {
	return PluginModelAuthFlowPollingPayload{
		Start:           DefaultPluginModelAuthEndpoint(),
		State:           "",
		VerificationUrl: "",
		Token:           DefaultPluginModelAuthEndpoint(),
		Success:         DefaultPluginModelAuthCondition(),
		IntervalMs:      nil,
	}
}
func (value *PluginModelAuthFlowPollingPayload) UnmarshalJSON(data []byte) error {
	type plain PluginModelAuthFlowPollingPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"start", "state", "verification_url", "token", "success"}, true); err != nil {
		return err
	}
	*value = PluginModelAuthFlowPollingPayload(decoded)
	return nil
}
func (value PluginModelAuthFlowPollingPayload) MarshalJSON() ([]byte, error) {
	type plain PluginModelAuthFlowPollingPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type PluginModelAuthFlowPkcePayload struct {
	Authorize   string                  `json:"authorize"`
	Token       PluginModelAuthEndpoint `json:"token"`
	Redirect    PluginModelAuthRedirect `json:"redirect"`
	RedirectUri *string                 `json:"redirect_uri"`
}

func DefaultPluginModelAuthFlowPkcePayload() PluginModelAuthFlowPkcePayload {
	return PluginModelAuthFlowPkcePayload{
		Authorize:   "",
		Token:       DefaultPluginModelAuthEndpoint(),
		Redirect:    PluginModelAuthRedirect(""),
		RedirectUri: nil,
	}
}
func (value *PluginModelAuthFlowPkcePayload) UnmarshalJSON(data []byte) error {
	type plain PluginModelAuthFlowPkcePayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"authorize", "token", "redirect"}, true); err != nil {
		return err
	}
	*value = PluginModelAuthFlowPkcePayload(decoded)
	return nil
}
func (value PluginModelAuthFlowPkcePayload) MarshalJSON() ([]byte, error) {
	type plain PluginModelAuthFlowPkcePayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type PluginModelAuthFlowDeviceCodePayload struct {
	Device PluginModelAuthEndpoint `json:"device"`
	Token  PluginModelAuthEndpoint `json:"token"`
}

func DefaultPluginModelAuthFlowDeviceCodePayload() PluginModelAuthFlowDeviceCodePayload {
	return PluginModelAuthFlowDeviceCodePayload{
		Device: DefaultPluginModelAuthEndpoint(),
		Token:  DefaultPluginModelAuthEndpoint(),
	}
}
func (value *PluginModelAuthFlowDeviceCodePayload) UnmarshalJSON(data []byte) error {
	type plain PluginModelAuthFlowDeviceCodePayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"device", "token"}, true); err != nil {
		return err
	}
	*value = PluginModelAuthFlowDeviceCodePayload(decoded)
	return nil
}
func (value PluginModelAuthFlowDeviceCodePayload) MarshalJSON() ([]byte, error) {
	type plain PluginModelAuthFlowDeviceCodePayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type PluginModelAuthFlowTokenExchangePayload struct {
	Token PluginModelAuthEndpoint `json:"token"`
}

func DefaultPluginModelAuthFlowTokenExchangePayload() PluginModelAuthFlowTokenExchangePayload {
	return PluginModelAuthFlowTokenExchangePayload{
		Token: DefaultPluginModelAuthEndpoint(),
	}
}
func (value *PluginModelAuthFlowTokenExchangePayload) UnmarshalJSON(data []byte) error {
	type plain PluginModelAuthFlowTokenExchangePayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"token"}, true); err != nil {
		return err
	}
	*value = PluginModelAuthFlowTokenExchangePayload(decoded)
	return nil
}
func (value PluginModelAuthFlowTokenExchangePayload) MarshalJSON() ([]byte, error) {
	type plain PluginModelAuthFlowTokenExchangePayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

var pluginModelAuthFlowVariants = variantFactories{"script": func() any { return new(PluginModelAuthFlowScriptPayload) },
	"polling":        func() any { return new(PluginModelAuthFlowPollingPayload) },
	"pkce":           func() any { return new(PluginModelAuthFlowPkcePayload) },
	"device_code":    func() any { return new(PluginModelAuthFlowDeviceCodePayload) },
	"token_exchange": func() any { return new(PluginModelAuthFlowTokenExchangePayload) }}

func (value PluginModelAuthFlow) MarshalJSON() ([]byte, error) {
	return MarshalUnion("kind", "", value.Kind, value.Data, pluginModelAuthFlowVariants)
}
func (value *PluginModelAuthFlow) UnmarshalJSON(data []byte) error {
	kind, decoded, err := UnmarshalUnion(data, "kind", "", pluginModelAuthFlowVariants, true)
	if err != nil {
		return err
	}
	value.Kind, value.Data = kind, decoded
	return nil
}

type PluginModelAuthPresentation struct {
	Kind string
	Data any
}
type PluginModelAuthPresentationScriptPayload struct {
	Callback *PluginModelAuthCallback `json:"callback"`
}

func DefaultPluginModelAuthPresentationScriptPayload() PluginModelAuthPresentationScriptPayload {
	return PluginModelAuthPresentationScriptPayload{
		Callback: nil,
	}
}
func (value *PluginModelAuthPresentationScriptPayload) UnmarshalJSON(data []byte) error {
	type plain PluginModelAuthPresentationScriptPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{}, true); err != nil {
		return err
	}
	*value = PluginModelAuthPresentationScriptPayload(decoded)
	return nil
}
func (value PluginModelAuthPresentationScriptPayload) MarshalJSON() ([]byte, error) {
	type plain PluginModelAuthPresentationScriptPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type PluginModelAuthPresentationPkcePayload struct {
	Redirect PluginModelAuthRedirect `json:"redirect"`
}

func DefaultPluginModelAuthPresentationPkcePayload() PluginModelAuthPresentationPkcePayload {
	return PluginModelAuthPresentationPkcePayload{
		Redirect: PluginModelAuthRedirect(""),
	}
}
func (value *PluginModelAuthPresentationPkcePayload) UnmarshalJSON(data []byte) error {
	type plain PluginModelAuthPresentationPkcePayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"redirect"}, true); err != nil {
		return err
	}
	*value = PluginModelAuthPresentationPkcePayload(decoded)
	return nil
}
func (value PluginModelAuthPresentationPkcePayload) MarshalJSON() ([]byte, error) {
	type plain PluginModelAuthPresentationPkcePayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

var pluginModelAuthPresentationVariants = variantFactories{"script": func() any { return new(PluginModelAuthPresentationScriptPayload) },
	"pkce":           func() any { return new(PluginModelAuthPresentationPkcePayload) },
	"polling":        nil,
	"device_code":    nil,
	"token_exchange": nil}

func (value PluginModelAuthPresentation) MarshalJSON() ([]byte, error) {
	return MarshalUnion("kind", "", value.Kind, value.Data, pluginModelAuthPresentationVariants)
}
func (value *PluginModelAuthPresentation) UnmarshalJSON(data []byte) error {
	kind, decoded, err := UnmarshalUnion(data, "kind", "", pluginModelAuthPresentationVariants, true)
	if err != nil {
		return err
	}
	value.Kind, value.Data = kind, decoded
	return nil
}

type PluginModelAuthCallback struct {
	Path           string  `json:"path"`
	Port           *uint16 `json:"port,omitempty"`
	RedirectUri    *string `json:"redirect_uri"`
	Manual         bool    `json:"manual"`
	AllowLocalhost bool    `json:"allow_localhost,omitempty"`
}

func DefaultPluginModelAuthCallback() PluginModelAuthCallback {
	return PluginModelAuthCallback{
		Path:           "",
		Port:           nil,
		RedirectUri:    nil,
		Manual:         false,
		AllowLocalhost: false,
	}
}
func (value *PluginModelAuthCallback) UnmarshalJSON(data []byte) error {
	type plain PluginModelAuthCallback
	decoded := plain{}
	decoded.Port = nil
	decoded.AllowLocalhost = false
	if err := DecodeObject(data, &decoded, []string{"path", "manual"}, true); err != nil {
		return err
	}
	*value = PluginModelAuthCallback(decoded)
	return nil
}
func (value PluginModelAuthCallback) MarshalJSON() ([]byte, error) {
	type plain PluginModelAuthCallback
	encoded := plain(value)
	return json.Marshal(encoded)
}

type PluginModelAuthCondition struct {
	Pointer string            `json:"pointer"`
	Values  []json.RawMessage `json:"values"`
}

func DefaultPluginModelAuthCondition() PluginModelAuthCondition {
	return PluginModelAuthCondition{
		Pointer: "",
		Values:  []json.RawMessage{},
	}
}
func (value *PluginModelAuthCondition) UnmarshalJSON(data []byte) error {
	type plain PluginModelAuthCondition
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"pointer", "values"}, true); err != nil {
		return err
	}
	*value = PluginModelAuthCondition(decoded)
	return nil
}
func (value PluginModelAuthCondition) MarshalJSON() ([]byte, error) {
	type plain PluginModelAuthCondition
	encoded := plain(value)
	encoded.Values = normalizeCollections(encoded.Values)
	return json.Marshal(encoded)
}

type PluginModelAuthRedirect string

const (
	PluginModelAuthRedirectLoopback PluginModelAuthRedirect = "loopback"
	PluginModelAuthRedirectPaste    PluginModelAuthRedirect = "paste"
)

func (value PluginModelAuthRedirect) MarshalJSON() ([]byte, error) { return marshalEnum(value) }
func (value *PluginModelAuthRedirect) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch PluginModelAuthRedirect(decoded) {
	case PluginModelAuthRedirectLoopback, PluginModelAuthRedirectPaste:
		*value = PluginModelAuthRedirect(decoded)
		return nil
	default:
		return unknownEnum(data, value, decoded)
	}
}
func (value PluginModelAuthRedirect) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *PluginModelAuthRedirect) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type PluginModelAuthClient struct {
	ClientID            *string `json:"client_id"`
	ClientIdSetting     *string `json:"client_id_setting"`
	ClientSecretSetting *string `json:"client_secret_setting"`
}

func DefaultPluginModelAuthClient() PluginModelAuthClient {
	return PluginModelAuthClient{
		ClientID:            nil,
		ClientIdSetting:     nil,
		ClientSecretSetting: nil,
	}
}
func (value *PluginModelAuthClient) UnmarshalJSON(data []byte) error {
	type plain PluginModelAuthClient
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{}, true); err != nil {
		return err
	}
	*value = PluginModelAuthClient(decoded)
	return nil
}
func (value PluginModelAuthClient) MarshalJSON() ([]byte, error) {
	type plain PluginModelAuthClient
	encoded := plain(value)
	return json.Marshal(encoded)
}

type PluginModelAuthMapping struct {
	Access       string            `json:"access"`
	Refresh      *string           `json:"refresh"`
	ExpiresIn    *string           `json:"expires_in"`
	AccountLabel *string           `json:"account_label,omitempty"`
	Extra        map[string]string `json:"extra"`
}

func DefaultPluginModelAuthMapping() PluginModelAuthMapping {
	return PluginModelAuthMapping{
		Access:       "",
		Refresh:      nil,
		ExpiresIn:    nil,
		AccountLabel: nil,
		Extra:        map[string]string{},
	}
}
func (value *PluginModelAuthMapping) UnmarshalJSON(data []byte) error {
	type plain PluginModelAuthMapping
	decoded := plain{}
	decoded.AccountLabel = nil
	decoded.Extra = map[string]string{}
	if err := DecodeObject(data, &decoded, []string{"access"}, true); err != nil {
		return err
	}
	*value = PluginModelAuthMapping(decoded)
	return nil
}
func (value PluginModelAuthMapping) MarshalJSON() ([]byte, error) {
	type plain PluginModelAuthMapping
	encoded := plain(value)
	encoded.Extra = normalizeCollections(encoded.Extra)
	return json.Marshal(encoded)
}

type PluginModelAuthInjection struct {
	Header   string `json:"header"`
	Template string `json:"template"`
}

func DefaultPluginModelAuthInjection() PluginModelAuthInjection {
	return PluginModelAuthInjection{
		Header:   "",
		Template: "",
	}
}
func (value *PluginModelAuthInjection) UnmarshalJSON(data []byte) error {
	type plain PluginModelAuthInjection
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"header", "template"}, true); err != nil {
		return err
	}
	*value = PluginModelAuthInjection(decoded)
	return nil
}
func (value PluginModelAuthInjection) MarshalJSON() ([]byte, error) {
	type plain PluginModelAuthInjection
	encoded := plain(value)
	return json.Marshal(encoded)
}

type PluginModelAuthRefresh struct {
	Request PluginModelAuthEndpoint `json:"request"`
	Token   PluginModelAuthMapping  `json:"token"`
}

func DefaultPluginModelAuthRefresh() PluginModelAuthRefresh {
	return PluginModelAuthRefresh{
		Request: DefaultPluginModelAuthEndpoint(),
		Token:   DefaultPluginModelAuthMapping(),
	}
}
func (value *PluginModelAuthRefresh) UnmarshalJSON(data []byte) error {
	type plain PluginModelAuthRefresh
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"request", "token"}, true); err != nil {
		return err
	}
	*value = PluginModelAuthRefresh(decoded)
	return nil
}
func (value PluginModelAuthRefresh) MarshalJSON() ([]byte, error) {
	type plain PluginModelAuthRefresh
	encoded := plain(value)
	return json.Marshal(encoded)
}

type PluginModelAuthTransformation struct {
	Script  PluginModelAdapterScript `json:"script"`
	Handler string                   `json:"handler"`
}

func DefaultPluginModelAuthTransformation() PluginModelAuthTransformation {
	return PluginModelAuthTransformation{
		Script:  DefaultPluginModelAdapterScript(),
		Handler: "",
	}
}
func (value *PluginModelAuthTransformation) UnmarshalJSON(data []byte) error {
	type plain PluginModelAuthTransformation
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"script", "handler"}, true); err != nil {
		return err
	}
	*value = PluginModelAuthTransformation(decoded)
	return nil
}
func (value PluginModelAuthTransformation) MarshalJSON() ([]byte, error) {
	type plain PluginModelAuthTransformation
	encoded := plain(value)
	return json.Marshal(encoded)
}

type PluginModelAuthEndpoint struct {
	CacheTtlMs *uint32               `json:"cache_ttl_ms,omitempty"`
	Url        string                `json:"url"`
	Method     PluginModelAuthMethod `json:"method"`
	Headers    map[string]string     `json:"headers"`
	Query      map[string]string     `json:"query"`
	Body       *PluginModelAuthBody  `json:"body"`
}

func DefaultPluginModelAuthEndpoint() PluginModelAuthEndpoint {
	return PluginModelAuthEndpoint{
		CacheTtlMs: nil,
		Url:        "",
		Method:     PluginModelAuthMethodPost,
		Headers:    map[string]string{},
		Query:      map[string]string{},
		Body:       nil,
	}
}
func (value *PluginModelAuthEndpoint) UnmarshalJSON(data []byte) error {
	type plain PluginModelAuthEndpoint
	decoded := plain{}
	decoded.CacheTtlMs = nil
	decoded.Method = PluginModelAuthMethodPost
	decoded.Headers = map[string]string{}
	decoded.Query = map[string]string{}
	decoded.Body = nil
	if err := DecodeObject(data, &decoded, []string{"url"}, true); err != nil {
		return err
	}
	*value = PluginModelAuthEndpoint(decoded)
	return nil
}
func (value PluginModelAuthEndpoint) MarshalJSON() ([]byte, error) {
	type plain PluginModelAuthEndpoint
	encoded := plain(value)
	encoded.Headers = normalizeCollections(encoded.Headers)
	encoded.Query = normalizeCollections(encoded.Query)
	return json.Marshal(encoded)
}

type PluginModelAuthMethod string

const (
	PluginModelAuthMethodGet  PluginModelAuthMethod = "GET"
	PluginModelAuthMethodPost PluginModelAuthMethod = "POST"
)

func (value PluginModelAuthMethod) MarshalJSON() ([]byte, error) { return marshalEnum(value) }
func (value *PluginModelAuthMethod) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch PluginModelAuthMethod(decoded) {
	case PluginModelAuthMethodGet, PluginModelAuthMethodPost:
		*value = PluginModelAuthMethod(decoded)
		return nil
	default:
		return unknownEnum(data, value, decoded)
	}
}
func (value PluginModelAuthMethod) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *PluginModelAuthMethod) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type PluginModelAuthBody struct {
	Kind string
	Data any
}
type PluginModelAuthBodyFormPayload struct {
	Fields map[string]string `json:"fields"`
}

func DefaultPluginModelAuthBodyFormPayload() PluginModelAuthBodyFormPayload {
	return PluginModelAuthBodyFormPayload{
		Fields: map[string]string{},
	}
}
func (value *PluginModelAuthBodyFormPayload) UnmarshalJSON(data []byte) error {
	type plain PluginModelAuthBodyFormPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"fields"}, true); err != nil {
		return err
	}
	*value = PluginModelAuthBodyFormPayload(decoded)
	return nil
}
func (value PluginModelAuthBodyFormPayload) MarshalJSON() ([]byte, error) {
	type plain PluginModelAuthBodyFormPayload
	encoded := plain(value)
	encoded.Fields = normalizeCollections(encoded.Fields)
	return json.Marshal(encoded)
}

type PluginModelAuthBodyJsonPayload struct {
	Value json.RawMessage `json:"value"`
}

func DefaultPluginModelAuthBodyJsonPayload() PluginModelAuthBodyJsonPayload {
	return PluginModelAuthBodyJsonPayload{
		Value: json.RawMessage("null"),
	}
}
func (value *PluginModelAuthBodyJsonPayload) UnmarshalJSON(data []byte) error {
	type plain PluginModelAuthBodyJsonPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"value"}, true); err != nil {
		return err
	}
	*value = PluginModelAuthBodyJsonPayload(decoded)
	return nil
}
func (value PluginModelAuthBodyJsonPayload) MarshalJSON() ([]byte, error) {
	type plain PluginModelAuthBodyJsonPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

var pluginModelAuthBodyVariants = variantFactories{"form": func() any { return new(PluginModelAuthBodyFormPayload) },
	"json": func() any { return new(PluginModelAuthBodyJsonPayload) }}

func (value PluginModelAuthBody) MarshalJSON() ([]byte, error) {
	return MarshalUnion("kind", "", value.Kind, value.Data, pluginModelAuthBodyVariants)
}
func (value *PluginModelAuthBody) UnmarshalJSON(data []byte) error {
	kind, decoded, err := UnmarshalUnion(data, "kind", "", pluginModelAuthBodyVariants, true)
	if err != nil {
		return err
	}
	value.Kind, value.Data = kind, decoded
	return nil
}
