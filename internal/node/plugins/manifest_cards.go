package plugins

import (
	"encoding/json"
	"net"
	"net/url"
	"slices"
	"strings"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func ValidateCards(extension protocol.PluginExtension) error {
	if len(extension.Cards) > MaxCards {
		return packageFault("too many plugin card declarations")
	}
	for index, card := range extension.Cards {
		if !validCardDeclaration(card, extension) || slices.ContainsFunc(extension.Cards[:index], func(previous protocol.PluginCardDeclaration) bool {
			return previous.Key == card.Key
		}) {
			return packageFault("invalid plugin card declaration")
		}
	}
	return nil
}

func validCardDeclaration(card protocol.PluginCardDeclaration, extension protocol.PluginExtension) bool {
	if !cardIdentifier(card.Key) || !namespacePath(card.Entry) || !strings.HasSuffix(card.Entry, ".js") && !strings.HasSuffix(card.Entry, ".mjs") ||
		extension.Desktop == nil || !slices.Contains(extension.Desktop.Resources, card.Entry) {
		return false
	}
	properties, valid := cardSchema(card.PropsSchema)
	if !valid {
		return false
	}
	if card.Tool != nil && (!cardIdentifier(*card.Tool) || !slices.ContainsFunc(extension.Tools, func(tool protocol.ToolDeclaration) bool {
		if tool.Name != *card.Tool || tool.Server != nil || tool.Presentation != protocol.ToolPresentationContent || tool.Handler == nil || !tool.Handler.ReadOnly {
			return false
		}
		if tool.Handler.Operation != nil && !cardReadOnlyOperation(*tool.Handler.Operation) {
			return false
		}
		return tool.Handler.Flow == nil || !slices.ContainsFunc(tool.Handler.Flow.Operations, func(operation protocol.ToolOperation) bool {
			return !cardReadOnlyOperation(operation)
		})
	})) {
		return false
	}
	if card.McpServer != nil && !cardIdentifier(*card.McpServer) ||
		card.Capabilities.Settings && extension.SettingsSchema == nil && (extension.SettingsPage == nil || extension.SettingsPage.Entry == nil) {
		return false
	}
	property := func(pointer, kind string) bool {
		name, found := strings.CutPrefix(pointer, "/")
		if !found || strings.Contains(name, "/") {
			return false
		}
		declaration, ok := properties[name].(map[string]any)
		return ok && declaration["type"] == kind
	}
	if qr := card.Capabilities.Qr; qr != nil {
		if !property(qr.Payload, "string") || qr.ExpiresAt != nil && !property(*qr.ExpiresAt, "integer") {
			return false
		}
	}
	if len(card.Capabilities.Browser) > 8 {
		return false
	}
	for index, target := range card.Capabilities.Browser {
		if !cardIdentifier(target.Name) || len(target.Origins) == 0 || len(target.Origins) > 8 || len(target.Modes) == 0 || len(target.Modes) > 2 ||
			slices.ContainsFunc(card.Capabilities.Browser[:index], func(previous protocol.PluginCardTarget) bool { return previous.Name == target.Name }) {
			return false
		}
		for _, origin := range target.Origins {
			if !cardOrigin(origin) {
				return false
			}
		}
		for index, mode := range target.Modes {
			if mode != protocol.PluginCardModeEmbedded && mode != protocol.PluginCardModeExternal || slices.Contains(target.Modes[:index], mode) {
				return false
			}
		}
		switch target.Source.Kind {
		case "mcp_authorization":
			if card.McpServer == nil {
				return false
			}
		case "props":
			body, err := json.Marshal(target.Source.Data)
			var source protocol.PluginCardSourcePropsPayload
			if err != nil || json.Unmarshal(body, &source) != nil || !property(source.Pointer, "string") {
				return false
			}
		default:
			return false
		}
	}
	return true
}

func cardSchema(raw json.RawMessage) (map[string]any, bool) {
	if !json.Valid(raw) {
		return nil, false
	}
	value, err := decodeValue(raw)
	if err != nil {
		return nil, false
	}
	object, ok := value.(map[string]any)
	if !ok || object["type"] != "object" || object["additionalProperties"] != false {
		return nil, false
	}
	encoded, err := json.Marshal(value)
	if err != nil || len(encoded) > MaxCardSchemaBytes {
		return nil, false
	}
	properties, ok := object["properties"].(map[string]any)
	if !ok || len(properties) > 32 {
		return nil, false
	}
	for name := range object {
		if !slices.Contains([]string{"type", "properties", "required", "additionalProperties"}, name) {
			return nil, false
		}
	}
	if required, exists := object["required"]; exists {
		names, ok := required.([]any)
		if !ok {
			return nil, false
		}
		for _, entry := range names {
			name, ok := entry.(string)
			if _, exists := properties[name]; !ok || !exists {
				return nil, false
			}
		}
	}
	for name, raw := range properties {
		property, ok := raw.(map[string]any)
		if !publicCardName(name) || !ok {
			return nil, false
		}
		for field := range property {
			if !slices.Contains([]string{"type", "maxLength", "minLength", "enum", "description", "minimum", "maximum"}, field) {
				return nil, false
			}
		}
		switch property["type"] {
		case "string":
			maximum, ok := cardUnsigned(property["maxLength"])
			if !ok || maximum == 0 || maximum > 4096 {
				return nil, false
			}
		case "integer", "boolean":
		default:
			return nil, false
		}
	}
	if _, err := compileSchema(value); err != nil {
		return nil, false
	}
	return properties, true
}

func cardOrigin(value string) bool {
	address, err := url.Parse(value)
	if err != nil || address.Scheme != "https" || address.Hostname() == "" || address.User != nil ||
		address.RawQuery != "" || address.ForceQuery || strings.ContainsRune(value, '#') ||
		address.Path != "" && address.Path != "/" || strings.ContainsFunc(value, func(character rune) bool { return character > 127 }) {
		return false
	}
	address.Host = strings.ToLower(address.Host)
	if host := net.ParseIP(address.Hostname()); host != nil {
		port := address.Port()
		address.Host = host.String()
		if port != "" {
			address.Host = net.JoinHostPort(address.Host, port)
		} else if strings.Contains(address.Host, ":") {
			address.Host = "[" + address.Host + "]"
		}
	}
	return pluginHTTPOrigin(address) == value
}

func cardReadOnlyOperation(operation protocol.ToolOperation) bool {
	switch operation {
	case protocol.ToolOperationInstallCodeServer, protocol.ToolOperationInstallModel, protocol.ToolOperationRemoveModel,
		protocol.ToolOperationSaveRole, protocol.ToolOperationRemoveRole, protocol.ToolOperationApplyRoles,
		protocol.ToolOperationStartCodeServer, protocol.ToolOperationStopCodeServer, protocol.ToolOperationWriteArtifact,
		protocol.ToolOperationUninstallCodeServer, protocol.ToolOperationDelegateAgent, protocol.ToolOperationGuideAgent,
		protocol.ToolOperationCreateManagedWorktree, protocol.ToolOperationRegisterWorktree, protocol.ToolOperationRemoveWorktree,
		protocol.ToolOperationSaveDatabaseConnection, protocol.ToolOperationSaveSshConnection, protocol.ToolOperationExecuteDatabase,
		protocol.ToolOperationRunSsh, protocol.ToolOperationTransferSsh, protocol.ToolOperationWriteFile,
		protocol.ToolOperationEditFile, protocol.ToolOperationEditNotebook, protocol.ToolOperationDeleteFile,
		protocol.ToolOperationMoveFile, protocol.ToolOperationApplyChanges, protocol.ToolOperationCreateDirectory,
		protocol.ToolOperationGenerateImage, protocol.ToolOperationGenerateVideo, protocol.ToolOperationHttp,
		protocol.ToolOperationControlComputer, protocol.ToolOperationSetValue, protocol.ToolOperationDeleteValue,
		protocol.ToolOperationStorageTransaction, protocol.ToolOperationKnowledgeCreate, protocol.ToolOperationKnowledgeRebuild,
		protocol.ToolOperationKnowledgeRemove, protocol.ToolOperationRunCommand, protocol.ToolOperationStopCommand,
		protocol.ToolOperationWriteCommand, protocol.ToolOperationControlExternalBrowser, protocol.ToolOperationControlBrowser:
		return false
	default:
		return true
	}
}
