// Code generated from the reviewed v1 protocol declarations. DO NOT EDIT.

package protocol

import (
	"encoding/json"
)

type CodeStartup struct {
	Worktree   WorktreeID `json:"worktree"`
	Root       string     `json:"root"`
	Server     string     `json:"server"`
	Executable string     `json:"executable"`
	Revision   string     `json:"revision"`
}

func DefaultCodeStartup() CodeStartup {
	return CodeStartup{
		Worktree:   WorktreeID{},
		Root:       "",
		Server:     "",
		Executable: "",
		Revision:   "",
	}
}
func (value *CodeStartup) UnmarshalJSON(data []byte) error {
	type plain CodeStartup
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"worktree", "root", "server", "executable", "revision"}, true); err != nil {
		return err
	}
	*value = CodeStartup(decoded)
	return nil
}
func (value CodeStartup) MarshalJSON() ([]byte, error) {
	type plain CodeStartup
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CodeStatus string

const (
	CodeStatusStarting     CodeStatus = "starting"
	CodeStatusIndexing     CodeStatus = "indexing"
	CodeStatusReady        CodeStatus = "ready"
	CodeStatusFailed       CodeStatus = "failed"
	CodeStatusStopped      CodeStatus = "stopped"
	CodeStatusNotInstalled CodeStatus = "not_installed"
)

func (value CodeStatus) MarshalJSON() ([]byte, error) { return marshalEnum(value) }
func (value *CodeStatus) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch CodeStatus(decoded) {
	case CodeStatusStarting, CodeStatusIndexing, CodeStatusReady, CodeStatusFailed, CodeStatusStopped, CodeStatusNotInstalled:
		*value = CodeStatus(decoded)
		return nil
	default:
		return unknownEnum(data, value, decoded)
	}
}
func (value CodeStatus) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *CodeStatus) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type CodeServer struct {
	Startup             CodeStartup `json:"startup"`
	Status              CodeStatus  `json:"status"`
	Progress            *string     `json:"progress"`
	DiagnosticsRevision uint64      `json:"diagnostics_revision"`
}

func DefaultCodeServer() CodeServer {
	return CodeServer{
		Startup:             DefaultCodeStartup(),
		Status:              CodeStatus(""),
		Progress:            nil,
		DiagnosticsRevision: 0,
	}
}
func (value *CodeServer) UnmarshalJSON(data []byte) error {
	type plain CodeServer
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"startup", "status", "diagnostics_revision"}, false); err != nil {
		return err
	}
	*value = CodeServer(decoded)
	return nil
}
func (value CodeServer) MarshalJSON() ([]byte, error) {
	type plain CodeServer
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CodeSeverity string

const (
	CodeSeverityError       CodeSeverity = "error"
	CodeSeverityWarning     CodeSeverity = "warning"
	CodeSeverityInformation CodeSeverity = "information"
	CodeSeverityHint        CodeSeverity = "hint"
)

func (value CodeSeverity) MarshalJSON() ([]byte, error) { return marshalEnum(value) }
func (value *CodeSeverity) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch CodeSeverity(decoded) {
	case CodeSeverityError, CodeSeverityWarning, CodeSeverityInformation, CodeSeverityHint:
		*value = CodeSeverity(decoded)
		return nil
	default:
		return unknownEnum(data, value, decoded)
	}
}
func (value CodeSeverity) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *CodeSeverity) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type CodePosition struct {
	Line   uint32 `json:"line"`
	Column uint32 `json:"column"`
}

func DefaultCodePosition() CodePosition {
	return CodePosition{
		Line:   0,
		Column: 0,
	}
}
func (value *CodePosition) UnmarshalJSON(data []byte) error {
	type plain CodePosition
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"line", "column"}, false); err != nil {
		return err
	}
	*value = CodePosition(decoded)
	return nil
}
func (value CodePosition) MarshalJSON() ([]byte, error) {
	type plain CodePosition
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CodeLocation struct {
	Path  string       `json:"path"`
	Start CodePosition `json:"start"`
	End   CodePosition `json:"end"`
}

func DefaultCodeLocation() CodeLocation {
	return CodeLocation{
		Path:  "",
		Start: DefaultCodePosition(),
		End:   DefaultCodePosition(),
	}
}
func (value *CodeLocation) UnmarshalJSON(data []byte) error {
	type plain CodeLocation
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"path", "start", "end"}, false); err != nil {
		return err
	}
	*value = CodeLocation(decoded)
	return nil
}
func (value CodeLocation) MarshalJSON() ([]byte, error) {
	type plain CodeLocation
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CodeDiagnostic struct {
	Location CodeLocation `json:"location"`
	Severity CodeSeverity `json:"severity"`
	Message  string       `json:"message"`
	Source   *string      `json:"source"`
	Code     *string      `json:"code"`
}

func DefaultCodeDiagnostic() CodeDiagnostic {
	return CodeDiagnostic{
		Location: DefaultCodeLocation(),
		Severity: CodeSeverity(""),
		Message:  "",
		Source:   nil,
		Code:     nil,
	}
}
func (value *CodeDiagnostic) UnmarshalJSON(data []byte) error {
	type plain CodeDiagnostic
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"location", "severity", "message"}, false); err != nil {
		return err
	}
	*value = CodeDiagnostic(decoded)
	return nil
}
func (value CodeDiagnostic) MarshalJSON() ([]byte, error) {
	type plain CodeDiagnostic
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CodeSymbol struct {
	Name     string       `json:"name"`
	Kind     uint32       `json:"kind"`
	Location CodeLocation `json:"location"`
}

func DefaultCodeSymbol() CodeSymbol {
	return CodeSymbol{
		Name:     "",
		Kind:     0,
		Location: DefaultCodeLocation(),
	}
}
func (value *CodeSymbol) UnmarshalJSON(data []byte) error {
	type plain CodeSymbol
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"name", "kind", "location"}, false); err != nil {
		return err
	}
	*value = CodeSymbol(decoded)
	return nil
}
func (value CodeSymbol) MarshalJSON() ([]byte, error) {
	type plain CodeSymbol
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CodeContent struct {
	Kind string
	Data any
}

var codeContentVariants = variantFactories{"diagnostics": func() any { return new([]CodeDiagnostic) },
	"locations":   func() any { return new([]CodeLocation) },
	"hover":       func() any { return new(string) },
	"symbols":     func() any { return new([]CodeSymbol) },
	"completions": func() any { return new([]CodeCompletion) }}

func (value CodeContent) MarshalJSON() ([]byte, error) {
	return MarshalUnion("kind", "data", value.Kind, value.Data, codeContentVariants)
}
func (value *CodeContent) UnmarshalJSON(data []byte) error {
	kind, decoded, err := UnmarshalUnion(data, "kind", "data", codeContentVariants, false)
	if err != nil {
		return err
	}
	value.Kind, value.Data = kind, decoded
	return nil
}

type CodeCompletion struct {
	Label      string        `json:"label"`
	Detail     *string       `json:"detail"`
	InsertText string        `json:"insert_text"`
	Range      *CodeLocation `json:"range"`
	Kind       *uint32       `json:"kind"`
	SortText   *string       `json:"sort_text"`
	FilterText *string       `json:"filter_text"`
}

func DefaultCodeCompletion() CodeCompletion {
	return CodeCompletion{
		Label:      "",
		Detail:     nil,
		InsertText: "",
		Range:      nil,
		Kind:       nil,
		SortText:   nil,
		FilterText: nil,
	}
}
func (value *CodeCompletion) UnmarshalJSON(data []byte) error {
	type plain CodeCompletion
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"label", "insert_text"}, false); err != nil {
		return err
	}
	*value = CodeCompletion(decoded)
	return nil
}
func (value CodeCompletion) MarshalJSON() ([]byte, error) {
	type plain CodeCompletion
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CodeResult struct {
	Status          CodeStatus   `json:"status"`
	Server          *string      `json:"server"`
	Revision        *string      `json:"revision"`
	Content         CodeContent  `json:"content"`
	Partial         bool         `json:"partial"`
	Notice          *string      `json:"notice"`
	Package         *string      `json:"package"`
	PackageInfo     *CodePackage `json:"package_info"`
	Startup         *CodeStartup `json:"startup"`
	StartupApproved bool         `json:"startup_approved"`
}

func DefaultCodeResult() CodeResult {
	return CodeResult{
		Status:          CodeStatus(""),
		Server:          nil,
		Revision:        nil,
		Content:         CodeContent{},
		Partial:         false,
		Notice:          nil,
		Package:         nil,
		PackageInfo:     nil,
		Startup:         nil,
		StartupApproved: false,
	}
}
func (value *CodeResult) UnmarshalJSON(data []byte) error {
	type plain CodeResult
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"status", "content", "partial", "startup_approved"}, false); err != nil {
		return err
	}
	*value = CodeResult(decoded)
	return nil
}
func (value CodeResult) MarshalJSON() ([]byte, error) {
	type plain CodeResult
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CodeSettings struct {
	Revision      uint64                  `json:"revision"`
	Enabled       bool                    `json:"enabled"`
	EditorEnabled bool                    `json:"editor_enabled"`
	AfterEdit     bool                    `json:"after_edit"`
	Servers       map[string]CodeOverride `json:"servers"`
	Languages     map[string][]string     `json:"languages"`
}

func DefaultCodeSettings() CodeSettings {
	return CodeSettings{
		Revision:      0,
		Enabled:       true,
		EditorEnabled: false,
		AfterEdit:     true,
		Servers:       map[string]CodeOverride{},
		Languages:     map[string][]string{},
	}
}
func (value *CodeSettings) UnmarshalJSON(data []byte) error {
	type plain CodeSettings
	decoded := plain{}
	decoded.Enabled = true
	decoded.EditorEnabled = false
	if err := DecodeObject(data, &decoded, []string{"revision", "after_edit", "servers", "languages"}, true); err != nil {
		return err
	}
	*value = CodeSettings(decoded)
	return nil
}
func (value CodeSettings) MarshalJSON() ([]byte, error) {
	type plain CodeSettings
	encoded := plain(value)
	encoded.Servers = normalizeCollections(encoded.Servers)
	encoded.Languages = normalizeCollections(encoded.Languages)
	return json.Marshal(encoded)
}

type CodeOverride struct {
	Disabled    bool               `json:"disabled"`
	Command     *string            `json:"command"`
	Args        *[]string          `json:"args"`
	Config      *json.RawMessage   `json:"config"`
	Environment *map[string]string `json:"environment"`
}

func DefaultCodeOverride() CodeOverride {
	return CodeOverride{
		Disabled:    false,
		Command:     nil,
		Args:        nil,
		Config:      nil,
		Environment: nil,
	}
}
func (value *CodeOverride) UnmarshalJSON(data []byte) error {
	type plain CodeOverride
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"disabled"}, true); err != nil {
		return err
	}
	*value = CodeOverride(decoded)
	return nil
}
func (value CodeOverride) MarshalJSON() ([]byte, error) {
	type plain CodeOverride
	encoded := plain(value)
	encoded.Args = normalizeCollections(encoded.Args)
	encoded.Environment = normalizeCollections(encoded.Environment)
	return json.Marshal(encoded)
}

type CodeInstall struct {
	Package  string    `json:"package"`
	Version  string    `json:"version"`
	Purl     string    `json:"purl"`
	Urls     []string  `json:"urls"`
	Licenses []string  `json:"licenses"`
	Command  *[]string `json:"command"`
	Revision string    `json:"revision"`
}

func DefaultCodeInstall() CodeInstall {
	return CodeInstall{
		Package:  "",
		Version:  "",
		Purl:     "",
		Urls:     []string{},
		Licenses: []string{},
		Command:  nil,
		Revision: "",
	}
}
func (value *CodeInstall) UnmarshalJSON(data []byte) error {
	type plain CodeInstall
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"package", "version", "purl", "urls", "licenses", "revision"}, true); err != nil {
		return err
	}
	*value = CodeInstall(decoded)
	return nil
}
func (value CodeInstall) MarshalJSON() ([]byte, error) {
	type plain CodeInstall
	encoded := plain(value)
	encoded.Urls = normalizeCollections(encoded.Urls)
	encoded.Licenses = normalizeCollections(encoded.Licenses)
	encoded.Command = normalizeCollections(encoded.Command)
	return json.Marshal(encoded)
}

type CodePackage struct {
	Name        string       `json:"name"`
	Version     string       `json:"version"`
	Homepage    *string      `json:"homepage"`
	Licenses    []string     `json:"licenses"`
	Install     *CodeInstall `json:"install"`
	Unavailable *string      `json:"unavailable"`
}

func DefaultCodePackage() CodePackage {
	return CodePackage{
		Name:        "",
		Version:     "",
		Homepage:    nil,
		Licenses:    []string{},
		Install:     nil,
		Unavailable: nil,
	}
}
func (value *CodePackage) UnmarshalJSON(data []byte) error {
	type plain CodePackage
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"name", "version", "licenses"}, false); err != nil {
		return err
	}
	*value = CodePackage(decoded)
	return nil
}
func (value CodePackage) MarshalJSON() ([]byte, error) {
	type plain CodePackage
	encoded := plain(value)
	encoded.Licenses = normalizeCollections(encoded.Licenses)
	return json.Marshal(encoded)
}

type CodeCandidate struct {
	Server          string       `json:"server"`
	Command         string       `json:"command"`
	Disabled        bool         `json:"disabled"`
	Status          CodeStatus   `json:"status"`
	Package         *CodePackage `json:"package"`
	Startup         *CodeStartup `json:"startup"`
	StartupApproved bool         `json:"startup_approved"`
}

func DefaultCodeCandidate() CodeCandidate {
	return CodeCandidate{
		Server:          "",
		Command:         "",
		Disabled:        false,
		Status:          CodeStatus(""),
		Package:         nil,
		Startup:         nil,
		StartupApproved: false,
	}
}
func (value *CodeCandidate) UnmarshalJSON(data []byte) error {
	type plain CodeCandidate
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"server", "command", "disabled", "status", "startup_approved"}, false); err != nil {
		return err
	}
	*value = CodeCandidate(decoded)
	return nil
}
func (value CodeCandidate) MarshalJSON() ([]byte, error) {
	type plain CodeCandidate
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CodeLanguage struct {
	Name    string          `json:"name"`
	ID      string          `json:"id"`
	Servers []CodeCandidate `json:"servers"`
}

func DefaultCodeLanguage() CodeLanguage {
	return CodeLanguage{
		Name:    "",
		ID:      "",
		Servers: []CodeCandidate{},
	}
}
func (value *CodeLanguage) UnmarshalJSON(data []byte) error {
	type plain CodeLanguage
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"name", "id", "servers"}, false); err != nil {
		return err
	}
	*value = CodeLanguage(decoded)
	return nil
}
func (value CodeLanguage) MarshalJSON() ([]byte, error) {
	type plain CodeLanguage
	encoded := plain(value)
	encoded.Servers = normalizeCollections(encoded.Servers)
	return json.Marshal(encoded)
}

type CodeWorkspace struct {
	Worktree  WorktreeID     `json:"worktree"`
	Languages []CodeLanguage `json:"languages"`
	Truncated bool           `json:"truncated"`
}

func DefaultCodeWorkspace() CodeWorkspace {
	return CodeWorkspace{
		Worktree:  WorktreeID{},
		Languages: []CodeLanguage{},
		Truncated: false,
	}
}
func (value *CodeWorkspace) UnmarshalJSON(data []byte) error {
	type plain CodeWorkspace
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"worktree", "languages", "truncated"}, false); err != nil {
		return err
	}
	*value = CodeWorkspace(decoded)
	return nil
}
func (value CodeWorkspace) MarshalJSON() ([]byte, error) {
	type plain CodeWorkspace
	encoded := plain(value)
	encoded.Languages = normalizeCollections(encoded.Languages)
	return json.Marshal(encoded)
}

type CodeDownload struct {
	Url      string `json:"url"`
	Sha256   string `json:"sha256"`
	Verified bool   `json:"verified"`
}

func DefaultCodeDownload() CodeDownload {
	return CodeDownload{
		Url:      "",
		Sha256:   "",
		Verified: false,
	}
}
func (value *CodeDownload) UnmarshalJSON(data []byte) error {
	type plain CodeDownload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"url", "sha256", "verified"}, false); err != nil {
		return err
	}
	*value = CodeDownload(decoded)
	return nil
}
func (value CodeDownload) MarshalJSON() ([]byte, error) {
	type plain CodeDownload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CodeReceipt struct {
	Package     string         `json:"package"`
	Version     string         `json:"version"`
	Purl        string         `json:"purl"`
	Revision    string         `json:"revision"`
	Downloads   []CodeDownload `json:"downloads"`
	InstalledAt string         `json:"installed_at"`
}

func DefaultCodeReceipt() CodeReceipt {
	return CodeReceipt{
		Package:     "",
		Version:     "",
		Purl:        "",
		Revision:    "",
		Downloads:   []CodeDownload{},
		InstalledAt: "",
	}
}
func (value *CodeReceipt) UnmarshalJSON(data []byte) error {
	type plain CodeReceipt
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"package", "version", "purl", "revision", "downloads", "installed_at"}, false); err != nil {
		return err
	}
	*value = CodeReceipt(decoded)
	return nil
}
func (value CodeReceipt) MarshalJSON() ([]byte, error) {
	type plain CodeReceipt
	encoded := plain(value)
	encoded.Downloads = normalizeCollections(encoded.Downloads)
	return json.Marshal(encoded)
}

type CodeInstallStatus string

const (
	CodeInstallStatusPreparing   CodeInstallStatus = "preparing"
	CodeInstallStatusDownloading CodeInstallStatus = "downloading"
	CodeInstallStatusInstalling  CodeInstallStatus = "installing"
	CodeInstallStatusReady       CodeInstallStatus = "ready"
	CodeInstallStatusFailed      CodeInstallStatus = "failed"
	CodeInstallStatusCancelled   CodeInstallStatus = "cancelled"
)

func (value CodeInstallStatus) MarshalJSON() ([]byte, error) { return marshalEnum(value) }
func (value *CodeInstallStatus) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch CodeInstallStatus(decoded) {
	case CodeInstallStatusPreparing, CodeInstallStatusDownloading, CodeInstallStatusInstalling, CodeInstallStatusReady, CodeInstallStatusFailed, CodeInstallStatusCancelled:
		*value = CodeInstallStatus(decoded)
		return nil
	default:
		return unknownEnum(data, value, decoded)
	}
}
func (value CodeInstallStatus) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *CodeInstallStatus) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type CodeInstallation struct {
	Package  string            `json:"package"`
	Status   CodeInstallStatus `json:"status"`
	Progress *string           `json:"progress"`
	Receipt  *CodeReceipt      `json:"receipt"`
	Error    *string           `json:"error"`
}

func DefaultCodeInstallation() CodeInstallation {
	return CodeInstallation{
		Package:  "",
		Status:   CodeInstallStatus(""),
		Progress: nil,
		Receipt:  nil,
		Error:    nil,
	}
}
func (value *CodeInstallation) UnmarshalJSON(data []byte) error {
	type plain CodeInstallation
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"package", "status"}, false); err != nil {
		return err
	}
	*value = CodeInstallation(decoded)
	return nil
}
func (value CodeInstallation) MarshalJSON() ([]byte, error) {
	type plain CodeInstallation
	encoded := plain(value)
	return json.Marshal(encoded)
}

type CodeRules struct {
	HelixCommit string `json:"helix_commit"`
	MasonTag    string `json:"mason_tag"`
	Retrieved   string `json:"retrieved"`
	Cached      bool   `json:"cached"`
}

func DefaultCodeRules() CodeRules {
	return CodeRules{
		HelixCommit: "",
		MasonTag:    "",
		Retrieved:   "",
		Cached:      false,
	}
}
func (value *CodeRules) UnmarshalJSON(data []byte) error {
	type plain CodeRules
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"helix_commit", "mason_tag", "retrieved", "cached"}, false); err != nil {
		return err
	}
	*value = CodeRules(decoded)
	return nil
}
func (value CodeRules) MarshalJSON() ([]byte, error) {
	type plain CodeRules
	encoded := plain(value)
	return json.Marshal(encoded)
}
