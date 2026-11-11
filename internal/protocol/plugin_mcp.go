// Code generated from the reviewed v1 protocol declarations. DO NOT EDIT.

package protocol

import (
	"encoding/json"
)

type PluginMcpAvailability string

const (
	PluginMcpAvailabilityAvailable             PluginMcpAvailability = "available"
	PluginMcpAvailabilityPackageDisabled       PluginMcpAvailability = "package_disabled"
	PluginMcpAvailabilityServerDisabled        PluginMcpAvailability = "server_disabled"
	PluginMcpAvailabilityConfigurationRequired PluginMcpAvailability = "configuration_required"
	PluginMcpAvailabilityConnectionRequired    PluginMcpAvailability = "connection_required"
	PluginMcpAvailabilityConnectionDisabled    PluginMcpAvailability = "connection_disabled"
	PluginMcpAvailabilityAuthorizationExpired  PluginMcpAvailability = "authorization_expired"
	PluginMcpAvailabilityConsentRequired       PluginMcpAvailability = "consent_required"
	PluginMcpAvailabilityUnavailable           PluginMcpAvailability = "unavailable"
)

func (value PluginMcpAvailability) MarshalJSON() ([]byte, error) { return marshalEnum(value) }
func (value *PluginMcpAvailability) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch PluginMcpAvailability(decoded) {
	case PluginMcpAvailabilityAvailable, PluginMcpAvailabilityPackageDisabled, PluginMcpAvailabilityServerDisabled, PluginMcpAvailabilityConfigurationRequired, PluginMcpAvailabilityConnectionRequired, PluginMcpAvailabilityConnectionDisabled, PluginMcpAvailabilityAuthorizationExpired, PluginMcpAvailabilityConsentRequired, PluginMcpAvailabilityUnavailable:
		*value = PluginMcpAvailability(decoded)
		return nil
	default:
		return unknownEnum(data, value, decoded)
	}
}
func (value PluginMcpAvailability) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *PluginMcpAvailability) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type PluginMcpRuntime struct {
	State       PluginMcpRuntimeState `json:"state"`
	Tools       uint32                `json:"tools"`
	Fault       *Fault                `json:"fault,omitempty"`
	UpdatedAtMs uint64                `json:"updated_at_ms"`
}

func DefaultPluginMcpRuntime() PluginMcpRuntime {
	return PluginMcpRuntime{
		State:       PluginMcpRuntimeState(""),
		Tools:       0,
		Fault:       nil,
		UpdatedAtMs: 0,
	}
}
func (value *PluginMcpRuntime) UnmarshalJSON(data []byte) error {
	type plain PluginMcpRuntime
	decoded := plain{}
	decoded.Fault = nil
	if err := DecodeObject(data, &decoded, []string{"state", "tools", "updated_at_ms"}, false); err != nil {
		return err
	}
	*value = PluginMcpRuntime(decoded)
	return nil
}
func (value PluginMcpRuntime) MarshalJSON() ([]byte, error) {
	type plain PluginMcpRuntime
	encoded := plain(value)
	return json.Marshal(encoded)
}

type PluginMcpRuntimeState string

const (
	PluginMcpRuntimeStateConnecting PluginMcpRuntimeState = "connecting"
	PluginMcpRuntimeStateConnected  PluginMcpRuntimeState = "connected"
	PluginMcpRuntimeStateFailed     PluginMcpRuntimeState = "failed"
)

func (value PluginMcpRuntimeState) MarshalJSON() ([]byte, error) { return marshalEnum(value) }
func (value *PluginMcpRuntimeState) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch PluginMcpRuntimeState(decoded) {
	case PluginMcpRuntimeStateConnecting, PluginMcpRuntimeStateConnected, PluginMcpRuntimeStateFailed:
		*value = PluginMcpRuntimeState(decoded)
		return nil
	default:
		return unknownEnum(data, value, decoded)
	}
}
func (value PluginMcpRuntimeState) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *PluginMcpRuntimeState) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type PluginMcpCheck struct {
	Package PluginReference `json:"package"`
	Server  string          `json:"server"`
	Tools   []PluginMcpTool `json:"tools"`
}

func DefaultPluginMcpCheck() PluginMcpCheck {
	return PluginMcpCheck{
		Package: DefaultPluginReference(),
		Server:  "",
		Tools:   []PluginMcpTool{},
	}
}
func (value *PluginMcpCheck) UnmarshalJSON(data []byte) error {
	type plain PluginMcpCheck
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"package", "server", "tools"}, false); err != nil {
		return err
	}
	*value = PluginMcpCheck(decoded)
	return nil
}
func (value PluginMcpCheck) MarshalJSON() ([]byte, error) {
	type plain PluginMcpCheck
	encoded := plain(value)
	encoded.Tools = normalizeCollections(encoded.Tools)
	return json.Marshal(encoded)
}

type PluginMcpTool struct {
	Name        string  `json:"name"`
	Description *string `json:"description"`
}

func DefaultPluginMcpTool() PluginMcpTool {
	return PluginMcpTool{
		Name:        "",
		Description: nil,
	}
}
func (value *PluginMcpTool) UnmarshalJSON(data []byte) error {
	type plain PluginMcpTool
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"name"}, false); err != nil {
		return err
	}
	*value = PluginMcpTool(decoded)
	return nil
}
func (value PluginMcpTool) MarshalJSON() ([]byte, error) {
	type plain PluginMcpTool
	encoded := plain(value)
	return json.Marshal(encoded)
}

type PluginMcpConfiguration struct {
	Schema  string                     `json:"$schema"`
	Servers map[string]PluginMcpServer `json:"mcpServers"`
}

func DefaultPluginMcpConfiguration() PluginMcpConfiguration {
	return PluginMcpConfiguration{
		Schema:  "",
		Servers: map[string]PluginMcpServer{},
	}
}
func (value *PluginMcpConfiguration) UnmarshalJSON(data []byte) error {
	type plain PluginMcpConfiguration
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"$schema", "mcpServers"}, true); err != nil {
		return err
	}
	*value = PluginMcpConfiguration(decoded)
	return nil
}
func (value PluginMcpConfiguration) MarshalJSON() ([]byte, error) {
	type plain PluginMcpConfiguration
	encoded := plain(value)
	encoded.Servers = normalizeCollections(encoded.Servers)
	return json.Marshal(encoded)
}

type PluginMcpServer struct {
	Kind string
	Data any
}
type PluginMcpServerStdioPayload struct {
	Command    string            `json:"command"`
	Args       []string          `json:"args"`
	Env        map[string]string `json:"env"`
	Cwd        string            `json:"cwd"`
	Exclusive  bool              `json:"exclusive,omitempty"`
	KeepImages *uint64           `json:"keep_images,omitempty"`
	Reuse      bool              `json:"reuse"`
	Tools      *[]string         `json:"tools,omitempty"`
}

func DefaultPluginMcpServerStdioPayload() PluginMcpServerStdioPayload {
	return PluginMcpServerStdioPayload{
		Command:    "",
		Args:       []string{},
		Env:        map[string]string{},
		Cwd:        "",
		Exclusive:  false,
		KeepImages: nil,
		Reuse:      false,
		Tools:      nil,
	}
}
func (value *PluginMcpServerStdioPayload) UnmarshalJSON(data []byte) error {
	type plain PluginMcpServerStdioPayload
	decoded := plain{}
	decoded.Args = []string{}
	decoded.Env = map[string]string{}
	decoded.Cwd = "${PLUGIN_ROOT}"
	decoded.Exclusive = false
	decoded.KeepImages = nil
	decoded.Reuse = true
	decoded.Tools = nil
	if err := DecodeObject(data, &decoded, []string{"command"}, true); err != nil {
		return err
	}
	*value = PluginMcpServerStdioPayload(decoded)
	return nil
}
func (value PluginMcpServerStdioPayload) MarshalJSON() ([]byte, error) {
	type plain PluginMcpServerStdioPayload
	encoded := plain(value)
	encoded.Args = normalizeCollections(encoded.Args)
	encoded.Env = normalizeCollections(encoded.Env)
	encoded.Tools = normalizeCollections(encoded.Tools)
	data, err := json.Marshal(encoded)
	if err != nil {
		return nil, err
	}
	var fields map[string]json.RawMessage
	if err := json.Unmarshal(data, &fields); err != nil {
		return nil, err
	}
	if encoded.Reuse {
		delete(fields, "reuse")
	}
	return json.Marshal(fields)
}

type PluginMcpServerHttpPayload struct {
	Url        string            `json:"url"`
	Headers    map[string]string `json:"headers"`
	Exclusive  bool              `json:"exclusive,omitempty"`
	KeepImages *uint64           `json:"keep_images,omitempty"`
	Reuse      bool              `json:"reuse"`
	Tools      *[]string         `json:"tools,omitempty"`
}

func DefaultPluginMcpServerHttpPayload() PluginMcpServerHttpPayload {
	return PluginMcpServerHttpPayload{
		Url:        "",
		Headers:    map[string]string{},
		Exclusive:  false,
		KeepImages: nil,
		Reuse:      false,
		Tools:      nil,
	}
}
func (value *PluginMcpServerHttpPayload) UnmarshalJSON(data []byte) error {
	type plain PluginMcpServerHttpPayload
	decoded := plain{}
	decoded.Headers = map[string]string{}
	decoded.Exclusive = false
	decoded.KeepImages = nil
	decoded.Reuse = true
	decoded.Tools = nil
	if err := DecodeObject(data, &decoded, []string{"url"}, true); err != nil {
		return err
	}
	*value = PluginMcpServerHttpPayload(decoded)
	return nil
}
func (value PluginMcpServerHttpPayload) MarshalJSON() ([]byte, error) {
	type plain PluginMcpServerHttpPayload
	encoded := plain(value)
	encoded.Headers = normalizeCollections(encoded.Headers)
	encoded.Tools = normalizeCollections(encoded.Tools)
	data, err := json.Marshal(encoded)
	if err != nil {
		return nil, err
	}
	var fields map[string]json.RawMessage
	if err := json.Unmarshal(data, &fields); err != nil {
		return nil, err
	}
	if encoded.Reuse {
		delete(fields, "reuse")
	}
	return json.Marshal(fields)
}

type PluginMcpServerSsePayload struct {
	Url        string            `json:"url"`
	Headers    map[string]string `json:"headers"`
	Exclusive  bool              `json:"exclusive,omitempty"`
	KeepImages *uint64           `json:"keep_images,omitempty"`
	Reuse      bool              `json:"reuse"`
	Tools      *[]string         `json:"tools,omitempty"`
}

func DefaultPluginMcpServerSsePayload() PluginMcpServerSsePayload {
	return PluginMcpServerSsePayload{
		Url:        "",
		Headers:    map[string]string{},
		Exclusive:  false,
		KeepImages: nil,
		Reuse:      false,
		Tools:      nil,
	}
}
func (value *PluginMcpServerSsePayload) UnmarshalJSON(data []byte) error {
	type plain PluginMcpServerSsePayload
	decoded := plain{}
	decoded.Headers = map[string]string{}
	decoded.Exclusive = false
	decoded.KeepImages = nil
	decoded.Reuse = true
	decoded.Tools = nil
	if err := DecodeObject(data, &decoded, []string{"url"}, true); err != nil {
		return err
	}
	*value = PluginMcpServerSsePayload(decoded)
	return nil
}
func (value PluginMcpServerSsePayload) MarshalJSON() ([]byte, error) {
	type plain PluginMcpServerSsePayload
	encoded := plain(value)
	encoded.Headers = normalizeCollections(encoded.Headers)
	encoded.Tools = normalizeCollections(encoded.Tools)
	data, err := json.Marshal(encoded)
	if err != nil {
		return nil, err
	}
	var fields map[string]json.RawMessage
	if err := json.Unmarshal(data, &fields); err != nil {
		return nil, err
	}
	if encoded.Reuse {
		delete(fields, "reuse")
	}
	return json.Marshal(fields)
}

var pluginMcpServerVariants = variantFactories{"stdio": func() any { return new(PluginMcpServerStdioPayload) },
	"streamable-http": func() any { return new(PluginMcpServerHttpPayload) },
	"sse":             func() any { return new(PluginMcpServerSsePayload) }}

func (value PluginMcpServer) MarshalJSON() ([]byte, error) {
	return MarshalUnion("type", "", value.Kind, value.Data, pluginMcpServerVariants)
}
func (value *PluginMcpServer) UnmarshalJSON(data []byte) error {
	kind, decoded, err := UnmarshalUnion(data, "type", "", pluginMcpServerVariants, true)
	if err != nil {
		return err
	}
	value.Kind, value.Data = kind, decoded
	return nil
}

type PluginMcpOptions struct {
	Exclusive  bool    `json:"exclusive"`
	KeepImages *uint64 `json:"keep_images"`
	Reuse      bool    `json:"reuse"`
}

func DefaultPluginMcpOptions() PluginMcpOptions {
	return PluginMcpOptions{
		Exclusive:  false,
		KeepImages: nil,
		Reuse:      true,
	}
}
func (value *PluginMcpOptions) UnmarshalJSON(data []byte) error {
	type plain PluginMcpOptions
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"exclusive", "reuse"}, false); err != nil {
		return err
	}
	*value = PluginMcpOptions(decoded)
	return nil
}
func (value PluginMcpOptions) MarshalJSON() ([]byte, error) {
	type plain PluginMcpOptions
	encoded := plain(value)
	return json.Marshal(encoded)
}

type PluginMcpState struct {
	Package       PluginReference        `json:"package"`
	Configuration PluginMcpConfiguration `json:"configuration"`
}

func DefaultPluginMcpState() PluginMcpState {
	return PluginMcpState{
		Package:       DefaultPluginReference(),
		Configuration: DefaultPluginMcpConfiguration(),
	}
}
func (value *PluginMcpState) UnmarshalJSON(data []byte) error {
	type plain PluginMcpState
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"package", "configuration"}, false); err != nil {
		return err
	}
	*value = PluginMcpState(decoded)
	return nil
}
func (value PluginMcpState) MarshalJSON() ([]byte, error) {
	type plain PluginMcpState
	encoded := plain(value)
	return json.Marshal(encoded)
}

type PluginMcpDefinition struct {
	Kind string
	Data any
}
type PluginMcpDefinitionStdioPayload struct {
	Command string   `json:"command"`
	Args    []string `json:"args"`
	Env     []string `json:"env"`
}

func DefaultPluginMcpDefinitionStdioPayload() PluginMcpDefinitionStdioPayload {
	return PluginMcpDefinitionStdioPayload{
		Command: "",
		Args:    []string{},
		Env:     []string{},
	}
}
func (value *PluginMcpDefinitionStdioPayload) UnmarshalJSON(data []byte) error {
	type plain PluginMcpDefinitionStdioPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"command", "args", "env"}, true); err != nil {
		return err
	}
	*value = PluginMcpDefinitionStdioPayload(decoded)
	return nil
}
func (value PluginMcpDefinitionStdioPayload) MarshalJSON() ([]byte, error) {
	type plain PluginMcpDefinitionStdioPayload
	encoded := plain(value)
	encoded.Args = normalizeCollections(encoded.Args)
	encoded.Env = normalizeCollections(encoded.Env)
	return json.Marshal(encoded)
}

type PluginMcpDefinitionStreamableHttpPayload struct {
	Url     string   `json:"url"`
	Headers []string `json:"headers"`
}

func DefaultPluginMcpDefinitionStreamableHttpPayload() PluginMcpDefinitionStreamableHttpPayload {
	return PluginMcpDefinitionStreamableHttpPayload{
		Url:     "",
		Headers: []string{},
	}
}
func (value *PluginMcpDefinitionStreamableHttpPayload) UnmarshalJSON(data []byte) error {
	type plain PluginMcpDefinitionStreamableHttpPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"url", "headers"}, true); err != nil {
		return err
	}
	*value = PluginMcpDefinitionStreamableHttpPayload(decoded)
	return nil
}
func (value PluginMcpDefinitionStreamableHttpPayload) MarshalJSON() ([]byte, error) {
	type plain PluginMcpDefinitionStreamableHttpPayload
	encoded := plain(value)
	encoded.Headers = normalizeCollections(encoded.Headers)
	return json.Marshal(encoded)
}

type PluginMcpDefinitionSsePayload struct {
	Url     string   `json:"url"`
	Headers []string `json:"headers"`
}

func DefaultPluginMcpDefinitionSsePayload() PluginMcpDefinitionSsePayload {
	return PluginMcpDefinitionSsePayload{
		Url:     "",
		Headers: []string{},
	}
}
func (value *PluginMcpDefinitionSsePayload) UnmarshalJSON(data []byte) error {
	type plain PluginMcpDefinitionSsePayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"url", "headers"}, true); err != nil {
		return err
	}
	*value = PluginMcpDefinitionSsePayload(decoded)
	return nil
}
func (value PluginMcpDefinitionSsePayload) MarshalJSON() ([]byte, error) {
	type plain PluginMcpDefinitionSsePayload
	encoded := plain(value)
	encoded.Headers = normalizeCollections(encoded.Headers)
	return json.Marshal(encoded)
}

var pluginMcpDefinitionVariants = variantFactories{"stdio": func() any { return new(PluginMcpDefinitionStdioPayload) },
	"streamable-http": func() any { return new(PluginMcpDefinitionStreamableHttpPayload) },
	"sse":             func() any { return new(PluginMcpDefinitionSsePayload) }}

func (value PluginMcpDefinition) MarshalJSON() ([]byte, error) {
	return MarshalUnion("type", "", value.Kind, value.Data, pluginMcpDefinitionVariants)
}
func (value *PluginMcpDefinition) UnmarshalJSON(data []byte) error {
	kind, decoded, err := UnmarshalUnion(data, "type", "", pluginMcpDefinitionVariants, true)
	if err != nil {
		return err
	}
	value.Kind, value.Data = kind, decoded
	return nil
}
