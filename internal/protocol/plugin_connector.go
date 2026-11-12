// Hand-maintained v1 protocol declarations; keep them consistent with the fixtures and tolerance tests.

package protocol

import (
	"encoding/json"
	"fmt"
	"reflect"
)

type PluginConnectorID = RequestID

type PluginConnectorDeclaration struct {
	ID       string                          `json:"id"`
	Display  PluginDesktopNavigation         `json:"display"`
	Accounts PluginConnectorAccounts         `json:"accounts"`
	Origins  []string                        `json:"origins"`
	Auth     []PluginConnectorAuth           `json:"auth"`
	Fields   []PluginConnectorField          `json:"fields"`
	Inject   []PluginConnectorInjection      `json:"inject"`
	Identity *PluginConnectorIdentityRequest `json:"identity"`
	Health   *PluginConnectorHealthRequest   `json:"health"`
}

func DefaultPluginConnectorDeclaration() PluginConnectorDeclaration {
	return PluginConnectorDeclaration{
		ID:       "",
		Display:  DefaultPluginDesktopNavigation(),
		Accounts: PluginConnectorAccounts(""),
		Origins:  []string{},
		Auth:     []PluginConnectorAuth{},
		Fields:   []PluginConnectorField{},
		Inject:   []PluginConnectorInjection{},
		Identity: nil,
		Health:   nil,
	}
}
func (value *PluginConnectorDeclaration) UnmarshalJSON(data []byte) error {
	type plain PluginConnectorDeclaration
	decoded := plain{}
	decoded.Fields = []PluginConnectorField{}
	decoded.Identity = nil
	decoded.Health = nil
	if err := DecodeObject(data, &decoded, []string{"id", "display", "accounts", "origins", "auth", "inject"}, true); err != nil {
		return err
	}
	*value = PluginConnectorDeclaration(decoded)
	return nil
}
func (value PluginConnectorDeclaration) MarshalJSON() ([]byte, error) {
	type plain PluginConnectorDeclaration
	encoded := plain(value)
	encoded.Origins = normalizeCollections(encoded.Origins)
	encoded.Auth = normalizeCollections(encoded.Auth)
	encoded.Fields = normalizeCollections(encoded.Fields)
	encoded.Inject = normalizeCollections(encoded.Inject)
	return json.Marshal(encoded)
}

type PluginConnectorAccounts string

const (
	PluginConnectorAccountsSingle PluginConnectorAccounts = "single"
)

func (value PluginConnectorAccounts) MarshalJSON() ([]byte, error) { return marshalEnum(value) }
func (value *PluginConnectorAccounts) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch PluginConnectorAccounts(decoded) {
	case PluginConnectorAccountsSingle:
		*value = PluginConnectorAccounts(decoded)
		return nil
	default:
		return unknownEnum(data, value, decoded)
	}
}
func (value PluginConnectorAccounts) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *PluginConnectorAccounts) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type PluginConnectorField struct {
	Name     string            `json:"name"`
	Title    string            `json:"title"`
	Locales  map[string]string `json:"locales"`
	Secret   bool              `json:"secret"`
	Optional bool              `json:"optional"`
	Default  *string           `json:"default"`
	Help     *string           `json:"help"`
}

func DefaultPluginConnectorField() PluginConnectorField {
	return PluginConnectorField{
		Name:     "",
		Title:    "",
		Locales:  map[string]string{},
		Secret:   false,
		Optional: false,
		Default:  nil,
		Help:     nil,
	}
}
func (value *PluginConnectorField) UnmarshalJSON(data []byte) error {
	type plain PluginConnectorField
	decoded := plain{}
	decoded.Locales = map[string]string{}
	decoded.Secret = false
	decoded.Optional = false
	decoded.Default = nil
	decoded.Help = nil
	if err := DecodeObject(data, &decoded, []string{"name", "title"}, true); err != nil {
		return err
	}
	*value = PluginConnectorField(decoded)
	return nil
}
func (value PluginConnectorField) MarshalJSON() ([]byte, error) {
	type plain PluginConnectorField
	encoded := plain(value)
	encoded.Locales = normalizeCollections(encoded.Locales)
	return json.Marshal(encoded)
}

type PluginConnectorKind string

const (
	PluginConnectorKindOauth2      PluginConnectorKind = "oauth2"
	PluginConnectorKindApplication PluginConnectorKind = "application"
	PluginConnectorKindSecret      PluginConnectorKind = "secret"
	PluginConnectorKindMcpOauth    PluginConnectorKind = "mcp_oauth"
	PluginConnectorKindLocalCli    PluginConnectorKind = "local_cli"
)

func (value PluginConnectorKind) MarshalJSON() ([]byte, error) { return marshalEnum(value) }
func (value *PluginConnectorKind) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch PluginConnectorKind(decoded) {
	case PluginConnectorKindOauth2, PluginConnectorKindApplication, PluginConnectorKindSecret, PluginConnectorKindMcpOauth, PluginConnectorKindLocalCli:
		*value = PluginConnectorKind(decoded)
		return nil
	default:
		return unknownEnum(data, value, decoded)
	}
}
func (value PluginConnectorKind) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *PluginConnectorKind) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type PluginConnectorAuth struct {
	Kind string
	Data any
}
type PluginConnectorAuthOauth2Payload struct {
	Flow      PluginConnectorFlow   `json:"flow"`
	Authorize string                `json:"authorize"`
	Token     string                `json:"token"`
	Device    *string               `json:"device"`
	Client    PluginConnectorClient `json:"client"`
	Scopes    []string              `json:"scopes"`
	Refresh   bool                  `json:"refresh"`
}

func DefaultPluginConnectorAuthOauth2Payload() PluginConnectorAuthOauth2Payload {
	return PluginConnectorAuthOauth2Payload{
		Flow:      PluginConnectorFlow(""),
		Authorize: "",
		Token:     "",
		Device:    nil,
		Client:    PluginConnectorClient{},
		Scopes:    []string{},
		Refresh:   false,
	}
}
func (value *PluginConnectorAuthOauth2Payload) UnmarshalJSON(data []byte) error {
	type plain PluginConnectorAuthOauth2Payload
	decoded := plain{}
	decoded.Device = nil
	decoded.Scopes = []string{}
	decoded.Refresh = false
	if err := DecodeObject(data, &decoded, []string{"flow", "authorize", "token", "client"}, true); err != nil {
		return err
	}
	*value = PluginConnectorAuthOauth2Payload(decoded)
	return nil
}
func (value PluginConnectorAuthOauth2Payload) MarshalJSON() ([]byte, error) {
	type plain PluginConnectorAuthOauth2Payload
	encoded := plain(value)
	encoded.Scopes = normalizeCollections(encoded.Scopes)
	return json.Marshal(encoded)
}

type PluginConnectorAuthApplicationPayload struct {
	Fields       PluginConnectorClientFields   `json:"fields"`
	Token        PluginConnectorToken          `json:"token"`
	Registration **PluginConnectorRegistration `json:"registration"`
}

func DefaultPluginConnectorAuthApplicationPayload() PluginConnectorAuthApplicationPayload {
	return PluginConnectorAuthApplicationPayload{
		Fields:       DefaultPluginConnectorClientFields(),
		Token:        DefaultPluginConnectorToken(),
		Registration: nil,
	}
}
func (value *PluginConnectorAuthApplicationPayload) UnmarshalJSON(data []byte) error {
	type plain PluginConnectorAuthApplicationPayload
	decoded := plain{}
	decoded.Registration = nil
	if err := DecodeObject(data, &decoded, []string{"fields", "token"}, true); err != nil {
		return err
	}
	*value = PluginConnectorAuthApplicationPayload(decoded)
	return nil
}
func (value PluginConnectorAuthApplicationPayload) MarshalJSON() ([]byte, error) {
	type plain PluginConnectorAuthApplicationPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type PluginConnectorAuthSecretPayload struct {
	Fields []string `json:"fields"`
}

func DefaultPluginConnectorAuthSecretPayload() PluginConnectorAuthSecretPayload {
	return PluginConnectorAuthSecretPayload{
		Fields: []string{},
	}
}
func (value *PluginConnectorAuthSecretPayload) UnmarshalJSON(data []byte) error {
	type plain PluginConnectorAuthSecretPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"fields"}, true); err != nil {
		return err
	}
	*value = PluginConnectorAuthSecretPayload(decoded)
	return nil
}
func (value PluginConnectorAuthSecretPayload) MarshalJSON() ([]byte, error) {
	type plain PluginConnectorAuthSecretPayload
	encoded := plain(value)
	encoded.Fields = normalizeCollections(encoded.Fields)
	return json.Marshal(encoded)
}

type PluginConnectorAuthMcpOauthPayload struct {
	Server string                 `json:"server"`
	Client *PluginConnectorClient `json:"client"`
}

func DefaultPluginConnectorAuthMcpOauthPayload() PluginConnectorAuthMcpOauthPayload {
	return PluginConnectorAuthMcpOauthPayload{
		Server: "",
		Client: nil,
	}
}
func (value *PluginConnectorAuthMcpOauthPayload) UnmarshalJSON(data []byte) error {
	type plain PluginConnectorAuthMcpOauthPayload
	decoded := plain{}
	decoded.Client = nil
	if err := DecodeObject(data, &decoded, []string{"server"}, true); err != nil {
		return err
	}
	*value = PluginConnectorAuthMcpOauthPayload(decoded)
	return nil
}
func (value PluginConnectorAuthMcpOauthPayload) MarshalJSON() ([]byte, error) {
	type plain PluginConnectorAuthMcpOauthPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type PluginConnectorAuthLocalCliPayload struct {
	Command   string                   `json:"command"`
	Env       map[string]string        `json:"env"`
	Status    PluginConnectorCliStatus `json:"status"`
	Token     PluginConnectorCliToken  `json:"token"`
	TimeoutMs uint64                   `json:"timeout_ms"`
	MaxOutput uint64                   `json:"max_output"`
}

func DefaultPluginConnectorAuthLocalCliPayload() PluginConnectorAuthLocalCliPayload {
	return PluginConnectorAuthLocalCliPayload{
		Command:   "",
		Env:       map[string]string{},
		Status:    DefaultPluginConnectorCliStatus(),
		Token:     DefaultPluginConnectorCliToken(),
		TimeoutMs: 0,
		MaxOutput: 0,
	}
}
func (value *PluginConnectorAuthLocalCliPayload) UnmarshalJSON(data []byte) error {
	type plain PluginConnectorAuthLocalCliPayload
	decoded := plain{}
	decoded.Env = map[string]string{}
	if err := DecodeObject(data, &decoded, []string{"command", "status", "token", "timeout_ms", "max_output"}, true); err != nil {
		return err
	}
	*value = PluginConnectorAuthLocalCliPayload(decoded)
	return nil
}
func (value PluginConnectorAuthLocalCliPayload) MarshalJSON() ([]byte, error) {
	type plain PluginConnectorAuthLocalCliPayload
	encoded := plain(value)
	encoded.Env = normalizeCollections(encoded.Env)
	return json.Marshal(encoded)
}

var pluginConnectorAuthVariants = variantFactories{"oauth2": func() any { return new(PluginConnectorAuthOauth2Payload) },
	"application": func() any { return new(PluginConnectorAuthApplicationPayload) },
	"secret":      func() any { return new(PluginConnectorAuthSecretPayload) },
	"mcp_oauth":   func() any { return new(PluginConnectorAuthMcpOauthPayload) },
	"local_cli":   func() any { return new(PluginConnectorAuthLocalCliPayload) }}

func (value PluginConnectorAuth) MarshalJSON() ([]byte, error) {
	return MarshalUnion("kind", "", value.Kind, value.Data, pluginConnectorAuthVariants)
}
func (value *PluginConnectorAuth) UnmarshalJSON(data []byte) error {
	kind, decoded, err := UnmarshalUnion(data, "kind", "", pluginConnectorAuthVariants, true)
	if err != nil {
		return err
	}
	value.Kind, value.Data = kind, decoded
	return nil
}

type PluginConnectorFlow string

const (
	PluginConnectorFlowPkce       PluginConnectorFlow = "pkce"
	PluginConnectorFlowDeviceCode PluginConnectorFlow = "device_code"
)

func (value PluginConnectorFlow) MarshalJSON() ([]byte, error) { return marshalEnum(value) }
func (value *PluginConnectorFlow) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch PluginConnectorFlow(decoded) {
	case PluginConnectorFlowPkce, PluginConnectorFlowDeviceCode:
		*value = PluginConnectorFlow(decoded)
		return nil
	default:
		return unknownEnum(data, value, decoded)
	}
}
func (value PluginConnectorFlow) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *PluginConnectorFlow) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type PluginConnectorClient struct {
	Kind string
	Data any
}
type PluginConnectorClientBundledPayload struct {
	Bundled PluginConnectorPublicClient `json:"bundled"`
}

func DefaultPluginConnectorClientBundledPayload() PluginConnectorClientBundledPayload {
	return PluginConnectorClientBundledPayload{
		Bundled: DefaultPluginConnectorPublicClient(),
	}
}
func (value *PluginConnectorClientBundledPayload) UnmarshalJSON(data []byte) error {
	type plain PluginConnectorClientBundledPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"bundled"}, true); err != nil {
		return err
	}
	*value = PluginConnectorClientBundledPayload(decoded)
	return nil
}
func (value PluginConnectorClientBundledPayload) MarshalJSON() ([]byte, error) {
	type plain PluginConnectorClientBundledPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type PluginConnectorClientFieldsPayload struct {
	Fields PluginConnectorClientFields `json:"fields"`
}

func DefaultPluginConnectorClientFieldsPayload() PluginConnectorClientFieldsPayload {
	return PluginConnectorClientFieldsPayload{
		Fields: DefaultPluginConnectorClientFields(),
	}
}
func (value *PluginConnectorClientFieldsPayload) UnmarshalJSON(data []byte) error {
	type plain PluginConnectorClientFieldsPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"fields"}, true); err != nil {
		return err
	}
	*value = PluginConnectorClientFieldsPayload(decoded)
	return nil
}
func (value PluginConnectorClientFieldsPayload) MarshalJSON() ([]byte, error) {
	type plain PluginConnectorClientFieldsPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

var pluginConnectorClientVariants = variantFactories{"Bundled": func() any { return new(PluginConnectorClientBundledPayload) },
	"Fields": func() any { return new(PluginConnectorClientFieldsPayload) }}

func (value PluginConnectorClient) MarshalJSON() ([]byte, error) {
	switch value.Kind {
	case "Bundled", "Fields":
		return json.Marshal(value.Data)
	default:
		return nil, fmt.Errorf("unknown PluginConnectorClient variant %q", value.Kind)
	}
}
func (value *PluginConnectorClient) UnmarshalJSON(data []byte) error {
	{
		var decoded PluginConnectorClientBundledPayload
		if err := checkJSONNulls(data, reflect.TypeFor[PluginConnectorClientBundledPayload]()); err == nil {
			if err := json.Unmarshal(data, &decoded); err == nil {
				value.Kind = "Bundled"
				value.Data = &decoded
				return nil
			}
		}
	}
	{
		var decoded PluginConnectorClientFieldsPayload
		if err := checkJSONNulls(data, reflect.TypeFor[PluginConnectorClientFieldsPayload]()); err == nil {
			if err := json.Unmarshal(data, &decoded); err == nil {
				value.Kind = "Fields"
				value.Data = &decoded
				return nil
			}
		}
	}
	return fmt.Errorf("unrecognized PluginConnectorClient shape")
}

type PluginConnectorPublicClient struct {
	ClientID string `json:"client_id"`
}

func DefaultPluginConnectorPublicClient() PluginConnectorPublicClient {
	return PluginConnectorPublicClient{
		ClientID: "",
	}
}
func (value *PluginConnectorPublicClient) UnmarshalJSON(data []byte) error {
	type plain PluginConnectorPublicClient
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"client_id"}, true); err != nil {
		return err
	}
	*value = PluginConnectorPublicClient(decoded)
	return nil
}
func (value PluginConnectorPublicClient) MarshalJSON() ([]byte, error) {
	type plain PluginConnectorPublicClient
	encoded := plain(value)
	return json.Marshal(encoded)
}

type PluginConnectorClientFields struct {
	ClientID     string  `json:"client_id"`
	ClientSecret *string `json:"client_secret"`
}

func DefaultPluginConnectorClientFields() PluginConnectorClientFields {
	return PluginConnectorClientFields{
		ClientID:     "",
		ClientSecret: nil,
	}
}
func (value *PluginConnectorClientFields) UnmarshalJSON(data []byte) error {
	type plain PluginConnectorClientFields
	decoded := plain{}
	decoded.ClientSecret = nil
	if err := DecodeObject(data, &decoded, []string{"client_id"}, true); err != nil {
		return err
	}
	*value = PluginConnectorClientFields(decoded)
	return nil
}
func (value PluginConnectorClientFields) MarshalJSON() ([]byte, error) {
	type plain PluginConnectorClientFields
	encoded := plain(value)
	return json.Marshal(encoded)
}

type PluginConnectorToken struct {
	Url       string            `json:"url"`
	Body      map[string]string `json:"body"`
	Access    string            `json:"access"`
	ExpiresIn string            `json:"expires_in"`
	Form      bool              `json:"form"`
}

func DefaultPluginConnectorToken() PluginConnectorToken {
	return PluginConnectorToken{
		Url:       "",
		Body:      map[string]string{},
		Access:    "",
		ExpiresIn: "",
		Form:      false,
	}
}
func (value *PluginConnectorToken) UnmarshalJSON(data []byte) error {
	type plain PluginConnectorToken
	decoded := plain{}
	decoded.Form = false
	if err := DecodeObject(data, &decoded, []string{"url", "body", "access", "expires_in"}, true); err != nil {
		return err
	}
	*value = PluginConnectorToken(decoded)
	return nil
}
func (value PluginConnectorToken) MarshalJSON() ([]byte, error) {
	type plain PluginConnectorToken
	encoded := plain(value)
	encoded.Body = normalizeCollections(encoded.Body)
	return json.Marshal(encoded)
}

type PluginConnectorRegistration struct {
	Flow   PluginModelAuthFlow         `json:"flow"`
	Token  PluginModelAuthMapping      `json:"token"`
	Issues PluginConnectorClientFields `json:"issues"`
	Scopes []string                    `json:"scopes"`
}

func DefaultPluginConnectorRegistration() PluginConnectorRegistration {
	return PluginConnectorRegistration{
		Flow:   PluginModelAuthFlow{},
		Token:  DefaultPluginModelAuthMapping(),
		Issues: DefaultPluginConnectorClientFields(),
		Scopes: []string{},
	}
}
func (value *PluginConnectorRegistration) UnmarshalJSON(data []byte) error {
	type plain PluginConnectorRegistration
	decoded := plain{}
	decoded.Scopes = []string{}
	if err := DecodeObject(data, &decoded, []string{"flow", "token", "issues"}, true); err != nil {
		return err
	}
	*value = PluginConnectorRegistration(decoded)
	return nil
}
func (value PluginConnectorRegistration) MarshalJSON() ([]byte, error) {
	type plain PluginConnectorRegistration
	encoded := plain(value)
	encoded.Scopes = normalizeCollections(encoded.Scopes)
	return json.Marshal(encoded)
}

type PluginConnectorCliStatus struct {
	Argv      []string                  `json:"argv"`
	Account   string                    `json:"account"`
	Condition *PluginModelAuthCondition `json:"condition,omitempty"`
}

func DefaultPluginConnectorCliStatus() PluginConnectorCliStatus {
	return PluginConnectorCliStatus{
		Argv:      []string{},
		Account:   "",
		Condition: nil,
	}
}
func (value *PluginConnectorCliStatus) UnmarshalJSON(data []byte) error {
	type plain PluginConnectorCliStatus
	decoded := plain{}
	decoded.Condition = nil
	if err := DecodeObject(data, &decoded, []string{"argv", "account"}, true); err != nil {
		return err
	}
	*value = PluginConnectorCliStatus(decoded)
	return nil
}
func (value PluginConnectorCliStatus) MarshalJSON() ([]byte, error) {
	type plain PluginConnectorCliStatus
	encoded := plain(value)
	encoded.Argv = normalizeCollections(encoded.Argv)
	return json.Marshal(encoded)
}

type PluginConnectorCliToken struct {
	Argv  []string             `json:"argv"`
	Parse PluginConnectorParse `json:"parse"`
}

func DefaultPluginConnectorCliToken() PluginConnectorCliToken {
	return PluginConnectorCliToken{
		Argv:  []string{},
		Parse: PluginConnectorParse{},
	}
}
func (value *PluginConnectorCliToken) UnmarshalJSON(data []byte) error {
	type plain PluginConnectorCliToken
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"argv", "parse"}, true); err != nil {
		return err
	}
	*value = PluginConnectorCliToken(decoded)
	return nil
}
func (value PluginConnectorCliToken) MarshalJSON() ([]byte, error) {
	type plain PluginConnectorCliToken
	encoded := plain(value)
	encoded.Argv = normalizeCollections(encoded.Argv)
	return json.Marshal(encoded)
}

type PluginConnectorParse struct {
	Kind string
	Data any
}

var pluginConnectorParseVariants = variantFactories{"line": nil,
	"json": func() any { return new(string) }}

func (value PluginConnectorParse) MarshalJSON() ([]byte, error) {
	return MarshalUnion("", "", value.Kind, value.Data, pluginConnectorParseVariants)
}
func (value *PluginConnectorParse) UnmarshalJSON(data []byte) error {
	kind, decoded, err := UnmarshalUnion(data, "", "", pluginConnectorParseVariants, false)
	if err != nil {
		return err
	}
	value.Kind, value.Data = kind, decoded
	return nil
}

type PluginConnectorInjection struct {
	Origin *string `json:"origin"`
	Server *string `json:"server"`
	Header *string `json:"header"`
	Env    *string `json:"env"`
	Prefix string  `json:"prefix"`
	Field  *string `json:"field"`
}

func DefaultPluginConnectorInjection() PluginConnectorInjection {
	return PluginConnectorInjection{
		Origin: nil,
		Server: nil,
		Header: nil,
		Env:    nil,
		Prefix: "",
		Field:  nil,
	}
}
func (value *PluginConnectorInjection) UnmarshalJSON(data []byte) error {
	type plain PluginConnectorInjection
	decoded := plain{}
	decoded.Origin = nil
	decoded.Server = nil
	decoded.Header = nil
	decoded.Env = nil
	decoded.Prefix = ""
	decoded.Field = nil
	if err := DecodeObject(data, &decoded, []string{}, true); err != nil {
		return err
	}
	*value = PluginConnectorInjection(decoded)
	return nil
}
func (value PluginConnectorInjection) MarshalJSON() ([]byte, error) {
	type plain PluginConnectorInjection
	encoded := plain(value)
	return json.Marshal(encoded)
}

type PluginConnectorIdentityRequest struct {
	Request string `json:"request"`
	Name    string `json:"name"`
	ID      string `json:"id"`
}

func DefaultPluginConnectorIdentityRequest() PluginConnectorIdentityRequest {
	return PluginConnectorIdentityRequest{
		Request: "",
		Name:    "",
		ID:      "",
	}
}
func (value *PluginConnectorIdentityRequest) UnmarshalJSON(data []byte) error {
	type plain PluginConnectorIdentityRequest
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"request", "name", "id"}, true); err != nil {
		return err
	}
	*value = PluginConnectorIdentityRequest(decoded)
	return nil
}
func (value PluginConnectorIdentityRequest) MarshalJSON() ([]byte, error) {
	type plain PluginConnectorIdentityRequest
	encoded := plain(value)
	return json.Marshal(encoded)
}

type PluginConnectorHealthRequest struct {
	Request string   `json:"request"`
	Ok      []uint16 `json:"ok"`
}

func DefaultPluginConnectorHealthRequest() PluginConnectorHealthRequest {
	return PluginConnectorHealthRequest{
		Request: "",
		Ok:      []uint16{},
	}
}
func (value *PluginConnectorHealthRequest) UnmarshalJSON(data []byte) error {
	type plain PluginConnectorHealthRequest
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"request", "ok"}, true); err != nil {
		return err
	}
	*value = PluginConnectorHealthRequest(decoded)
	return nil
}
func (value PluginConnectorHealthRequest) MarshalJSON() ([]byte, error) {
	type plain PluginConnectorHealthRequest
	encoded := plain(value)
	encoded.Ok = normalizeCollections(encoded.Ok)
	return json.Marshal(encoded)
}

type PluginConnectorType struct {
	Package     PluginReference            `json:"package"`
	Declaration PluginConnectorDeclaration `json:"declaration"`
	Enabled     bool                       `json:"enabled"`
}

func DefaultPluginConnectorType() PluginConnectorType {
	return PluginConnectorType{
		Package:     DefaultPluginReference(),
		Declaration: DefaultPluginConnectorDeclaration(),
		Enabled:     false,
	}
}
func (value *PluginConnectorType) UnmarshalJSON(data []byte) error {
	type plain PluginConnectorType
	decoded := plain{}
	decoded.Enabled = true
	if err := DecodeObject(data, &decoded, []string{"package", "declaration"}, false); err != nil {
		return err
	}
	*value = PluginConnectorType(decoded)
	return nil
}
func (value PluginConnectorType) MarshalJSON() ([]byte, error) {
	type plain PluginConnectorType
	encoded := plain(value)
	return json.Marshal(encoded)
}

type PluginConnectorIdentity struct {
	Name string `json:"name"`
	ID   string `json:"id"`
}

func DefaultPluginConnectorIdentity() PluginConnectorIdentity {
	return PluginConnectorIdentity{
		Name: "",
		ID:   "",
	}
}
func (value *PluginConnectorIdentity) UnmarshalJSON(data []byte) error {
	type plain PluginConnectorIdentity
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"name", "id"}, false); err != nil {
		return err
	}
	*value = PluginConnectorIdentity(decoded)
	return nil
}
func (value PluginConnectorIdentity) MarshalJSON() ([]byte, error) {
	type plain PluginConnectorIdentity
	encoded := plain(value)
	return json.Marshal(encoded)
}

type PluginConnectorState string

const (
	PluginConnectorStateConnected         PluginConnectorState = "connected"
	PluginConnectorStateNotConnected      PluginConnectorState = "not_connected"
	PluginConnectorStateConnecting        PluginConnectorState = "connecting"
	PluginConnectorStateExpired           PluginConnectorState = "expired"
	PluginConnectorStateNeedsConfirmation PluginConnectorState = "needs_confirmation"
	PluginConnectorStateFailed            PluginConnectorState = "failed"
)

func (value PluginConnectorState) MarshalJSON() ([]byte, error) { return marshalEnum(value) }
func (value *PluginConnectorState) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch PluginConnectorState(decoded) {
	case PluginConnectorStateConnected, PluginConnectorStateNotConnected, PluginConnectorStateConnecting, PluginConnectorStateExpired, PluginConnectorStateNeedsConfirmation, PluginConnectorStateFailed:
		*value = PluginConnectorState(decoded)
		return nil
	default:
		return unknownEnum(data, value, decoded)
	}
}
func (value PluginConnectorState) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *PluginConnectorState) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type PluginConnectorConnection struct {
	ID            PluginConnectorID        `json:"id"`
	Package       PluginReference          `json:"package"`
	Connector     string                   `json:"connector"`
	Method        PluginConnectorKind      `json:"method"`
	Revision      uint64                   `json:"revision"`
	Enabled       bool                     `json:"enabled"`
	State         PluginConnectorState     `json:"state"`
	CanDisconnect bool                     `json:"can_disconnect"`
	Identity      *PluginConnectorIdentity `json:"identity"`
	ExpiresAtMs   *uint64                  `json:"expires_at_ms"`
	CheckedAtMs   *uint64                  `json:"checked_at_ms"`
	Fields        map[string]string        `json:"fields"`
	Configured    []string                 `json:"configured"`
	Consent       *PluginConnectorConsent  `json:"consent,omitempty"`
}

func DefaultPluginConnectorConnection() PluginConnectorConnection {
	return PluginConnectorConnection{
		ID:            RequestID{},
		Package:       DefaultPluginReference(),
		Connector:     "",
		Method:        PluginConnectorKind(""),
		Revision:      0,
		Enabled:       false,
		State:         PluginConnectorState(""),
		CanDisconnect: false,
		Identity:      nil,
		ExpiresAtMs:   nil,
		CheckedAtMs:   nil,
		Fields:        map[string]string{},
		Configured:    []string{},
		Consent:       nil,
	}
}
func (value *PluginConnectorConnection) UnmarshalJSON(data []byte) error {
	type plain PluginConnectorConnection
	decoded := plain{}
	decoded.CanDisconnect = false
	decoded.Consent = nil
	if err := DecodeObject(data, &decoded, []string{"id", "package", "connector", "method", "revision", "enabled", "state", "fields", "configured"}, false); err != nil {
		return err
	}
	*value = PluginConnectorConnection(decoded)
	return nil
}
func (value PluginConnectorConnection) MarshalJSON() ([]byte, error) {
	type plain PluginConnectorConnection
	encoded := plain(value)
	encoded.Fields = normalizeCollections(encoded.Fields)
	encoded.Configured = normalizeCollections(encoded.Configured)
	return json.Marshal(encoded)
}

type PluginConnectorConsent struct {
	Changes []PluginConnectorChange `json:"changes"`
}

func DefaultPluginConnectorConsent() PluginConnectorConsent {
	return PluginConnectorConsent{
		Changes: []PluginConnectorChange{},
	}
}
func (value *PluginConnectorConsent) UnmarshalJSON(data []byte) error {
	type plain PluginConnectorConsent
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"changes"}, false); err != nil {
		return err
	}
	*value = PluginConnectorConsent(decoded)
	return nil
}
func (value PluginConnectorConsent) MarshalJSON() ([]byte, error) {
	type plain PluginConnectorConsent
	encoded := plain(value)
	encoded.Changes = normalizeCollections(encoded.Changes)
	return json.Marshal(encoded)
}

type PluginConnectorChange struct {
	Kind string
	Data any
}
type PluginConnectorChangeOriginsPayload struct {
	Previous []string `json:"previous"`
	Current  []string `json:"current"`
}

func DefaultPluginConnectorChangeOriginsPayload() PluginConnectorChangeOriginsPayload {
	return PluginConnectorChangeOriginsPayload{
		Previous: []string{},
		Current:  []string{},
	}
}
func (value *PluginConnectorChangeOriginsPayload) UnmarshalJSON(data []byte) error {
	type plain PluginConnectorChangeOriginsPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"previous", "current"}, false); err != nil {
		return err
	}
	*value = PluginConnectorChangeOriginsPayload(decoded)
	return nil
}
func (value PluginConnectorChangeOriginsPayload) MarshalJSON() ([]byte, error) {
	type plain PluginConnectorChangeOriginsPayload
	encoded := plain(value)
	encoded.Previous = normalizeCollections(encoded.Previous)
	encoded.Current = normalizeCollections(encoded.Current)
	return json.Marshal(encoded)
}

type PluginConnectorChangeScopesPayload struct {
	Previous []string `json:"previous"`
	Current  []string `json:"current"`
}

func DefaultPluginConnectorChangeScopesPayload() PluginConnectorChangeScopesPayload {
	return PluginConnectorChangeScopesPayload{
		Previous: []string{},
		Current:  []string{},
	}
}
func (value *PluginConnectorChangeScopesPayload) UnmarshalJSON(data []byte) error {
	type plain PluginConnectorChangeScopesPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"previous", "current"}, false); err != nil {
		return err
	}
	*value = PluginConnectorChangeScopesPayload(decoded)
	return nil
}
func (value PluginConnectorChangeScopesPayload) MarshalJSON() ([]byte, error) {
	type plain PluginConnectorChangeScopesPayload
	encoded := plain(value)
	encoded.Previous = normalizeCollections(encoded.Previous)
	encoded.Current = normalizeCollections(encoded.Current)
	return json.Marshal(encoded)
}

type PluginConnectorChangeClientPayload struct {
	Previous string `json:"previous"`
	Current  string `json:"current"`
}

func DefaultPluginConnectorChangeClientPayload() PluginConnectorChangeClientPayload {
	return PluginConnectorChangeClientPayload{
		Previous: "",
		Current:  "",
	}
}
func (value *PluginConnectorChangeClientPayload) UnmarshalJSON(data []byte) error {
	type plain PluginConnectorChangeClientPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"previous", "current"}, false); err != nil {
		return err
	}
	*value = PluginConnectorChangeClientPayload(decoded)
	return nil
}
func (value PluginConnectorChangeClientPayload) MarshalJSON() ([]byte, error) {
	type plain PluginConnectorChangeClientPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type PluginConnectorChangeFlowPayload struct {
	Previous string `json:"previous"`
	Current  string `json:"current"`
}

func DefaultPluginConnectorChangeFlowPayload() PluginConnectorChangeFlowPayload {
	return PluginConnectorChangeFlowPayload{
		Previous: "",
		Current:  "",
	}
}
func (value *PluginConnectorChangeFlowPayload) UnmarshalJSON(data []byte) error {
	type plain PluginConnectorChangeFlowPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"previous", "current"}, false); err != nil {
		return err
	}
	*value = PluginConnectorChangeFlowPayload(decoded)
	return nil
}
func (value PluginConnectorChangeFlowPayload) MarshalJSON() ([]byte, error) {
	type plain PluginConnectorChangeFlowPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type PluginConnectorChangeTargetsPayload struct {
	Previous []string `json:"previous"`
	Current  []string `json:"current"`
}

func DefaultPluginConnectorChangeTargetsPayload() PluginConnectorChangeTargetsPayload {
	return PluginConnectorChangeTargetsPayload{
		Previous: []string{},
		Current:  []string{},
	}
}
func (value *PluginConnectorChangeTargetsPayload) UnmarshalJSON(data []byte) error {
	type plain PluginConnectorChangeTargetsPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"previous", "current"}, false); err != nil {
		return err
	}
	*value = PluginConnectorChangeTargetsPayload(decoded)
	return nil
}
func (value PluginConnectorChangeTargetsPayload) MarshalJSON() ([]byte, error) {
	type plain PluginConnectorChangeTargetsPayload
	encoded := plain(value)
	encoded.Previous = normalizeCollections(encoded.Previous)
	encoded.Current = normalizeCollections(encoded.Current)
	return json.Marshal(encoded)
}

type PluginConnectorChangeMcpPayload struct {
	Servers []string `json:"servers"`
}

func DefaultPluginConnectorChangeMcpPayload() PluginConnectorChangeMcpPayload {
	return PluginConnectorChangeMcpPayload{
		Servers: []string{},
	}
}
func (value *PluginConnectorChangeMcpPayload) UnmarshalJSON(data []byte) error {
	type plain PluginConnectorChangeMcpPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"servers"}, false); err != nil {
		return err
	}
	*value = PluginConnectorChangeMcpPayload(decoded)
	return nil
}
func (value PluginConnectorChangeMcpPayload) MarshalJSON() ([]byte, error) {
	type plain PluginConnectorChangeMcpPayload
	encoded := plain(value)
	encoded.Servers = normalizeCollections(encoded.Servers)
	return json.Marshal(encoded)
}

var pluginConnectorChangeVariants = variantFactories{"origins": func() any { return new(PluginConnectorChangeOriginsPayload) },
	"scopes":  func() any { return new(PluginConnectorChangeScopesPayload) },
	"client":  func() any { return new(PluginConnectorChangeClientPayload) },
	"flow":    func() any { return new(PluginConnectorChangeFlowPayload) },
	"targets": func() any { return new(PluginConnectorChangeTargetsPayload) },
	"mcp":     func() any { return new(PluginConnectorChangeMcpPayload) },
	"source":  nil}

func (value PluginConnectorChange) MarshalJSON() ([]byte, error) {
	return MarshalUnion("kind", "", value.Kind, value.Data, pluginConnectorChangeVariants)
}
func (value *PluginConnectorChange) UnmarshalJSON(data []byte) error {
	kind, decoded, err := UnmarshalUnion(data, "kind", "", pluginConnectorChangeVariants, false)
	if err != nil {
		return err
	}
	value.Kind, value.Data = kind, decoded
	return nil
}

type PluginConnectorCapture struct {
	ID       PluginConnectorID `json:"id"`
	Revision uint64            `json:"revision"`
}

func DefaultPluginConnectorCapture() PluginConnectorCapture {
	return PluginConnectorCapture{
		ID:       RequestID{},
		Revision: 0,
	}
}
func (value *PluginConnectorCapture) UnmarshalJSON(data []byte) error {
	type plain PluginConnectorCapture
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"id", "revision"}, false); err != nil {
		return err
	}
	*value = PluginConnectorCapture(decoded)
	return nil
}
func (value PluginConnectorCapture) MarshalJSON() ([]byte, error) {
	type plain PluginConnectorCapture
	encoded := plain(value)
	return json.Marshal(encoded)
}

type PluginConnectorAttempt struct {
	ID         RequestID         `json:"id"`
	Connection PluginConnectorID `json:"connection"`
	Package    PluginReference   `json:"package"`
	Revision   uint64            `json:"revision"`
}

func DefaultPluginConnectorAttempt() PluginConnectorAttempt {
	return PluginConnectorAttempt{
		ID:         RequestID{},
		Connection: RequestID{},
		Package:    DefaultPluginReference(),
		Revision:   0,
	}
}
func (value *PluginConnectorAttempt) UnmarshalJSON(data []byte) error {
	type plain PluginConnectorAttempt
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"id", "connection", "package", "revision"}, false); err != nil {
		return err
	}
	*value = PluginConnectorAttempt(decoded)
	return nil
}
func (value PluginConnectorAttempt) MarshalJSON() ([]byte, error) {
	type plain PluginConnectorAttempt
	encoded := plain(value)
	return json.Marshal(encoded)
}

type PluginConnectorLoginState struct {
	Kind string
	Data any
}
type PluginConnectorLoginStateDevicePayload struct {
	Code        string `json:"code"`
	Url         string `json:"url"`
	ExpiresAtMs uint64 `json:"expires_at_ms"`
}

func DefaultPluginConnectorLoginStateDevicePayload() PluginConnectorLoginStateDevicePayload {
	return PluginConnectorLoginStateDevicePayload{
		Code:        "",
		Url:         "",
		ExpiresAtMs: 0,
	}
}
func (value *PluginConnectorLoginStateDevicePayload) UnmarshalJSON(data []byte) error {
	type plain PluginConnectorLoginStateDevicePayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"code", "url", "expires_at_ms"}, false); err != nil {
		return err
	}
	*value = PluginConnectorLoginStateDevicePayload(decoded)
	return nil
}
func (value PluginConnectorLoginStateDevicePayload) MarshalJSON() ([]byte, error) {
	type plain PluginConnectorLoginStateDevicePayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type PluginConnectorLoginStateBrowserPayload struct {
	Url         string `json:"url"`
	ExpiresAtMs uint64 `json:"expires_at_ms"`
}

func DefaultPluginConnectorLoginStateBrowserPayload() PluginConnectorLoginStateBrowserPayload {
	return PluginConnectorLoginStateBrowserPayload{
		Url:         "",
		ExpiresAtMs: 0,
	}
}
func (value *PluginConnectorLoginStateBrowserPayload) UnmarshalJSON(data []byte) error {
	type plain PluginConnectorLoginStateBrowserPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"url", "expires_at_ms"}, false); err != nil {
		return err
	}
	*value = PluginConnectorLoginStateBrowserPayload(decoded)
	return nil
}
func (value PluginConnectorLoginStateBrowserPayload) MarshalJSON() ([]byte, error) {
	type plain PluginConnectorLoginStateBrowserPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

var pluginConnectorLoginStateVariants = variantFactories{"starting": nil,
	"device":     func() any { return new(PluginConnectorLoginStateDevicePayload) },
	"browser":    func() any { return new(PluginConnectorLoginStateBrowserPayload) },
	"exchanging": nil,
	"connected":  func() any { return new(PluginConnectorConnection) },
	"failed":     func() any { return new(Fault) },
	"cancelled":  nil}

func (value PluginConnectorLoginState) MarshalJSON() ([]byte, error) {
	return MarshalUnion("kind", "data", value.Kind, value.Data, pluginConnectorLoginStateVariants)
}
func (value *PluginConnectorLoginState) UnmarshalJSON(data []byte) error {
	kind, decoded, err := UnmarshalUnion(data, "kind", "data", pluginConnectorLoginStateVariants, false)
	if err != nil {
		return err
	}
	value.Kind, value.Data = kind, decoded
	return nil
}

type PluginConnectorLoginUpdate struct {
	Attempt  PluginConnectorAttempt    `json:"attempt"`
	Revision uint64                    `json:"revision"`
	State    PluginConnectorLoginState `json:"state"`
}

func DefaultPluginConnectorLoginUpdate() PluginConnectorLoginUpdate {
	return PluginConnectorLoginUpdate{
		Attempt:  DefaultPluginConnectorAttempt(),
		Revision: 0,
		State:    PluginConnectorLoginState{},
	}
}
func (value *PluginConnectorLoginUpdate) UnmarshalJSON(data []byte) error {
	type plain PluginConnectorLoginUpdate
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"attempt", "revision", "state"}, false); err != nil {
		return err
	}
	*value = PluginConnectorLoginUpdate(decoded)
	return nil
}
func (value PluginConnectorLoginUpdate) MarshalJSON() ([]byte, error) {
	type plain PluginConnectorLoginUpdate
	encoded := plain(value)
	return json.Marshal(encoded)
}

type PluginConnectorChanges struct {
	Cursor      uint64                      `json:"cursor"`
	Types       []PluginConnectorType       `json:"types"`
	Connections []PluginConnectorConnection `json:"connections"`
}

func DefaultPluginConnectorChanges() PluginConnectorChanges {
	return PluginConnectorChanges{
		Cursor:      0,
		Types:       []PluginConnectorType{},
		Connections: []PluginConnectorConnection{},
	}
}
func (value *PluginConnectorChanges) UnmarshalJSON(data []byte) error {
	type plain PluginConnectorChanges
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"cursor", "types", "connections"}, false); err != nil {
		return err
	}
	*value = PluginConnectorChanges(decoded)
	return nil
}
func (value PluginConnectorChanges) MarshalJSON() ([]byte, error) {
	type plain PluginConnectorChanges
	encoded := plain(value)
	encoded.Types = normalizeCollections(encoded.Types)
	encoded.Connections = normalizeCollections(encoded.Connections)
	return json.Marshal(encoded)
}

type PluginConnectorInput struct {
	Fields       map[string]string `json:"fields"`
	Secrets      map[string]Secret `json:"secrets"`
	Redirect     *string           `json:"redirect"`
	Registration bool              `json:"registration"`
}

func DefaultPluginConnectorInput() PluginConnectorInput {
	return PluginConnectorInput{
		Fields:       map[string]string{},
		Secrets:      map[string]Secret{},
		Redirect:     nil,
		Registration: false,
	}
}
func (value *PluginConnectorInput) UnmarshalJSON(data []byte) error {
	type plain PluginConnectorInput
	decoded := plain{}
	decoded.Fields = map[string]string{}
	decoded.Secrets = map[string]Secret{}
	decoded.Redirect = nil
	decoded.Registration = false
	if err := DecodeObject(data, &decoded, []string{}, false); err != nil {
		return err
	}
	*value = PluginConnectorInput(decoded)
	return nil
}
func (value PluginConnectorInput) MarshalJSON() ([]byte, error) {
	type plain PluginConnectorInput
	encoded := plain(value)
	encoded.Fields = normalizeCollections(encoded.Fields)
	encoded.Secrets = normalizeCollections(encoded.Secrets)
	return json.Marshal(encoded)
}
