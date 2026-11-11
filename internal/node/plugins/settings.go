package plugins

import (
	"bytes"
	"encoding/json"
	"net"
	"net/http"
	"net/url"
	"reflect"
	"slices"
	"strconv"
	"strings"
	"unicode/utf8"

	"github.com/sailry/sailry-harness/internal/protocol"
	"github.com/santhosh-tekuri/jsonschema/v6"
)

const SettingsSchema = "https://json-schema.org/draft/2020-12/schema"
const MaxSettingsBytes = 64 * 1024

func declarationIdentifier(value string) bool {
	if value == "" || len(value) > 64 {
		return false
	}
	for _, character := range []byte(value) {
		if !(character >= 'a' && character <= 'z' || character >= 'A' && character <= 'Z' ||
			character >= '0' && character <= '9' || character == '_' || character == '-' || character == '.') {
			return false
		}
	}
	return true
}

func compileSchema(raw any) (*jsonschema.Schema, error) {
	compiler := jsonschema.NewCompiler()
	compiler.UseLoader(jsonschema.SchemeURLLoader{})
	if err := compiler.AddResource("urn:sailry:plugin:schema", raw); err != nil {
		return nil, packageFault("invalid plugin JSON schema")
	}
	schema, err := compiler.Compile("urn:sailry:plugin:schema")
	if err != nil {
		return nil, packageFault("invalid plugin JSON schema constraints")
	}
	return schema, nil
}

func decodeValue(data []byte) (any, error) {
	decoder := json.NewDecoder(bytes.NewReader(data))
	decoder.UseNumber()
	var value any
	if err := decoder.Decode(&value); err != nil {
		return nil, packageFault("invalid plugin JSON value")
	}
	return value, nil
}

func ValidateSettings(data []byte, declarations ...map[string]protocol.PluginMcpServer) (protocol.PluginSettingsSchema, error) {
	var schema protocol.PluginSettingsSchema
	if len(data) > MaxSettingsBytes || json.Unmarshal(data, &schema) != nil {
		return schema, packageFault("unsupported settings form schema")
	}
	if schema.Schema != SettingsSchema || schema.Kind != protocol.PluginSettingsObjectObject || schema.AdditionalProperties ||
		len(schema.Properties) > 32 || len(schema.Required) > 32 || len(schema.Tabs) > 32 {
		return schema, packageFault("invalid settings form schema")
	}
	for _, text := range []*string{schema.Title, schema.Description} {
		if text != nil && (len(*text) > 4096 || strings.ContainsRune(*text, 0)) {
			return schema, packageFault("invalid settings form metadata")
		}
	}
	required := map[string]bool{}
	for _, name := range schema.Required {
		if _, exists := schema.Properties[name]; !exists || required[name] {
			return schema, packageFault("invalid required setting")
		}
		required[name] = true
	}
	tabIDs, tabFields := map[string]bool{}, map[string]bool{}
	for _, tab := range schema.Tabs {
		if !declarationIdentifier(tab.ID) || tabIDs[tab.ID] || strings.TrimSpace(tab.Title) == "" || len(tab.Title) > 4096 || len(tab.Fields) == 0 {
			return schema, packageFault("invalid settings tab")
		}
		tabIDs[tab.ID] = true
		for _, name := range tab.Fields {
			if _, exists := schema.Properties[name]; !exists || tabFields[name] {
				return schema, packageFault("invalid settings tab field")
			}
			tabFields[name] = true
		}
		if tab.Enabled != nil && (!slices.Contains(tab.Fields, *tab.Enabled) || schema.Properties[*tab.Enabled].Kind != protocol.PluginSettingsKindBoolean) {
			return schema, packageFault("invalid settings tab enable field")
		}
	}
	targets := map[string]bool{}
	childFields := 0
	for name, field := range schema.Properties {
		if !declarationIdentifier(name) {
			return schema, packageFault("invalid settings field name")
		}
		for _, text := range []*string{field.Title, field.Description, field.Pattern} {
			if text != nil && (len(*text) > 4096 || strings.ContainsRune(*text, 0)) {
				return schema, packageFault("invalid settings field metadata")
			}
		}
		if field.Secret != nil {
			if field.Kind != protocol.PluginSettingsKindString || field.Default != nil || field.Choices != nil {
				return schema, packageFault("secret fields must be strings without defaults or choices")
			}
			var servers map[string]protocol.PluginMcpServer
			if len(declarations) != 0 {
				servers = declarations[0]
			}
			target, err := ValidateBinding(*field.Secret, servers)
			if err != nil {
				return schema, err
			}
			if targets[target] {
				return schema, packageFault("secret settings must target distinct slots")
			}
			targets[target] = true
		}
		if field.Kind != protocol.PluginSettingsKindString && (field.MinLength != nil || field.MaxLength != nil || field.Pattern != nil) ||
			(field.Kind != protocol.PluginSettingsKindInteger && field.Kind != protocol.PluginSettingsKindNumber && (field.Minimum != nil || field.Maximum != nil)) ||
			field.MinLength != nil && *field.MinLength > 4096 || field.MaxLength != nil && *field.MaxLength > 4096 ||
			field.MinLength != nil && field.MaxLength != nil && *field.MinLength > *field.MaxLength {
			return schema, packageFault("invalid settings field constraints")
		}
		if field.Model && (field.Kind != protocol.PluginSettingsKindString || field.Secret != nil || field.Choices != nil) ||
			field.Time && (field.Kind != protocol.PluginSettingsKindString || field.Secret != nil || field.Choices != nil || field.Model || field.Effort != nil || field.ChildConcurrency) {
			return schema, packageFault("invalid settings field control")
		}
		if field.Effort != nil {
			model, exists := schema.Properties[*field.Effort]
			if !exists || !model.Model || field.Kind != protocol.PluginSettingsKindString || field.Model || field.Secret != nil || field.Choices != nil {
				return schema, packageFault("reasoning settings must reference a model field")
			}
		}
		if field.ChildConcurrency {
			childFields++
			var value string
			if childFields > 1 || field.Kind != protocol.PluginSettingsKindString || field.Secret != nil || field.Model || field.Effort != nil ||
				field.Choices != nil || !required[name] || field.Default == nil || json.Unmarshal(*field.Default, &value) != nil {
				return schema, packageFault("child concurrency requires a required decimal string default")
			}
			parsed, err := strconv.ParseUint(value, 10, 64)
			if err != nil || parsed == 0 {
				return schema, packageFault("child concurrency must be a positive decimal integer")
			}
		}
		raw, err := json.Marshal(field)
		if err != nil {
			return schema, err
		}
		value, err := decodeValue(raw)
		if err != nil {
			return schema, err
		}
		validator, err := compileSchema(value)
		if err != nil {
			return schema, err
		}
		values := []json.RawMessage{}
		if field.Default != nil {
			values = append(values, *field.Default)
		}
		if field.Choices != nil {
			if len(*field.Choices) == 0 || len(*field.Choices) > 64 {
				return schema, packageFault("invalid settings choices")
			}
			seen := []any{}
			for _, choice := range *field.Choices {
				value, err := decodeValue(choice)
				if err != nil || slices.ContainsFunc(seen, func(previous any) bool { return reflect.DeepEqual(previous, value) }) {
					return schema, packageFault("invalid settings choices")
				}
				seen = append(seen, value)
			}
			values = append(values, *field.Choices...)
		}
		for _, raw := range values {
			value, err := decodeValue(raw)
			if err != nil || validator.Validate(value) != nil {
				return schema, packageFault("settings default or choice violates its schema")
			}
			if text, ok := value.(string); ok && utf8.RuneCountInString(text) > 4096 {
				return schema, packageFault("settings string exceeds its size limit")
			}
		}
	}
	value, err := decodeValue(data)
	if err != nil {
		return schema, err
	}
	if _, err := compileSchema(value); err != nil {
		return schema, err
	}
	return schema, nil
}

func ValidatePublicSettings(schema protocol.PluginSettingsSchema, values map[string]json.RawMessage) error {
	public := schema
	public.Properties = map[string]protocol.PluginSettingsField{}
	public.Required = []string{}
	for name, field := range schema.Properties {
		if field.Secret == nil {
			public.Properties[name] = field
			if slices.Contains(schema.Required, name) {
				public.Required = append(public.Required, name)
			}
		}
	}
	raw, err := json.Marshal(public)
	if err != nil {
		return err
	}
	declaration, err := decodeValue(raw)
	if err != nil {
		return err
	}
	validator, err := compileSchema(declaration)
	if err != nil {
		return err
	}
	raw, err = json.Marshal(values)
	if err != nil || len(raw) > MaxSettingsBytes {
		return packageFault("settings values exceed their size limit")
	}
	if values == nil {
		raw = []byte("{}")
	}
	value, err := decodeValue(raw)
	if err != nil || validator.Validate(value) != nil {
		return packageFault("settings values violate their schema")
	}
	return nil
}

func SettingDefaults(schema *protocol.PluginSettingsSchema) map[string]json.RawMessage {
	result := map[string]json.RawMessage{}
	if schema != nil {
		for name, field := range schema.Properties {
			if field.Secret == nil && field.Default != nil {
				result[name] = bytes.Clone(*field.Default)
			}
		}
	}
	return result
}

func protocolHeader(name string) bool {
	return slices.Contains([]string{"accept", "content-type", "content-length", "host", "connection", "mcp-session-id", "mcp-protocol-version", "last-event-id", "transfer-encoding"}, strings.ToLower(name))
}

func headerName(name string) bool {
	if name == "" {
		return false
	}
	for _, value := range []byte(name) {
		if !(value >= 'a' && value <= 'z' || value >= 'A' && value <= 'Z' || value >= '0' && value <= '9' || strings.ContainsRune("!#$%&'*+-.^_`|~", rune(value))) {
			return false
		}
	}
	return true
}

func ValidateBinding(binding protocol.PluginSettingsBinding, servers map[string]protocol.PluginMcpServer) (string, error) {
	if binding.Unsupported || binding.Prefix != nil && strings.ContainsAny(*binding.Prefix, "\r\n\x00") {
		return "", packageFault("unsupported secret binding")
	}
	if binding.Origin != nil {
		origin, err := url.Parse(*binding.Origin)
		if err != nil || origin.Scheme != "https" && origin.Scheme != "http" || origin.Host == "" || origin.User != nil || origin.RawQuery != "" || origin.Fragment != "" ||
			origin.Path != "" && origin.Path != "/" || binding.Server != nil || binding.Env != nil || binding.Header == nil || !headerName(*binding.Header) || protocolHeader(*binding.Header) {
			return "", packageFault("invalid HTTP credential binding")
		}
		if origin.Scheme == "http" && origin.Hostname() != "localhost" {
			host := net.ParseIP(origin.Hostname())
			if host == nil || !host.IsLoopback() {
				return "", packageFault("HTTP credentials require HTTPS or loopback")
			}
		}
		return "http:" + strings.TrimSuffix(origin.String(), "/") + ":" + http.CanonicalHeaderKey(*binding.Header), nil
	}
	if binding.Server == nil || (binding.Env == nil) == (binding.Header == nil) {
		return "", packageFault("secret binding must target one supported MCP slot")
	}
	server, exists := servers[*binding.Server]
	if !exists {
		return "", packageFault("secret binding references an unavailable MCP server")
	}
	if binding.Env != nil {
		name := *binding.Env
		payload, ok := server.Data.(*protocol.PluginMcpServerStdioPayload)
		if !ok || name == "" || strings.ContainsAny(name, "=\x00") || binding.Prefix != nil ||
			slices.Contains([]string{"PLUGIN_ROOT", "PLUGIN_DATA", "SAILRY_PLUGIN_SETTINGS_FILE"}, strings.ToUpper(name)) {
			return "", packageFault("invalid MCP environment binding")
		}
		if value, exists := payload.Env[name]; !exists || value != "" {
			return "", packageFault("secret environment binding must target an empty declaration")
		}
		return *binding.Server + ":env:" + strings.ToUpper(name), nil
	}
	if !headerName(*binding.Header) || protocolHeader(*binding.Header) {
		return "", packageFault("invalid MCP header binding")
	}
	var headers map[string]string
	switch payload := server.Data.(type) {
	case *protocol.PluginMcpServerHttpPayload:
		headers = payload.Headers
	case *protocol.PluginMcpServerSsePayload:
		headers = payload.Headers
	default:
		return "", packageFault("secret header binding requires an HTTP MCP server")
	}
	for name, value := range headers {
		if strings.EqualFold(name, *binding.Header) && value == "" {
			return *binding.Server + ":header:" + strings.ToLower(name), nil
		}
	}
	return "", packageFault("secret header binding must target an empty declaration")
}

// OptionSchema parses a package's conversation option schema: public scalar
// values only, with distinct imports.
func OptionSchema(raw json.RawMessage) (protocol.PluginSettingsSchema, error) {
	schema, err := ValidateSettings(raw)
	if err != nil {
		return schema, err
	}
	imports := map[string]bool{}
	for _, field := range schema.Properties {
		if field.Secret != nil || field.Model || field.Time || field.Effort != nil || field.ChildConcurrency ||
			field.Import != nil && !optionImport(*field.Import) {
			return schema, packageFault("conversation options must be public scalar values")
		}
		if field.Import != nil {
			if imports[*field.Import] {
				return schema, packageFault("conversation option imports must be distinct")
			}
			imports[*field.Import] = true
		}
	}
	return schema, nil
}

// CapturedOptions are the package's conversation options as the turn froze
// them: the declared defaults overlaid with the saved values of declared keys.
func CapturedOptions(info protocol.PluginInfo, config protocol.SessionConfig) (map[string]json.RawMessage, error) {
	values := map[string]json.RawMessage{}
	if info.Extension == nil || info.Extension.ConversationOptions == nil {
		return values, nil
	}
	schema, err := OptionSchema(*info.Extension.ConversationOptions)
	if err != nil {
		return nil, err
	}
	for name, field := range schema.Properties {
		if field.Default != nil {
			values[name] = bytes.Clone(*field.Default)
		}
	}
	for name, value := range config.Options[info.Summary.Name] {
		if _, declared := schema.Properties[name]; declared {
			values[name] = bytes.Clone(value)
		}
	}
	if ValidatePublicSettings(schema, values) != nil {
		return nil, packageFault("conversation options do not match their declared schema")
	}
	return values, nil
}

func optionImport(value string) bool {
	if value == "" || len(value) > 128 {
		return false
	}
	for _, character := range []byte(value) {
		if !(character >= 'a' && character <= 'z' || character >= 'A' && character <= 'Z' ||
			character >= '0' && character <= '9' || character == '_' || character == '-' || character == '.') {
			return false
		}
	}
	return true
}
