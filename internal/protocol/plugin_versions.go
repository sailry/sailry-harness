package protocol

import "encoding/json"

// PluginRequirements survive extension decoding, including an unsupported API.
// Nil requirements identify a package with no Sailry platform extension.
type PluginRequirements struct {
	APIVersion       string  `json:"api_version"`
	MinSailryVersion *string `json:"min_sailry_version,omitempty"`
	Invalid          bool    `json:"invalid,omitempty"`
}

func (value *PluginRequirements) UnmarshalJSON(data []byte) error {
	type plain PluginRequirements
	var decoded plain
	if err := DecodeObject(data, &decoded, []string{"api_version"}, true); err != nil {
		return err
	}
	*value = PluginRequirements(decoded)
	return nil
}

type PluginCompatibilityUnsupportedPayload struct {
	Reason string `json:"reason"`
}

func (value *PluginCompatibilityUnsupportedPayload) UnmarshalJSON(data []byte) error {
	type plain PluginCompatibilityUnsupportedPayload
	var decoded plain
	if err := DecodeObject(data, &decoded, []string{"reason"}, false); err != nil {
		return err
	}
	*value = PluginCompatibilityUnsupportedPayload(decoded)
	return nil
}

// PluginInstallMode separates automatic upgrades from an explicit code switch.
type PluginInstallMode string

const (
	PluginInstallModeInstall PluginInstallMode = "install"
	PluginInstallModeUpdate  PluginInstallMode = "update"
	PluginInstallModeSwitch  PluginInstallMode = "switch"
)

func (value PluginInstallMode) MarshalJSON() ([]byte, error) { return marshalEnum(value) }
func (value *PluginInstallMode) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch mode := PluginInstallMode(decoded); mode {
	case PluginInstallModeInstall, PluginInstallModeUpdate, PluginInstallModeSwitch:
		*value = mode
		return nil
	default:
		return unknownEnum(data, value, decoded)
	}
}

type CommandInstallCatalogPluginPayload struct {
	Selection        PluginCatalogSelection `json:"selection"`
	ExpectedRevision uint64                 `json:"expected_revision"`
	Mode             PluginInstallMode      `json:"mode"`
}

func DefaultCommandInstallCatalogPluginPayload() CommandInstallCatalogPluginPayload {
	return CommandInstallCatalogPluginPayload{Selection: DefaultPluginCatalogSelection(), Mode: PluginInstallModeInstall}
}

func (value *CommandInstallCatalogPluginPayload) UnmarshalJSON(data []byte) error {
	type plain CommandInstallCatalogPluginPayload
	var decoded plain
	if err := DecodeObject(data, &decoded, []string{"selection", "expected_revision", "mode"}, false); err != nil {
		return err
	}
	*value = CommandInstallCatalogPluginPayload(decoded)
	return nil
}

type CommandSetPluginPinnedPayload struct {
	Name             string `json:"name"`
	ExpectedRevision uint64 `json:"expected_revision"`
	Pinned           bool   `json:"pinned"`
}

func DefaultCommandSetPluginPinnedPayload() CommandSetPluginPinnedPayload {
	return CommandSetPluginPinnedPayload{}
}

func (value *CommandSetPluginPinnedPayload) UnmarshalJSON(data []byte) error {
	type plain CommandSetPluginPinnedPayload
	var decoded plain
	if err := DecodeObject(data, &decoded, []string{"name", "expected_revision", "pinned"}, false); err != nil {
		return err
	}
	*value = CommandSetPluginPinnedPayload(decoded)
	return nil
}
