// Hand-maintained v1 protocol declarations; keep them consistent with the fixtures and tolerance tests.

package protocol

import (
	"encoding/json"
)

type ToolPresentation string

const (
	ToolPresentationSummary  ToolPresentation = "summary"
	ToolPresentationHidden   ToolPresentation = "hidden"
	ToolPresentationProgress ToolPresentation = "progress"
	ToolPresentationContent  ToolPresentation = "content"
	ToolPresentationDetails  ToolPresentation = "details"
)

func (value ToolPresentation) MarshalJSON() ([]byte, error)     { return marshalEnum(value) }
func (value *ToolPresentation) UnmarshalJSON(data []byte) error { return DecodeValue(data, value) }
func (value *ToolPresentation) DecodeJSON(decoder *Decoder) error {
	decoded, err := decoder.String()
	if err != nil {
		return err
	}
	switch ToolPresentation(decoded) {
	case ToolPresentationSummary, ToolPresentationHidden, ToolPresentationProgress, ToolPresentationContent, ToolPresentationDetails:
		*value = ToolPresentation(decoded)
		return nil
	default:
		return settleEnum(decoder.tolerance, value, decoded)
	}
}
func (value ToolPresentation) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *ToolPresentation) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type ToolGrouping string

const (
	ToolGroupingSequence   ToolGrouping = "sequence"
	ToolGroupingStandalone ToolGrouping = "standalone"
)

func (value ToolGrouping) MarshalJSON() ([]byte, error)     { return marshalEnum(value) }
func (value *ToolGrouping) UnmarshalJSON(data []byte) error { return DecodeValue(data, value) }
func (value *ToolGrouping) DecodeJSON(decoder *Decoder) error {
	decoded, err := decoder.String()
	if err != nil {
		return err
	}
	switch ToolGrouping(decoded) {
	case ToolGroupingSequence, ToolGroupingStandalone:
		*value = ToolGrouping(decoded)
		return nil
	default:
		return settleEnum(decoder.tolerance, value, decoded)
	}
}
func (value ToolGrouping) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *ToolGrouping) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type ToolDeclaration struct {
	Name                 string                       `json:"name"`
	Server               *string                      `json:"server"`
	Presentation         ToolPresentation             `json:"presentation"`
	Grouping             ToolGrouping                 `json:"grouping"`
	Operation            *ToolOperation               `json:"operation"`
	Handler              *ToolHandler                 `json:"handler"`
	Description          *string                      `json:"description"`
	Display              *ToolDisplay                 `json:"display"`
	Platforms            []string                     `json:"platforms,omitempty"`
	Contexts             []PluginConversationResource `json:"contexts,omitempty"`
	PlatformDescriptions map[string]string            `json:"platform_descriptions,omitempty"`
	Initial              bool                         `json:"initial,omitempty"`
	KeepImages           *uint64                      `json:"keep_images,omitempty"`
}

func DefaultToolDeclaration() ToolDeclaration {
	return ToolDeclaration{
		Name:                 "",
		Server:               nil,
		Presentation:         ToolPresentationSummary,
		Grouping:             ToolGroupingSequence,
		Operation:            nil,
		Handler:              nil,
		Description:          nil,
		Display:              nil,
		Platforms:            []string{},
		Contexts:             []PluginConversationResource{},
		PlatformDescriptions: map[string]string{},
		Initial:              false,
		KeepImages:           nil,
	}
}
func (value *ToolDeclaration) UnmarshalJSON(data []byte) error {
	type plain ToolDeclaration
	decoded := plain{}
	decoded.Presentation = ToolPresentationSummary
	decoded.Grouping = ToolGroupingSequence
	decoded.Platforms = []string{}
	decoded.Contexts = []PluginConversationResource{}
	decoded.PlatformDescriptions = map[string]string{}
	decoded.Initial = false
	decoded.KeepImages = nil
	if err := DecodeObject(data, &decoded, []string{"name"}, true); err != nil {
		return err
	}
	*value = ToolDeclaration(decoded)
	return nil
}
func (value ToolDeclaration) MarshalJSON() ([]byte, error) {
	type plain ToolDeclaration
	encoded := plain(value)
	encoded.Platforms = normalizeCollections(encoded.Platforms)
	encoded.Contexts = normalizeCollections(encoded.Contexts)
	encoded.PlatformDescriptions = normalizeCollections(encoded.PlatformDescriptions)
	return json.Marshal(encoded)
}

type ToolHandler struct {
	Name       string          `json:"name"`
	Parameters json.RawMessage `json:"parameters"`
	ReadOnly   bool            `json:"read_only"`
	Operation  *ToolOperation  `json:"operation"`
	Flow       *ToolFlow       `json:"flow"`
	Result     *string         `json:"result"`
	// Approval makes a plain handler's call ask for the ordinary tool
	// approval, which it otherwise never does.
	Approval bool `json:"approval,omitempty"`
}

func DefaultToolHandler() ToolHandler {
	return ToolHandler{
		Name:       "",
		Parameters: json.RawMessage("null"),
		ReadOnly:   false,
		Operation:  nil,
		Flow:       nil,
		Result:     nil,
		Approval:   false,
	}
}
func (value *ToolHandler) UnmarshalJSON(data []byte) error {
	type plain ToolHandler
	decoded := plain{}
	decoded.ReadOnly = false
	decoded.Approval = false
	if err := DecodeObject(data, &decoded, []string{"name", "parameters"}, true); err != nil {
		return err
	}
	*value = ToolHandler(decoded)
	return nil
}
func (value ToolHandler) MarshalJSON() ([]byte, error) {
	type plain ToolHandler
	encoded := plain(value)
	return json.Marshal(encoded)
}

type ToolFlow struct {
	Operations   []ToolOperation `json:"operations"`
	MessageChars *uint64         `json:"message_chars"`
}

func DefaultToolFlow() ToolFlow {
	return ToolFlow{
		Operations:   []ToolOperation{},
		MessageChars: nil,
	}
}
func (value *ToolFlow) UnmarshalJSON(data []byte) error {
	type plain ToolFlow
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"operations"}, true); err != nil {
		return err
	}
	*value = ToolFlow(decoded)
	return nil
}
func (value ToolFlow) MarshalJSON() ([]byte, error) {
	type plain ToolFlow
	encoded := plain(value)
	encoded.Operations = normalizeCollections(encoded.Operations)
	return json.Marshal(encoded)
}

type ToolOperation string

const (
	ToolOperationInstallModel            ToolOperation = "models.install"
	ToolOperationRemoveModel             ToolOperation = "models.remove"
	ToolOperationListRoles               ToolOperation = "roles.list"
	ToolOperationSaveRole                ToolOperation = "roles.save"
	ToolOperationRemoveRole              ToolOperation = "roles.remove"
	ToolOperationApplyRoles              ToolOperation = "roles.apply"
	ToolOperationReadArtifact            ToolOperation = "artifacts.read"
	ToolOperationListArtifacts           ToolOperation = "artifacts.list"
	ToolOperationListArtifactRevisions   ToolOperation = "artifacts.revisions"
	ToolOperationWriteArtifact           ToolOperation = "artifacts.write"
	ToolOperationSearchCode              ToolOperation = "code.search"
	ToolOperationOutlineCode             ToolOperation = "code.outline"
	ToolOperationGraphCode               ToolOperation = "code.graph"
	ToolOperationCodePackages            ToolOperation = "code.packages"
	ToolOperationCodeCatalog             ToolOperation = "code.catalog"
	ToolOperationCodeServers             ToolOperation = "code.servers"
	ToolOperationStartCodeServer         ToolOperation = "code.start"
	ToolOperationStopCodeServer          ToolOperation = "code.stop"
	ToolOperationInstallCodeServer       ToolOperation = "code.install"
	ToolOperationUninstallCodeServer     ToolOperation = "code.uninstall"
	ToolOperationCodeDiagnostics         ToolOperation = "code.diagnostics"
	ToolOperationCodeDefinition          ToolOperation = "code.definition"
	ToolOperationCodeReferences          ToolOperation = "code.references"
	ToolOperationCodeHover               ToolOperation = "code.hover"
	ToolOperationCodeSymbols             ToolOperation = "code.symbols"
	ToolOperationInspectGit              ToolOperation = "git.status"
	ToolOperationReadGitDiff             ToolOperation = "git.diff"
	ToolOperationReadGitLog              ToolOperation = "git.log"
	ToolOperationReadGitShow             ToolOperation = "git.show"
	ToolOperationReadGitBlame            ToolOperation = "git.blame"
	ToolOperationListGitChanges          ToolOperation = "git.changes"
	ToolOperationReadGitChange           ToolOperation = "git.change"
	ToolOperationListWorktrees           ToolOperation = "worktrees.list"
	ToolOperationCreateManagedWorktree   ToolOperation = "worktrees.create"
	ToolOperationRegisterWorktree        ToolOperation = "worktrees.register"
	ToolOperationRemoveWorktree          ToolOperation = "worktrees.remove"
	ToolOperationListDatabaseConnections ToolOperation = "databases.connections"
	ToolOperationSaveDatabaseConnection  ToolOperation = "databases.save"
	ToolOperationListSshConnections      ToolOperation = "ssh.connections"
	ToolOperationSaveSshConnection       ToolOperation = "ssh.save"
	ToolOperationBrowseDatabase          ToolOperation = "databases.catalog"
	ToolOperationQueryDatabase           ToolOperation = "databases.query"
	ToolOperationExecuteDatabase         ToolOperation = "databases.execute"
	ToolOperationRunSsh                  ToolOperation = "ssh.run"
	ToolOperationTransferSsh             ToolOperation = "ssh.transfer"
	ToolOperationDelegateAgent           ToolOperation = "agents.delegate"
	ToolOperationGuideAgent              ToolOperation = "agents.guide"
	ToolOperationWaitAgent               ToolOperation = "agents.wait"
	ToolOperationListDirectory           ToolOperation = "files.list"
	ToolOperationReadFile                ToolOperation = "files.read"
	ToolOperationReadFileLines           ToolOperation = "files.read_lines"
	ToolOperationWriteFile               ToolOperation = "files.write"
	ToolOperationEditFile                ToolOperation = "files.edit"
	ToolOperationDeleteFile              ToolOperation = "files.delete"
	ToolOperationMoveFile                ToolOperation = "files.move"
	ToolOperationApplyChanges            ToolOperation = "files.apply"
	ToolOperationCreateDirectory         ToolOperation = "files.mkdir"
	ToolOperationFindFiles               ToolOperation = "files.find"
	ToolOperationSearchFiles             ToolOperation = "files.search"
	ToolOperationReadOffice              ToolOperation = "office.read"
	ToolOperationReadNotebook            ToolOperation = "notebook.read"
	ToolOperationEditNotebook            ToolOperation = "notebook.edit"
	ToolOperationInspectMedia            ToolOperation = "media.inspect"
	ToolOperationGenerateImage           ToolOperation = "media.image"
	ToolOperationGenerateVideo           ToolOperation = "media.video"
	ToolOperationReadComputer            ToolOperation = "computer.read"
	ToolOperationControlComputer         ToolOperation = "computer.control"
	ToolOperationReadBrowser             ToolOperation = "browser.read"
	ToolOperationControlBrowser          ToolOperation = "browser.control"
	ToolOperationReadExternalBrowser     ToolOperation = "external_browser.read"
	ToolOperationControlExternalBrowser  ToolOperation = "external_browser.control"
	ToolOperationHttp                    ToolOperation = "http.request"
	ToolOperationFetchWeb                ToolOperation = "web.fetch"
	ToolOperationSearchWeb               ToolOperation = "web.search"
	ToolOperationProgress                ToolOperation = "progress.update"
	ToolOperationGetValue                ToolOperation = "storage.get"
	ToolOperationListKeys                ToolOperation = "storage.list"
	ToolOperationSetValue                ToolOperation = "storage.set"
	ToolOperationDeleteValue             ToolOperation = "storage.delete"
	ToolOperationStorageTransaction      ToolOperation = "storage.transaction"
	ToolOperationKnowledgeQuery          ToolOperation = "knowledge.query"
	ToolOperationKnowledgeRead           ToolOperation = "knowledge.read"
	ToolOperationKnowledgeCollections    ToolOperation = "knowledge.collections"
	ToolOperationKnowledgeCreate         ToolOperation = "knowledge.create"
	ToolOperationKnowledgeRebuild        ToolOperation = "knowledge.rebuild"
	ToolOperationKnowledgeRemove         ToolOperation = "knowledge.remove"
	ToolOperationReadSettings            ToolOperation = "settings.read"
	ToolOperationRunCommand              ToolOperation = "process.run"
	ToolOperationReadCommand             ToolOperation = "process.read"
	ToolOperationStopCommand             ToolOperation = "process.stop"
	ToolOperationWriteCommand            ToolOperation = "process.write"
)

func (value ToolOperation) MarshalJSON() ([]byte, error) { return marshalEnum(value) }
func (value *ToolOperation) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch ToolOperation(decoded) {
	case ToolOperationInstallModel, ToolOperationRemoveModel, ToolOperationListRoles, ToolOperationSaveRole, ToolOperationRemoveRole, ToolOperationApplyRoles, ToolOperationReadArtifact, ToolOperationListArtifacts, ToolOperationListArtifactRevisions, ToolOperationWriteArtifact, ToolOperationSearchCode, ToolOperationOutlineCode, ToolOperationGraphCode, ToolOperationCodePackages, ToolOperationCodeCatalog, ToolOperationCodeServers, ToolOperationStartCodeServer, ToolOperationStopCodeServer, ToolOperationInstallCodeServer, ToolOperationUninstallCodeServer, ToolOperationCodeDiagnostics, ToolOperationCodeDefinition, ToolOperationCodeReferences, ToolOperationCodeHover, ToolOperationCodeSymbols, ToolOperationInspectGit, ToolOperationReadGitDiff, ToolOperationReadGitLog, ToolOperationReadGitShow, ToolOperationReadGitBlame, ToolOperationListGitChanges, ToolOperationReadGitChange, ToolOperationListWorktrees, ToolOperationCreateManagedWorktree, ToolOperationRegisterWorktree, ToolOperationRemoveWorktree, ToolOperationListDatabaseConnections, ToolOperationSaveDatabaseConnection, ToolOperationListSshConnections, ToolOperationSaveSshConnection, ToolOperationBrowseDatabase, ToolOperationQueryDatabase, ToolOperationExecuteDatabase, ToolOperationRunSsh, ToolOperationTransferSsh, ToolOperationDelegateAgent, ToolOperationGuideAgent, ToolOperationWaitAgent, ToolOperationListDirectory, ToolOperationReadFile, ToolOperationReadFileLines, ToolOperationWriteFile, ToolOperationEditFile, ToolOperationDeleteFile, ToolOperationMoveFile, ToolOperationApplyChanges, ToolOperationCreateDirectory, ToolOperationFindFiles, ToolOperationSearchFiles, ToolOperationReadOffice, ToolOperationReadNotebook, ToolOperationEditNotebook, ToolOperationInspectMedia, ToolOperationGenerateImage, ToolOperationGenerateVideo, ToolOperationReadComputer, ToolOperationControlComputer, ToolOperationReadBrowser, ToolOperationControlBrowser, ToolOperationReadExternalBrowser, ToolOperationControlExternalBrowser, ToolOperationHttp, ToolOperationFetchWeb, ToolOperationSearchWeb, ToolOperationProgress, ToolOperationGetValue, ToolOperationListKeys, ToolOperationSetValue, ToolOperationDeleteValue, ToolOperationStorageTransaction, ToolOperationKnowledgeQuery, ToolOperationKnowledgeRead, ToolOperationKnowledgeCollections, ToolOperationKnowledgeCreate, ToolOperationKnowledgeRebuild, ToolOperationKnowledgeRemove, ToolOperationReadSettings, ToolOperationRunCommand, ToolOperationReadCommand, ToolOperationStopCommand, ToolOperationWriteCommand:
		*value = ToolOperation(decoded)
		return nil
	default:
		return unknownEnum(data, value, decoded)
	}
}
func (value ToolOperation) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *ToolOperation) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}
