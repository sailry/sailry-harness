// Code generated from the reviewed v1 protocol declarations. DO NOT EDIT.

package protocol

import (
	"encoding/json"
)

type SshAuthentication string

const (
	SshAuthenticationPassword   SshAuthentication = "password"
	SshAuthenticationPrivateKey SshAuthentication = "private_key"
	SshAuthenticationKeyPath    SshAuthentication = "key_path"
	SshAuthenticationAgent      SshAuthentication = "agent"
)

func (value SshAuthentication) MarshalJSON() ([]byte, error) { return marshalEnum(value) }
func (value *SshAuthentication) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch SshAuthentication(decoded) {
	case SshAuthenticationPassword, SshAuthenticationPrivateKey, SshAuthenticationKeyPath, SshAuthenticationAgent:
		*value = SshAuthentication(decoded)
		return nil
	default:
		return unknownEnum(data, value, decoded)
	}
}
func (value SshAuthentication) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *SshAuthentication) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type SshCredential struct {
	Kind string
	Data any
}
type SshCredentialPasswordPayload struct {
	Password Secret `json:"password"`
}

func DefaultSshCredentialPasswordPayload() SshCredentialPasswordPayload {
	return SshCredentialPasswordPayload{
		Password: Secret{},
	}
}
func (value *SshCredentialPasswordPayload) UnmarshalJSON(data []byte) error {
	type plain SshCredentialPasswordPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"password"}, false); err != nil {
		return err
	}
	*value = SshCredentialPasswordPayload(decoded)
	return nil
}
func (value SshCredentialPasswordPayload) MarshalJSON() ([]byte, error) {
	type plain SshCredentialPasswordPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type SshCredentialPrivateKeyPayload struct {
	Key        Secret  `json:"key"`
	Passphrase *Secret `json:"passphrase"`
}

func DefaultSshCredentialPrivateKeyPayload() SshCredentialPrivateKeyPayload {
	return SshCredentialPrivateKeyPayload{
		Key:        Secret{},
		Passphrase: nil,
	}
}
func (value *SshCredentialPrivateKeyPayload) UnmarshalJSON(data []byte) error {
	type plain SshCredentialPrivateKeyPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"key"}, false); err != nil {
		return err
	}
	*value = SshCredentialPrivateKeyPayload(decoded)
	return nil
}
func (value SshCredentialPrivateKeyPayload) MarshalJSON() ([]byte, error) {
	type plain SshCredentialPrivateKeyPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type SshCredentialKeyPathPayload struct {
	Path       Secret  `json:"path"`
	Passphrase *Secret `json:"passphrase"`
}

func DefaultSshCredentialKeyPathPayload() SshCredentialKeyPathPayload {
	return SshCredentialKeyPathPayload{
		Path:       Secret{},
		Passphrase: nil,
	}
}
func (value *SshCredentialKeyPathPayload) UnmarshalJSON(data []byte) error {
	type plain SshCredentialKeyPathPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"path"}, false); err != nil {
		return err
	}
	*value = SshCredentialKeyPathPayload(decoded)
	return nil
}
func (value SshCredentialKeyPathPayload) MarshalJSON() ([]byte, error) {
	type plain SshCredentialKeyPathPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

var sshCredentialVariants = variantFactories{"password": func() any { return new(SshCredentialPasswordPayload) },
	"private_key": func() any { return new(SshCredentialPrivateKeyPayload) },
	"key_path":    func() any { return new(SshCredentialKeyPathPayload) },
	"agent":       nil}

func (value SshCredential) MarshalJSON() ([]byte, error) {
	return MarshalUnion("kind", "", value.Kind, value.Data, sshCredentialVariants)
}
func (value *SshCredential) UnmarshalJSON(data []byte) error {
	kind, decoded, err := UnmarshalUnion(data, "kind", "", sshCredentialVariants, false)
	if err != nil {
		return err
	}
	value.Kind, value.Data = kind, decoded
	return nil
}

type SshHostKey struct {
	Algorithm   string `json:"algorithm"`
	Fingerprint string `json:"fingerprint"`
}

func DefaultSshHostKey() SshHostKey {
	return SshHostKey{
		Algorithm:   "",
		Fingerprint: "",
	}
}
func (value *SshHostKey) UnmarshalJSON(data []byte) error {
	type plain SshHostKey
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"algorithm", "fingerprint"}, false); err != nil {
		return err
	}
	*value = SshHostKey(decoded)
	return nil
}
func (value SshHostKey) MarshalJSON() ([]byte, error) {
	type plain SshHostKey
	encoded := plain(value)
	return json.Marshal(encoded)
}

type SshTrust struct {
	Profile          SshID      `json:"profile"`
	ExpectedRevision uint64     `json:"expected_revision"`
	Host             string     `json:"host"`
	Port             uint16     `json:"port"`
	Key              SshHostKey `json:"key"`
	Changed          bool       `json:"changed"`
}

func DefaultSshTrust() SshTrust {
	return SshTrust{
		Profile:          SshID{},
		ExpectedRevision: 0,
		Host:             "",
		Port:             0,
		Key:              DefaultSshHostKey(),
		Changed:          false,
	}
}
func (value *SshTrust) UnmarshalJSON(data []byte) error {
	type plain SshTrust
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"profile", "expected_revision", "host", "port", "key", "changed"}, false); err != nil {
		return err
	}
	*value = SshTrust(decoded)
	return nil
}
func (value SshTrust) MarshalJSON() ([]byte, error) {
	type plain SshTrust
	encoded := plain(value)
	return json.Marshal(encoded)
}

type SshTerminalLaunch struct {
	Viewport   TerminalViewport   `json:"viewport"`
	Appearance TerminalAppearance `json:"appearance"`
}

func DefaultSshTerminalLaunch() SshTerminalLaunch {
	return SshTerminalLaunch{
		Viewport:   DefaultTerminalViewport(),
		Appearance: DefaultTerminalAppearance(),
	}
}
func (value *SshTerminalLaunch) UnmarshalJSON(data []byte) error {
	type plain SshTerminalLaunch
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"viewport", "appearance"}, false); err != nil {
		return err
	}
	*value = SshTerminalLaunch(decoded)
	return nil
}
func (value SshTerminalLaunch) MarshalJSON() ([]byte, error) {
	type plain SshTerminalLaunch
	encoded := plain(value)
	return json.Marshal(encoded)
}

type SshProfile struct {
	Sharing        *ConnectionSharing `json:"sharing,omitempty"`
	ID             SshID              `json:"id"`
	Revision       uint64             `json:"revision"`
	Name           string             `json:"name"`
	Host           string             `json:"host"`
	Port           uint16             `json:"port"`
	Username       string             `json:"username"`
	Authentication SshAuthentication  `json:"authentication"`
	HostKey        *SshHostKey        `json:"host_key"`
}

func DefaultSshProfile() SshProfile {
	return SshProfile{
		Sharing:        nil,
		ID:             SshID{},
		Revision:       0,
		Name:           "",
		Host:           "",
		Port:           0,
		Username:       "",
		Authentication: SshAuthentication(""),
		HostKey:        nil,
	}
}
func (value *SshProfile) UnmarshalJSON(data []byte) error { return Decode(data, value) }
func (value *SshProfile) DecodeJSON(decoder *Decoder) error {
	type plain SshProfile
	decoded := plain{}
	if err := decoder.Object(&decoded, []string{"id", "revision", "name", "host", "port", "username", "authentication"}, false); err != nil {
		return err
	}
	*value = SshProfile(decoded)
	return nil
}
func (value SshProfile) MarshalJSON() ([]byte, error) {
	type plain SshProfile
	encoded := plain(value)
	return json.Marshal(encoded)
}

type SshOutcome struct {
	Kind string
	Data any
}
type SshOutcomeHostKeyRequiredPayload struct {
	Key     SshHostKey `json:"key"`
	Changed bool       `json:"changed"`
}

func DefaultSshOutcomeHostKeyRequiredPayload() SshOutcomeHostKeyRequiredPayload {
	return SshOutcomeHostKeyRequiredPayload{
		Key:     DefaultSshHostKey(),
		Changed: false,
	}
}
func (value *SshOutcomeHostKeyRequiredPayload) UnmarshalJSON(data []byte) error {
	type plain SshOutcomeHostKeyRequiredPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"key", "changed"}, false); err != nil {
		return err
	}
	*value = SshOutcomeHostKeyRequiredPayload(decoded)
	return nil
}
func (value SshOutcomeHostKeyRequiredPayload) MarshalJSON() ([]byte, error) {
	type plain SshOutcomeHostKeyRequiredPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type SshOutcomeHostInstalledPayload struct {
	Node NodeID `json:"node"`
}

func DefaultSshOutcomeHostInstalledPayload() SshOutcomeHostInstalledPayload {
	return SshOutcomeHostInstalledPayload{
		Node: NodeID{},
	}
}
func (value *SshOutcomeHostInstalledPayload) UnmarshalJSON(data []byte) error {
	type plain SshOutcomeHostInstalledPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"node"}, false); err != nil {
		return err
	}
	*value = SshOutcomeHostInstalledPayload(decoded)
	return nil
}
func (value SshOutcomeHostInstalledPayload) MarshalJSON() ([]byte, error) {
	type plain SshOutcomeHostInstalledPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type SshOutcomeHostNetworkUnconfirmedPayload struct {
	Node      NodeID           `json:"node"`
	Selection NetworkSelection `json:"selection"`
}

func DefaultSshOutcomeHostNetworkUnconfirmedPayload() SshOutcomeHostNetworkUnconfirmedPayload {
	return SshOutcomeHostNetworkUnconfirmedPayload{
		Node:      NodeID{},
		Selection: NetworkSelection{},
	}
}
func (value *SshOutcomeHostNetworkUnconfirmedPayload) UnmarshalJSON(data []byte) error {
	type plain SshOutcomeHostNetworkUnconfirmedPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"node", "selection"}, false); err != nil {
		return err
	}
	*value = SshOutcomeHostNetworkUnconfirmedPayload(decoded)
	return nil
}
func (value SshOutcomeHostNetworkUnconfirmedPayload) MarshalJSON() ([]byte, error) {
	type plain SshOutcomeHostNetworkUnconfirmedPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type SshOutcomeTransferredPayload struct {
	Bytes uint64 `json:"bytes"`
}

func DefaultSshOutcomeTransferredPayload() SshOutcomeTransferredPayload {
	return SshOutcomeTransferredPayload{
		Bytes: 0,
	}
}
func (value *SshOutcomeTransferredPayload) UnmarshalJSON(data []byte) error {
	type plain SshOutcomeTransferredPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"bytes"}, false); err != nil {
		return err
	}
	*value = SshOutcomeTransferredPayload(decoded)
	return nil
}
func (value SshOutcomeTransferredPayload) MarshalJSON() ([]byte, error) {
	type plain SshOutcomeTransferredPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type SshOutcomeCompletedPayload struct {
	ExitCode  uint32 `json:"exit_code"`
	Stdout    string `json:"stdout"`
	Stderr    string `json:"stderr"`
	Truncated bool   `json:"truncated"`
}

func DefaultSshOutcomeCompletedPayload() SshOutcomeCompletedPayload {
	return SshOutcomeCompletedPayload{
		ExitCode:  0,
		Stdout:    "",
		Stderr:    "",
		Truncated: false,
	}
}
func (value *SshOutcomeCompletedPayload) UnmarshalJSON(data []byte) error {
	type plain SshOutcomeCompletedPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"exit_code", "stdout", "stderr", "truncated"}, false); err != nil {
		return err
	}
	*value = SshOutcomeCompletedPayload(decoded)
	return nil
}
func (value SshOutcomeCompletedPayload) MarshalJSON() ([]byte, error) {
	type plain SshOutcomeCompletedPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

var sshOutcomeVariants = variantFactories{"files_changed": nil,
	"directory":                func() any { return new(SshDirectory) },
	"download":                 func() any { return new(SshDownload) },
	"host_key_required":        func() any { return new(SshOutcomeHostKeyRequiredPayload) },
	"host_installed":           func() any { return new(SshOutcomeHostInstalledPayload) },
	"host_network_unconfirmed": func() any { return new(SshOutcomeHostNetworkUnconfirmedPayload) },
	"connected":                nil,
	"terminal":                 func() any { return new(TerminalInfo) },
	"transferred":              func() any { return new(SshOutcomeTransferredPayload) },
	"completed":                func() any { return new(SshOutcomeCompletedPayload) }}

func (value SshOutcome) MarshalJSON() ([]byte, error) {
	return MarshalUnion("kind", "", value.Kind, value.Data, sshOutcomeVariants)
}
func (value *SshOutcome) UnmarshalJSON(data []byte) error {
	kind, decoded, err := UnmarshalUnion(data, "kind", "", sshOutcomeVariants, false)
	if err != nil {
		return err
	}
	value.Kind, value.Data = kind, decoded
	return nil
}

type SshDirection string

const (
	SshDirectionUpload   SshDirection = "upload"
	SshDirectionDownload SshDirection = "download"
)

func (value SshDirection) MarshalJSON() ([]byte, error) { return marshalEnum(value) }
func (value *SshDirection) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch SshDirection(decoded) {
	case SshDirectionUpload, SshDirectionDownload:
		*value = SshDirection(decoded)
		return nil
	default:
		return unknownEnum(data, value, decoded)
	}
}
func (value SshDirection) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *SshDirection) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type SshTransfer struct {
	Worktree   WorktreeID   `json:"worktree"`
	Path       string       `json:"path"`
	RemotePath string       `json:"remote_path"`
	Direction  SshDirection `json:"direction"`
}

func DefaultSshTransfer() SshTransfer {
	return SshTransfer{
		Worktree:   WorktreeID{},
		Path:       "",
		RemotePath: "",
		Direction:  SshDirection(""),
	}
}
func (value *SshTransfer) UnmarshalJSON(data []byte) error {
	type plain SshTransfer
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"worktree", "path", "remote_path", "direction"}, false); err != nil {
		return err
	}
	*value = SshTransfer(decoded)
	return nil
}
func (value SshTransfer) MarshalJSON() ([]byte, error) {
	type plain SshTransfer
	encoded := plain(value)
	return json.Marshal(encoded)
}

type SshInstallProgress struct {
	Kind string
	Data any
}

var sshInstallProgressVariants = variantFactories{"connecting": nil,
	"detecting":  nil,
	"installing": nil,
	"pairing":    nil}

func (value SshInstallProgress) MarshalJSON() ([]byte, error) {
	return MarshalUnion("stage", "", value.Kind, value.Data, sshInstallProgressVariants)
}
func (value *SshInstallProgress) UnmarshalJSON(data []byte) error {
	kind, decoded, err := UnmarshalUnion(data, "stage", "", sshInstallProgressVariants, false)
	if err != nil {
		return err
	}
	value.Kind, value.Data = kind, decoded
	return nil
}

type SshDirectory struct {
	Path    string      `json:"path"`
	Entries []FileEntry `json:"entries"`
	Next    *SshCursor  `json:"next"`
}

func DefaultSshDirectory() SshDirectory {
	return SshDirectory{
		Path:    "",
		Entries: []FileEntry{},
		Next:    nil,
	}
}
func (value *SshDirectory) UnmarshalJSON(data []byte) error {
	type plain SshDirectory
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"path", "entries"}, false); err != nil {
		return err
	}
	*value = SshDirectory(decoded)
	return nil
}
func (value SshDirectory) MarshalJSON() ([]byte, error) {
	type plain SshDirectory
	encoded := plain(value)
	encoded.Entries = normalizeCollections(encoded.Entries)
	return json.Marshal(encoded)
}

type SshCursor struct {
	Directory bool   `json:"directory"`
	Name      string `json:"name"`
}

func DefaultSshCursor() SshCursor {
	return SshCursor{
		Directory: false,
		Name:      "",
	}
}
func (value *SshCursor) UnmarshalJSON(data []byte) error {
	type plain SshCursor
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"directory", "name"}, false); err != nil {
		return err
	}
	*value = SshCursor(decoded)
	return nil
}
func (value SshCursor) MarshalJSON() ([]byte, error) {
	type plain SshCursor
	encoded := plain(value)
	return json.Marshal(encoded)
}

type SshUploadSpec struct {
	Profile          SshID  `json:"profile"`
	ExpectedRevision uint64 `json:"expected_revision"`
	Path             string `json:"path"`
	Size             uint64 `json:"size"`
	Revision         string `json:"revision"`
	Overwrite        bool   `json:"overwrite"`
}

func DefaultSshUploadSpec() SshUploadSpec {
	return SshUploadSpec{
		Profile:          SshID{},
		ExpectedRevision: 0,
		Path:             "",
		Size:             0,
		Revision:         "",
		Overwrite:        false,
	}
}
func (value *SshUploadSpec) UnmarshalJSON(data []byte) error {
	type plain SshUploadSpec
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"profile", "expected_revision", "path", "size", "revision", "overwrite"}, false); err != nil {
		return err
	}
	*value = SshUploadSpec(decoded)
	return nil
}
func (value SshUploadSpec) MarshalJSON() ([]byte, error) {
	type plain SshUploadSpec
	encoded := plain(value)
	return json.Marshal(encoded)
}

type SshUpload struct {
	Stream StreamID      `json:"stream"`
	Spec   SshUploadSpec `json:"spec"`
}

func DefaultSshUpload() SshUpload {
	return SshUpload{
		Stream: StreamID{},
		Spec:   DefaultSshUploadSpec(),
	}
}
func (value *SshUpload) UnmarshalJSON(data []byte) error {
	type plain SshUpload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"stream", "spec"}, false); err != nil {
		return err
	}
	*value = SshUpload(decoded)
	return nil
}
func (value SshUpload) MarshalJSON() ([]byte, error) {
	type plain SshUpload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type SshDownload struct {
	Stream   StreamID `json:"stream"`
	Path     string   `json:"path"`
	Size     uint64   `json:"size"`
	Revision string   `json:"revision"`
}

func DefaultSshDownload() SshDownload {
	return SshDownload{
		Stream:   StreamID{},
		Path:     "",
		Size:     0,
		Revision: "",
	}
}
func (value *SshDownload) UnmarshalJSON(data []byte) error {
	type plain SshDownload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"stream", "path", "size", "revision"}, false); err != nil {
		return err
	}
	*value = SshDownload(decoded)
	return nil
}
func (value SshDownload) MarshalJSON() ([]byte, error) {
	type plain SshDownload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type SshFileAction struct {
	Kind string
	Data any
}
type SshFileActionCreatePayload struct {
	Directory bool `json:"directory"`
}

func DefaultSshFileActionCreatePayload() SshFileActionCreatePayload {
	return SshFileActionCreatePayload{
		Directory: false,
	}
}
func (value *SshFileActionCreatePayload) UnmarshalJSON(data []byte) error {
	type plain SshFileActionCreatePayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"directory"}, false); err != nil {
		return err
	}
	*value = SshFileActionCreatePayload(decoded)
	return nil
}
func (value SshFileActionCreatePayload) MarshalJSON() ([]byte, error) {
	type plain SshFileActionCreatePayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type SshFileActionCopyPayload struct {
	Destination string `json:"destination"`
}

func DefaultSshFileActionCopyPayload() SshFileActionCopyPayload {
	return SshFileActionCopyPayload{
		Destination: "",
	}
}
func (value *SshFileActionCopyPayload) UnmarshalJSON(data []byte) error {
	type plain SshFileActionCopyPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"destination"}, false); err != nil {
		return err
	}
	*value = SshFileActionCopyPayload(decoded)
	return nil
}
func (value SshFileActionCopyPayload) MarshalJSON() ([]byte, error) {
	type plain SshFileActionCopyPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type SshFileActionMovePayload struct {
	Destination string `json:"destination"`
}

func DefaultSshFileActionMovePayload() SshFileActionMovePayload {
	return SshFileActionMovePayload{
		Destination: "",
	}
}
func (value *SshFileActionMovePayload) UnmarshalJSON(data []byte) error {
	type plain SshFileActionMovePayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"destination"}, false); err != nil {
		return err
	}
	*value = SshFileActionMovePayload(decoded)
	return nil
}
func (value SshFileActionMovePayload) MarshalJSON() ([]byte, error) {
	type plain SshFileActionMovePayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

var sshFileActionVariants = variantFactories{"create": func() any { return new(SshFileActionCreatePayload) },
	"copy":   func() any { return new(SshFileActionCopyPayload) },
	"move":   func() any { return new(SshFileActionMovePayload) },
	"remove": nil}

func (value SshFileAction) MarshalJSON() ([]byte, error) {
	return MarshalUnion("operation", "", value.Kind, value.Data, sshFileActionVariants)
}
func (value *SshFileAction) UnmarshalJSON(data []byte) error {
	kind, decoded, err := UnmarshalUnion(data, "operation", "", sshFileActionVariants, false)
	if err != nil {
		return err
	}
	value.Kind, value.Data = kind, decoded
	return nil
}
