package plugins

import (
	"bytes"
	"encoding/json"
	"slices"
	"strings"
	"unicode"

	"github.com/sailry/sailry-harness/internal/node/program"
	"github.com/sailry/sailry-harness/internal/protocol"
)

// operationAction is the action a package must declare before a tool or flow
// of its own may run the operation. Progress and settings reads need none.
func operationAction(operation protocol.ToolOperation) (protocol.PluginAction, bool) {
	switch operation {
	case protocol.ToolOperationInstallModel, protocol.ToolOperationRemoveModel:
		return protocol.PluginActionInstallModels, true
	case protocol.ToolOperationListRoles:
		return protocol.PluginActionReadRoles, true
	case protocol.ToolOperationSaveRole, protocol.ToolOperationRemoveRole, protocol.ToolOperationApplyRoles:
		return protocol.PluginActionWriteRoles, true
	case protocol.ToolOperationReadArtifact, protocol.ToolOperationListArtifacts, protocol.ToolOperationListArtifactRevisions:
		return protocol.PluginActionReadArtifacts, true
	case protocol.ToolOperationWriteArtifact:
		return protocol.PluginActionWriteArtifacts, true
	case protocol.ToolOperationSearchCode, protocol.ToolOperationOutlineCode, protocol.ToolOperationGraphCode:
		return protocol.PluginActionReadCodeIndex, true
	case protocol.ToolOperationInstallCodeServer, protocol.ToolOperationUninstallCodeServer, protocol.ToolOperationStartCodeServer, protocol.ToolOperationStopCodeServer:
		return protocol.PluginActionManageCode, true
	case protocol.ToolOperationCodePackages, protocol.ToolOperationCodeCatalog, protocol.ToolOperationCodeServers, protocol.ToolOperationCodeDiagnostics,
		protocol.ToolOperationCodeDefinition, protocol.ToolOperationCodeReferences, protocol.ToolOperationCodeHover, protocol.ToolOperationCodeSymbols:
		return protocol.PluginActionReadCode, true
	case protocol.ToolOperationInspectGit, protocol.ToolOperationReadGitDiff, protocol.ToolOperationReadGitLog, protocol.ToolOperationReadGitShow,
		protocol.ToolOperationListGitChanges, protocol.ToolOperationReadGitChange, protocol.ToolOperationReadGitBlame:
		return protocol.PluginActionReadGit, true
	case protocol.ToolOperationListWorktrees:
		return protocol.PluginActionReadWorktrees, true
	case protocol.ToolOperationCreateManagedWorktree, protocol.ToolOperationRegisterWorktree, protocol.ToolOperationRemoveWorktree:
		return protocol.PluginActionWriteWorktrees, true
	case protocol.ToolOperationListDatabaseConnections, protocol.ToolOperationBrowseDatabase, protocol.ToolOperationQueryDatabase:
		return protocol.PluginActionReadDatabases, true
	case protocol.ToolOperationSaveDatabaseConnection:
		return protocol.PluginActionManageDatabases, true
	case protocol.ToolOperationListSshConnections:
		return protocol.PluginActionReadSsh, true
	case protocol.ToolOperationSaveSshConnection:
		return protocol.PluginActionManageSsh, true
	case protocol.ToolOperationExecuteDatabase:
		return protocol.PluginActionControlDatabases, true
	case protocol.ToolOperationRunSsh, protocol.ToolOperationTransferSsh:
		return protocol.PluginActionControlSsh, true
	case protocol.ToolOperationDelegateAgent, protocol.ToolOperationGuideAgent, protocol.ToolOperationWaitAgent:
		return protocol.PluginActionDelegateAgents, true
	case protocol.ToolOperationListDirectory, protocol.ToolOperationReadFile, protocol.ToolOperationReadFileLines, protocol.ToolOperationFindFiles,
		protocol.ToolOperationSearchFiles, protocol.ToolOperationReadOffice, protocol.ToolOperationReadNotebook:
		return protocol.PluginActionReadFiles, true
	case protocol.ToolOperationWriteFile, protocol.ToolOperationEditFile, protocol.ToolOperationEditNotebook, protocol.ToolOperationDeleteFile,
		protocol.ToolOperationMoveFile, protocol.ToolOperationApplyChanges, protocol.ToolOperationCreateDirectory:
		return protocol.PluginActionWriteFiles, true
	case protocol.ToolOperationInspectMedia:
		return protocol.PluginActionInspectMedia, true
	case protocol.ToolOperationGenerateImage:
		return protocol.PluginActionGenerateImage, true
	case protocol.ToolOperationGenerateVideo:
		return protocol.PluginActionGenerateVideo, true
	case protocol.ToolOperationReadComputer:
		return protocol.PluginActionReadComputer, true
	case protocol.ToolOperationControlComputer:
		return protocol.PluginActionControlComputer, true
	case protocol.ToolOperationHttp:
		return protocol.PluginActionHttp, true
	case protocol.ToolOperationFetchWeb, protocol.ToolOperationSearchWeb:
		return protocol.PluginActionReadWeb, true
	case protocol.ToolOperationReadBrowser:
		return protocol.PluginActionReadBrowser, true
	case protocol.ToolOperationControlBrowser:
		return protocol.PluginActionControlBrowser, true
	case protocol.ToolOperationReadExternalBrowser:
		return protocol.PluginActionReadExternalBrowser, true
	case protocol.ToolOperationControlExternalBrowser:
		return protocol.PluginActionControlExternalBrowser, true
	case protocol.ToolOperationGetValue, protocol.ToolOperationListKeys:
		return protocol.PluginActionReadStorage, true
	case protocol.ToolOperationSetValue, protocol.ToolOperationDeleteValue, protocol.ToolOperationStorageTransaction:
		return protocol.PluginActionWriteStorage, true
	case protocol.ToolOperationKnowledgeQuery, protocol.ToolOperationKnowledgeRead, protocol.ToolOperationKnowledgeCollections:
		return protocol.PluginActionReadKnowledge, true
	case protocol.ToolOperationKnowledgeCreate, protocol.ToolOperationKnowledgeRebuild, protocol.ToolOperationKnowledgeRemove:
		return protocol.PluginActionWriteKnowledge, true
	case protocol.ToolOperationReadCommand:
		return protocol.PluginActionReadCommands, true
	case protocol.ToolOperationRunCommand, protocol.ToolOperationStopCommand, protocol.ToolOperationWriteCommand:
		return protocol.PluginActionControlCommands, true
	}
	return "", false
}

// retiredOperations and retiredActions belong to the language-server service
// that no longer exists. A package may not declare them.
var (
	retiredOperations = []protocol.ToolOperation{
		protocol.ToolOperationCodePackages, protocol.ToolOperationCodeCatalog, protocol.ToolOperationCodeServers,
		protocol.ToolOperationStartCodeServer, protocol.ToolOperationStopCodeServer, protocol.ToolOperationInstallCodeServer,
		protocol.ToolOperationUninstallCodeServer, protocol.ToolOperationCodeDiagnostics, protocol.ToolOperationCodeDefinition,
		protocol.ToolOperationCodeReferences, protocol.ToolOperationCodeHover, protocol.ToolOperationCodeSymbols,
	}
	retiredActions = []protocol.PluginAction{
		protocol.PluginActionReadCode, protocol.PluginActionManageCode, protocol.PluginActionReadCodeSettings, protocol.PluginActionSaveCodeSettings,
	}
	// A flow step has no approval of its own, so it runs only the operations
	// of these actions: reads, the package's own values and a declared
	// request. Every other change runs as a handler's bound operation, under
	// that tool's approval.
	flowActions = []protocol.PluginAction{
		protocol.PluginActionReadRoles, protocol.PluginActionReadArtifacts, protocol.PluginActionReadCodeIndex, protocol.PluginActionReadGit,
		protocol.PluginActionReadWorktrees, protocol.PluginActionReadDatabases, protocol.PluginActionReadSsh, protocol.PluginActionReadFiles,
		protocol.PluginActionReadWeb, protocol.PluginActionReadKnowledge, protocol.PluginActionReadCommands,
		protocol.PluginActionReadStorage, protocol.PluginActionWriteStorage, protocol.PluginActionHttp,
	}
	// A handler may prepare a delegation or a guidance (docs/delegation.md);
	// waiting, progress and settings reads are the model's own.
	handlerExcluded = []protocol.ToolOperation{
		protocol.ToolOperationProgress, protocol.ToolOperationReadSettings, protocol.ToolOperationWaitAgent,
	}
	toolPlatforms = []string{"macos", "windows", "linux"}
)

// FlowOperation reports whether a flow may declare an operation, which is
// whether a flow step may run it. Installation and the turn share this rule.
func FlowOperation(operation protocol.ToolOperation) bool {
	action, required := operationAction(operation)
	return required && slices.Contains(flowActions, action)
}

// validHandler is a callback name a script can export.
func validHandler(name string) bool {
	if name == "" || len(name) > 64 {
		return false
	}
	for index, value := range []byte(name) {
		letter := value >= 'a' && value <= 'z' || value >= 'A' && value <= 'Z' || value == '_'
		if !letter && (index == 0 || value < '0' || value > '9') {
			return false
		}
	}
	return true
}

func validCommandName(name string) bool {
	if name == "" || len(name) > 64 {
		return false
	}
	for _, value := range []byte(name) {
		if (value < 'a' || value > 'z') && (value < '0' || value > '9') && value != '_' && value != '-' {
			return false
		}
	}
	return true
}

func unique[T comparable](values []T) bool {
	for index, value := range values {
		if slices.Contains(values[:index], value) {
			return false
		}
	}
	return true
}

// validateHostCallbacks checks the callbacks a host script declares beyond
// its resources: every route names a declared handler.
func validateHostCallbacks(host protocol.PluginHostManifest) error {
	if len(host.Handlers) == 0 || len(host.Handlers) > 64 || len(host.Resources) > 64 || !unique(host.ReadOnly) {
		return packageFault("invalid plugin host handlers")
	}
	for _, handler := range host.Handlers {
		if !validHandler(handler) {
			return packageFault("invalid plugin host handler")
		}
	}
	for _, resource := range host.Resources {
		if !strings.HasSuffix(resource, ".js") && !strings.HasSuffix(resource, ".mjs") {
			return packageFault("plugin host resources must be scripts")
		}
	}
	if len(host.Commands) > 32 {
		return packageFault("plugin host commands exceed their declaration limit")
	}
	for index, command := range host.Commands {
		if !validCommandName(command.Name) || !slices.Contains(host.Handlers, command.Handler) ||
			slices.ContainsFunc(host.Commands[:index], func(other protocol.PluginHostCommand) bool { return other.Name == command.Name }) {
			return packageFault("invalid plugin host command")
		}
	}
	if turn := host.Turn; turn != nil {
		if !slices.Contains(host.Handlers, turn.Initialize) ||
			turn.BeforeModel != nil && !slices.Contains(host.Handlers, *turn.BeforeModel) ||
			turn.AfterTurn != nil && !slices.Contains(host.Handlers, *turn.AfterTurn) {
			return packageFault("plugin turn handler is not declared")
		}
	}
	return nil
}

// validateTool checks one declared tool: its identity, where it is offered,
// and that a handler or flow runs only operations the package declared the
// action for.
func validateTool(tool protocol.ToolDeclaration, extension protocol.PluginExtension) error {
	text := func(value string, limit int) bool {
		return strings.TrimSpace(value) != "" && len(value) <= limit
	}
	if tool.Name == "" || len(tool.Name) > 512 || strings.ContainsFunc(tool.Name, unicode.IsControl) ||
		tool.Server != nil && (*tool.Server == "" || len(*tool.Server) > 256 || strings.ContainsFunc(*tool.Server, unicode.IsControl)) {
		return packageFault("invalid plugin tool identity")
	}
	if tool.Description != nil && !text(*tool.Description, 4096) ||
		len(tool.Contexts) > 4 || !unique(tool.Contexts) || len(tool.Platforms) > 3 || len(tool.PlatformDescriptions) > 3 {
		return packageFault("invalid plugin tool declaration")
	}
	for _, platform := range tool.Platforms {
		if !slices.Contains(toolPlatforms, platform) {
			return packageFault("invalid plugin tool platform")
		}
	}
	for platform, description := range tool.PlatformDescriptions {
		if !slices.Contains(toolPlatforms, platform) || !text(description, 4096) {
			return packageFault("invalid plugin tool platform")
		}
	}
	handler := tool.Handler
	var operations []protocol.ToolOperation
	if tool.Operation != nil {
		operations = append(operations, *tool.Operation)
	}
	if handler != nil {
		if handler.Operation != nil {
			operations = append(operations, *handler.Operation)
		}
		if handler.Flow != nil {
			operations = append(operations, handler.Flow.Operations...)
		}
	}
	if tool.Operation != nil || handler != nil {
		named := len(tool.Name) <= 40 && !strings.ContainsFunc(tool.Name, func(value rune) bool {
			return value != '_' && (value < '0' || value > '9') && (value < 'a' || value > 'z') && (value < 'A' || value > 'Z')
		})
		if tool.Server != nil || !named ||
			tool.Operation != nil && *tool.Operation == protocol.ToolOperationProgress && tool.Presentation != protocol.ToolPresentationProgress {
			return packageFault("invalid plugin tool identity")
		}
	}
	if handler != nil {
		if extension.Host == nil || !slices.Contains(extension.Host.Handlers, handler.Name) {
			return packageFault("plugin tool handler is not declared")
		}
		if handler.Result != nil && (handler.Operation == nil || !slices.Contains(extension.Host.Handlers, *handler.Result)) {
			return packageFault("plugin result handler is not declared")
		}
		var parameters struct {
			Type string `json:"type"`
		}
		var compact bytes.Buffer
		if tool.Operation != nil || tool.Description == nil || json.Compact(&compact, handler.Parameters) != nil || compact.Len() > 16*1024 ||
			!bytes.HasPrefix(compact.Bytes(), []byte("{")) || json.Unmarshal(handler.Parameters, &parameters) != nil || parameters.Type != "object" {
			return packageFault("invalid plugin tool handler")
		}
		if handler.Operation != nil && slices.Contains(handlerExcluded, *handler.Operation) {
			return packageFault("plugin tool handler cannot run its operation")
		}
		if flow := handler.Flow; flow != nil {
			if handler.Operation != nil || handler.Result != nil || len(flow.Operations) == 0 || len(flow.Operations) > 8 || !unique(flow.Operations) ||
				flow.MessageChars != nil && (*flow.MessageChars == 0 || *flow.MessageChars > 2*1024*1024/4) {
				return packageFault("invalid plugin tool flow")
			}
			for _, operation := range flow.Operations {
				if !FlowOperation(operation) {
					return packageFault("plugin flows cannot run " + string(operation))
				}
			}
		}
	}
	contained := tool.Server == nil && (tool.Operation != nil || handler != nil && (handler.ReadOnly || handler.Operation != nil || handler.Flow != nil))
	for _, operation := range operations {
		if slices.Contains(retiredOperations, operation) {
			return packageFault("plugin tool operation is not available")
		}
		action, required := operationAction(operation)
		if required && !slices.Contains(extension.Actions, action) {
			return packageFault("plugin tool operation requires an undeclared action")
		}
		contained = contained && (action == protocol.PluginActionReadStorage || action == protocol.PluginActionWriteStorage)
	}
	// An initial tool runs before any approval, so its effects must stay
	// inside the package's own storage.
	if tool.Initial && !contained {
		return packageFault("an initial plugin tool must stay inside package storage")
	}
	return nil
}

// validatePermissions checks each system permission a package contributes:
// its callbacks are declared, its check cannot write, and the actions its
// callbacks may use are the package's own.
func validatePermissions(extension protocol.PluginExtension) error {
	if len(extension.Permissions) > 32 {
		return packageFault("too many plugin permission contributions")
	}
	for _, entry := range extension.Permissions {
		if strings.TrimSpace(entry.Subject) == "" || len(entry.Subject) > 256 ||
			entry.RequestGroup != nil && (*entry.RequestGroup == "" || len(*entry.RequestGroup) > 128) {
			return packageFault("invalid plugin permission contribution")
		}
		for _, action := range entry.Actions {
			if !slices.Contains(extension.Actions, action) {
				return packageFault("plugin permission contribution uses an undeclared action")
			}
		}
		if entry.Check == nil {
			// Only the application's own entries have no callback.
			if entry.Subject != "sailry" || entry.Request != nil || len(entry.Actions) != 0 {
				return packageFault("plugin permission contribution has no check callback")
			}
			continue
		}
		if extension.Host == nil || !slices.Contains(extension.Host.ReadOnly, *entry.Check) ||
			entry.Request != nil && !slices.Contains(extension.Host.Handlers, *entry.Request) {
			return packageFault("plugin permission callback is not declared")
		}
	}
	return nil
}

// validateModels checks the cross references between a package's model
// declarations and the actions each of them needs.
func validateModels(extension protocol.PluginExtension) error {
	has := func(action protocol.PluginAction) bool { return slices.Contains(extension.Actions, action) }
	for index, auth := range extension.ModelAuth {
		for _, other := range extension.ModelAuth[:index] {
			if other.ID == auth.ID || auth.Authentication != nil && other.Authentication != nil && *other.Authentication == *auth.Authentication {
				return packageFault("duplicate plugin model sign-in")
			}
		}
		if !has(protocol.PluginActionAuthenticateModels) ||
			(auth.Exchange != nil || auth.Flow.Kind == "script") && !has(protocol.PluginActionInspectCredentials) {
			return packageFault("plugin model sign-in requires an undeclared action")
		}
		if warning := auth.Warning; warning != nil {
			text := func(value string) bool {
				return strings.TrimSpace(value) != "" && len(value) <= 1024 && !strings.ContainsFunc(value, unicode.IsControl)
			}
			if !text(warning.Text) || len(warning.Locales) > 32 {
				return packageFault("invalid plugin model sign-in warning")
			}
			for locale, value := range warning.Locales {
				if strings.TrimSpace(locale) == "" || len(locale) > 32 || !text(value) {
					return packageFault("invalid plugin model sign-in warning")
				}
			}
		}
	}
	for index, resource := range extension.ModelResources {
		if slices.ContainsFunc(extension.ModelResources[:index], func(other protocol.PluginModelAuthResource) bool {
			return other.Authentication == resource.Authentication
		}) {
			return packageFault("duplicate plugin model resource")
		}
		if !has(protocol.PluginActionHttp) {
			return packageFault("plugin model resource requires an undeclared action")
		}
	}
	for index, adapter := range extension.ModelAdapters {
		if slices.ContainsFunc(extension.ModelAdapters[:index], func(other protocol.PluginModelAdapterDeclaration) bool { return other.ID == adapter.ID }) {
			return packageFault("duplicate plugin model adapter")
		}
		if !has(protocol.PluginActionProvideModels) {
			return packageFault("plugin model adapter requires an undeclared action")
		}
		if err := validateAdapterTransport(adapter, extension); err != nil {
			return err
		}
	}
	if err := validatePrograms(extension); err != nil {
		return err
	}
	for index, preset := range extension.ModelProviders {
		if slices.ContainsFunc(extension.ModelProviders[:index], func(other protocol.ConversationPresetsPreset) bool { return other.ID == preset.ID }) {
			return packageFault("duplicate plugin model provider")
		}
		flow := ""
		switch auth := preset.Auth.Data.(type) {
		case protocol.ConversationPresetsAuthPluginPayload:
			flow = auth.Flow
		case *protocol.ConversationPresetsAuthPluginPayload:
			flow = auth.Flow
		}
		if preset.Auth.Kind == "plugin" && !slices.ContainsFunc(extension.ModelAuth, func(auth protocol.PluginModelAuthDeclaration) bool { return auth.ID == flow }) {
			return packageFault("plugin model provider names an undeclared sign-in")
		}
		if preset.Adapter != nil && !slices.ContainsFunc(extension.ModelAdapters, func(adapter protocol.PluginModelAdapterDeclaration) bool { return adapter.ID == *preset.Adapter }) {
			return packageFault("plugin model provider names an undeclared adapter")
		}
	}
	return nil
}

// validateAdapterTransport requires exactly one transport: a translation
// script over HTTP, a declared agent program, or the user's Claude Code.
func validateAdapterTransport(adapter protocol.PluginModelAdapterDeclaration, extension protocol.PluginExtension) error {
	if adapter.ClaudeCode != nil {
		return validateClaudeCode(adapter, extension)
	}
	agent := adapter.Acp
	if agent == nil {
		if adapter.Script.Entry == "" {
			return packageFault("plugin model adapter has no script")
		}
		return nil
	}
	index := slices.IndexFunc(extension.Programs, func(program protocol.PluginProgramDeclaration) bool { return program.ID == agent.Program })
	text := func(value string, limit int) bool {
		return value != "" && len(value) <= limit && !strings.ContainsFunc(value, unicode.IsControl)
	}
	if adapter.Script.Entry != "" || len(adapter.Script.Resources) != 0 || len(adapter.Origins) != 0 || adapter.Capabilities.Files ||
		adapter.Framing != protocol.PluginModelAdapterFramingNdjson || index < 0 || !text(agent.SignIn, 256) ||
		agent.ReplayBytes < 4096 || agent.ReplayBytes > 8*1024*1024 || len(agent.Efforts) > 6 || len(agent.FailureMarkers) > 4 ||
		agent.AuthMethod != nil && !text(*agent.AuthMethod, 64) {
		return packageFault("invalid plugin agent adapter")
	}
	if path := agent.SystemMeta; path != nil {
		for _, name := range strings.Split(*path, ".") {
			if name == "" || len(*path) > 128 || strings.Trim(name, "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789_") != "" {
				return packageFault("invalid plugin agent adapter")
			}
		}
	}
	for level, suffix := range agent.Efforts {
		if !slices.Contains([]string{"minimal", "low", "medium", "high", "xhigh", "max"}, level) || !text(suffix, 32) {
			return packageFault("invalid plugin agent adapter")
		}
	}
	for _, marker := range agent.FailureMarkers {
		if strings.TrimSpace(marker) == "" || len(marker) > 256 {
			return packageFault("invalid plugin agent adapter")
		}
	}
	// The model argument is how such an adapter selects its model.
	named := slices.ContainsFunc(extension.Programs[index].Args, func(argument string) bool { return strings.Contains(argument, "{model}") })
	if agent.ModelArgument != named {
		return packageFault("invalid plugin agent adapter")
	}
	return nil
}

// validateClaudeCode checks an adapter served by Claude Code. The Node owns
// that command line, so the program declares only its command and variables.
func validateClaudeCode(adapter protocol.PluginModelAdapterDeclaration, extension protocol.PluginExtension) error {
	route := adapter.ClaudeCode
	index := slices.IndexFunc(extension.Programs, func(program protocol.PluginProgramDeclaration) bool { return program.ID == route.Program })
	text := func(value string, limit int) bool {
		return value != "" && len(value) <= limit && !strings.ContainsFunc(value, unicode.IsControl)
	}
	if adapter.Acp != nil || adapter.Script.Entry != "" || len(adapter.Script.Resources) != 0 || len(adapter.Origins) != 0 || adapter.Capabilities.Files ||
		adapter.Framing != protocol.PluginModelAdapterFramingNdjson || index < 0 || !text(route.SignIn, 256) ||
		len(route.Models) == 0 || len(route.Models) > 32 ||
		len(extension.Programs[index].Args) != 0 || len(extension.Programs[index].Files) != 0 {
		return packageFault("invalid plugin Claude Code adapter")
	}
	levels := []string{"low", "medium", "high", "xhigh", "max"}
	for at, model := range route.Models {
		// A model is a `--model` value: an alias or a full name, never a flag.
		if !text(model.ID, 128) || strings.HasPrefix(model.ID, "-") || strings.ContainsAny(model.ID, " \t") ||
			slices.ContainsFunc(route.Models[:at], func(other protocol.PluginModelAdapterClaudeCodeModel) bool { return other.ID == model.ID }) ||
			model.Name != nil && !text(*model.Name, 128) || model.Context != nil && *model.Context == 0 || model.Output != nil && *model.Output == 0 ||
			len(model.Efforts) > len(levels) {
			return packageFault("invalid plugin Claude Code adapter")
		}
		for at, level := range model.Efforts {
			if !slices.Contains(levels, level) || slices.Contains(model.Efforts[:at], level) {
				return packageFault("invalid plugin Claude Code adapter")
			}
		}
	}
	return nil
}

// validatePrograms bounds what a package may have the Node start: a command
// name the Node resolves on its own search path, fixed arguments, a few
// variables the Node does not own, and files inside the program's directory.
// The placeholders are {dir} (that directory), {model} and {key}.
func validatePrograms(extension protocol.PluginExtension) error {
	if len(extension.Programs) > 4 || len(extension.Programs) > 0 && !slices.Contains(extension.Actions, protocol.PluginActionRunPrograms) {
		return packageFault("plugin programs require an undeclared action")
	}
	filled := func(value string, placeholders ...string) bool {
		for _, placeholder := range placeholders {
			value = strings.ReplaceAll(value, placeholder, "")
		}
		return len(value) <= 2048 && !strings.ContainsAny(value, "\x00{}")
	}
	for index, declared := range extension.Programs {
		command := declared.Command
		if !ValidateName(declared.ID) || slices.ContainsFunc(extension.Programs[:index], func(other protocol.PluginProgramDeclaration) bool { return other.ID == declared.ID }) ||
			command == "" || len(command) > 64 || strings.Trim(command, "abcdefghijklmnopqrstuvwxyz0123456789._-") != "" || strings.ContainsRune("._-", rune(command[0])) ||
			len(declared.Args) > 32 || len(declared.Env) > 8 || len(declared.Files) > 8 {
			return packageFault("invalid plugin program")
		}
		for _, argument := range declared.Args {
			if !filled(argument, "{dir}", "{model}") {
				return packageFault("invalid plugin program")
			}
		}
		for name, value := range declared.Env {
			if name == "" || len(name) > 64 || name[0] < 'A' || name[0] > 'Z' || strings.Trim(name, "ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789_") != "" ||
				program.Inherited(name) || !filled(value, "{dir}", "{key}") {
				return packageFault("invalid plugin program")
			}
		}
		paths := map[string]bool{}
		for _, file := range declared.Files {
			if !resourcePath(file.Path) || paths[file.Path] || len(file.Content) > 64*1024 || strings.ContainsRune(file.Content, 0) {
				return packageFault("invalid plugin program")
			}
			paths[file.Path] = true
		}
	}
	return nil
}
