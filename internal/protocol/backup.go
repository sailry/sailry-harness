// Code generated from the reviewed v1 protocol declarations. DO NOT EDIT.

package protocol

import (
	"encoding/json"
)

type BackupKind string

const (
	BackupKindWebdav BackupKind = "webdav"
	BackupKindS3     BackupKind = "s3"
)

func (value BackupKind) MarshalJSON() ([]byte, error) { return marshalEnum(value) }
func (value *BackupKind) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch BackupKind(decoded) {
	case BackupKindWebdav, BackupKindS3:
		*value = BackupKind(decoded)
		return nil
	default:
		return unknownEnum(data, value, decoded)
	}
}
func (value BackupKind) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *BackupKind) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type BackupDestination struct {
	Kind     BackupKind `json:"kind"`
	Endpoint string     `json:"endpoint"`
	Root     string     `json:"root"`
	Bucket   string     `json:"bucket"`
	Region   string     `json:"region"`
}

func DefaultBackupDestination() BackupDestination {
	return BackupDestination{
		Kind:     BackupKindWebdav,
		Endpoint: "",
		Root:     "",
		Bucket:   "",
		Region:   "",
	}
}
func (value *BackupDestination) UnmarshalJSON(data []byte) error {
	type plain BackupDestination
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"kind", "endpoint", "root", "bucket", "region"}, true); err != nil {
		return err
	}
	*value = BackupDestination(decoded)
	return nil
}
func (value BackupDestination) MarshalJSON() ([]byte, error) {
	type plain BackupDestination
	encoded := plain(value)
	return json.Marshal(encoded)
}

type BackupSettings struct {
	Revision              uint64             `json:"revision"`
	Destination           *BackupDestination `json:"destination"`
	CredentialsConfigured bool               `json:"credentials_configured"`
	PassphraseConfigured  bool               `json:"passphrase_configured"`
	Local                 *BackupLocal       `json:"local,omitempty"`
	OnlineEnabled         bool               `json:"online_enabled"`
	Schedule              *BackupSchedule    `json:"schedule,omitempty"`
	// Keep is how many of this Node's newest backups each location retains
	// after a backup; zero keeps all.
	Keep uint32 `json:"keep,omitempty"`
	// Last is the outcome of the most recent backup. The Node owns it: a
	// saved value is ignored.
	Last *BackupOutcome `json:"last,omitempty"`
}

func DefaultBackupSettings() BackupSettings {
	return BackupSettings{
		Revision:              0,
		Destination:           nil,
		CredentialsConfigured: false,
		PassphraseConfigured:  false,
		Local:                 nil,
		OnlineEnabled:         true,
		Schedule:              nil,
		Keep:                  0,
		Last:                  nil,
	}
}
func (value *BackupSettings) UnmarshalJSON(data []byte) error {
	type plain BackupSettings
	decoded := plain{}
	decoded.PassphraseConfigured = false
	decoded.Local = nil
	decoded.OnlineEnabled = true
	decoded.Schedule = nil
	decoded.Keep = 0
	decoded.Last = nil
	if err := DecodeObject(data, &decoded, []string{"revision", "credentials_configured"}, true); err != nil {
		return err
	}
	*value = BackupSettings(decoded)
	return nil
}
func (value BackupSettings) MarshalJSON() ([]byte, error) {
	type plain BackupSettings
	encoded := plain(value)
	data, err := json.Marshal(encoded)
	if err != nil {
		return nil, err
	}
	var fields map[string]json.RawMessage
	if err := json.Unmarshal(data, &fields); err != nil {
		return nil, err
	}
	if encoded.OnlineEnabled {
		delete(fields, "online_enabled")
	}
	return json.Marshal(fields)
}

// BackupOutcome is how one backup ended. Error is absent after a success;
// Reason is the failure's message; Skipped counts the content left out.
type BackupOutcome struct {
	FinishedMs uint64     `json:"finished_ms"`
	Error      *ErrorCode `json:"error,omitempty"`
	Reason     string     `json:"reason,omitempty"`
	Skipped    uint32     `json:"skipped,omitempty"`
}

func DefaultBackupOutcome() BackupOutcome {
	return BackupOutcome{
		FinishedMs: 0,
		Error:      nil,
		Reason:     "",
		Skipped:    0,
	}
}
func (value *BackupOutcome) UnmarshalJSON(data []byte) error {
	type plain BackupOutcome
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"finished_ms"}, true); err != nil {
		return err
	}
	*value = BackupOutcome(decoded)
	return nil
}
func (value BackupOutcome) MarshalJSON() ([]byte, error) {
	type plain BackupOutcome
	encoded := plain(value)
	return json.Marshal(encoded)
}

type BackupLocal struct {
	Directory string `json:"directory"`
	Enabled   bool   `json:"enabled"`
}

func DefaultBackupLocal() BackupLocal {
	return BackupLocal{
		Directory: "",
		Enabled:   false,
	}
}
func (value *BackupLocal) UnmarshalJSON(data []byte) error {
	type plain BackupLocal
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"directory", "enabled"}, true); err != nil {
		return err
	}
	*value = BackupLocal(decoded)
	return nil
}
func (value BackupLocal) MarshalJSON() ([]byte, error) {
	type plain BackupLocal
	encoded := plain(value)
	return json.Marshal(encoded)
}

type BackupSchedule struct {
	Enabled  bool   `json:"enabled"`
	Timezone string `json:"timezone"`
	Hour     uint8  `json:"hour"`
	Minute   uint8  `json:"minute"`
}

func DefaultBackupSchedule() BackupSchedule {
	return BackupSchedule{
		Enabled:  false,
		Timezone: "",
		Hour:     0,
		Minute:   0,
	}
}
func (value *BackupSchedule) UnmarshalJSON(data []byte) error {
	type plain BackupSchedule
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"enabled", "timezone", "hour", "minute"}, true); err != nil {
		return err
	}
	*value = BackupSchedule(decoded)
	return nil
}
func (value BackupSchedule) MarshalJSON() ([]byte, error) {
	type plain BackupSchedule
	encoded := plain(value)
	return json.Marshal(encoded)
}

type BackupLocation string

const (
	BackupLocationLocal  BackupLocation = "local"
	BackupLocationOnline BackupLocation = "online"
)

func (value BackupLocation) MarshalJSON() ([]byte, error) { return marshalEnum(value) }
func (value *BackupLocation) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch BackupLocation(decoded) {
	case BackupLocationLocal, BackupLocationOnline:
		*value = BackupLocation(decoded)
		return nil
	default:
		return unknownEnum(data, value, decoded)
	}
}
func (value BackupLocation) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *BackupLocation) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type BackupLocations struct {
	Local  bool `json:"local"`
	Online bool `json:"online"`
}

func DefaultBackupLocations() BackupLocations {
	return BackupLocations{
		Local:  false,
		Online: false,
	}
}
func (value *BackupLocations) UnmarshalJSON(data []byte) error {
	type plain BackupLocations
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"local", "online"}, true); err != nil {
		return err
	}
	*value = BackupLocations(decoded)
	return nil
}
func (value BackupLocations) MarshalJSON() ([]byte, error) {
	type plain BackupLocations
	encoded := plain(value)
	return json.Marshal(encoded)
}

type BackupDownload struct {
	Stream   StreamID     `json:"stream"`
	Record   BackupRecord `json:"record"`
	Size     uint64       `json:"size"`
	Revision string       `json:"revision"`
}

func DefaultBackupDownload() BackupDownload {
	return BackupDownload{
		Stream:   StreamID{},
		Record:   DefaultBackupRecord(),
		Size:     0,
		Revision: "",
	}
}
func (value *BackupDownload) UnmarshalJSON(data []byte) error {
	type plain BackupDownload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"stream", "record", "size", "revision"}, true); err != nil {
		return err
	}
	*value = BackupDownload(decoded)
	return nil
}
func (value BackupDownload) MarshalJSON() ([]byte, error) {
	type plain BackupDownload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type BackupUpload struct {
	Stream   StreamID `json:"stream"`
	Size     uint64   `json:"size"`
	Revision string   `json:"revision"`
}

func DefaultBackupUpload() BackupUpload {
	return BackupUpload{
		Stream:   StreamID{},
		Size:     0,
		Revision: "",
	}
}
func (value *BackupUpload) UnmarshalJSON(data []byte) error {
	type plain BackupUpload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"stream", "size", "revision"}, true); err != nil {
		return err
	}
	*value = BackupUpload(decoded)
	return nil
}
func (value BackupUpload) MarshalJSON() ([]byte, error) {
	type plain BackupUpload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type BackupUpdate struct {
	Settings     BackupSettings             `json:"settings"`
	Identity     PluginSettingsSecretUpdate `json:"identity"`
	Secret       PluginSettingsSecretUpdate `json:"secret"`
	SessionToken PluginSettingsSecretUpdate `json:"session_token"`
	Passphrase   PluginSettingsSecretUpdate `json:"passphrase"`
}

func DefaultBackupUpdate() BackupUpdate {
	return BackupUpdate{
		Settings:     DefaultBackupSettings(),
		Identity:     PluginSettingsSecretUpdate{},
		Secret:       PluginSettingsSecretUpdate{},
		SessionToken: PluginSettingsSecretUpdate{},
		Passphrase:   PluginSettingsSecretUpdate{},
	}
}
func (value *BackupUpdate) UnmarshalJSON(data []byte) error {
	type plain BackupUpdate
	decoded := plain{}
	decoded.Passphrase = PluginSettingsSecretUpdate{Kind: "keep"}
	if err := DecodeObject(data, &decoded, []string{"settings", "identity", "secret", "session_token"}, true); err != nil {
		return err
	}
	*value = BackupUpdate(decoded)
	return nil
}
func (value BackupUpdate) MarshalJSON() ([]byte, error) {
	type plain BackupUpdate
	encoded := plain(value)
	return json.Marshal(encoded)
}

type BackupRecord struct {
	ID        string `json:"id"`
	Node      NodeID `json:"node"`
	CreatedMs uint64 `json:"created_ms"`
	SizeBytes uint64 `json:"size_bytes"`
	Files     uint32 `json:"files"`
	Sha256    string `json:"sha256"`
	// Skipped counts the content the backup left out.
	Skipped uint32 `json:"skipped,omitempty"`
}

func DefaultBackupRecord() BackupRecord {
	return BackupRecord{
		ID:        "",
		Node:      NodeID{},
		CreatedMs: 0,
		SizeBytes: 0,
		Files:     0,
		Sha256:    "",
		Skipped:   0,
	}
}
func (value *BackupRecord) UnmarshalJSON(data []byte) error {
	type plain BackupRecord
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"id", "node", "created_ms", "size_bytes", "files", "sha256"}, true); err != nil {
		return err
	}
	*value = BackupRecord(decoded)
	return nil
}
func (value BackupRecord) MarshalJSON() ([]byte, error) {
	type plain BackupRecord
	encoded := plain(value)
	return json.Marshal(encoded)
}

type BackupPage struct {
	Records   []BackupRecord             `json:"records"`
	Partial   bool                       `json:"partial"`
	Locations map[string]BackupLocations `json:"locations,omitempty"`
}

func DefaultBackupPage() BackupPage {
	return BackupPage{
		Records:   []BackupRecord{},
		Partial:   false,
		Locations: map[string]BackupLocations{},
	}
}
func (value *BackupPage) UnmarshalJSON(data []byte) error {
	type plain BackupPage
	decoded := plain{}
	decoded.Locations = map[string]BackupLocations{}
	if err := DecodeObject(data, &decoded, []string{"records", "partial"}, true); err != nil {
		return err
	}
	*value = BackupPage(decoded)
	return nil
}
func (value BackupPage) MarshalJSON() ([]byte, error) {
	type plain BackupPage
	encoded := plain(value)
	encoded.Records = normalizeCollections(encoded.Records)
	encoded.Locations = normalizeCollections(encoded.Locations)
	return json.Marshal(encoded)
}
