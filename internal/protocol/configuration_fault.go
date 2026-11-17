package protocol

import "encoding/json"

// ConfigurationReason identifies a configuration refusal without exposing its
// message, account, package name or credential. Unknown peer reasons remain
// readable but must not be treated as an expected refusal or sent to telemetry.
type ConfigurationReason string

const (
	ConfigurationDefaultsMissing             ConfigurationReason = "defaults_missing"
	ConfigurationProviderDisabled            ConfigurationReason = "provider_disabled"
	ConfigurationProviderRemoved             ConfigurationReason = "provider_removed"
	ConfigurationProviderMissing             ConfigurationReason = "provider_missing"
	ConfigurationCredentialMissing           ConfigurationReason = "credential_missing"
	ConfigurationLoginRequired               ConfigurationReason = "login_required"
	ConfigurationAuthenticationChanged       ConfigurationReason = "authentication_changed"
	ConfigurationAuthorizationExpired        ConfigurationReason = "authorization_expired"
	ConfigurationRenewalDeferred             ConfigurationReason = "renewal_deferred"
	ConfigurationCredentialRevoked           ConfigurationReason = "credential_revoked"
	ConfigurationMachineBound                ConfigurationReason = "machine_bound"
	ConfigurationRuntimeCredentialMissing    ConfigurationReason = "runtime_credential_missing"
	ConfigurationProviderCaptureMissing      ConfigurationReason = "provider_capture_missing"
	ConfigurationRoleCaptureMissing          ConfigurationReason = "role_capture_missing"
	ConfigurationTeamCaptureMissing          ConfigurationReason = "team_capture_missing"
	ConfigurationAssistantCaptureMissing     ConfigurationReason = "assistant_capture_missing"
	ConfigurationPluginDisabled              ConfigurationReason = "plugin_disabled"
	ConfigurationPluginUnavailable           ConfigurationReason = "plugin_unavailable"
	ConfigurationPluginBindingMissing        ConfigurationReason = "plugin_binding_missing"
	ConfigurationPluginDeclarationMissing    ConfigurationReason = "plugin_declaration_missing"
	ConfigurationAuthenticationClientMissing ConfigurationReason = "authentication_client_missing"
)

func (value ConfigurationReason) Known() bool {
	switch value {
	case ConfigurationDefaultsMissing, ConfigurationProviderDisabled, ConfigurationProviderRemoved,
		ConfigurationProviderMissing, ConfigurationCredentialMissing, ConfigurationLoginRequired,
		ConfigurationAuthenticationChanged, ConfigurationAuthorizationExpired, ConfigurationRenewalDeferred,
		ConfigurationCredentialRevoked, ConfigurationMachineBound, ConfigurationRuntimeCredentialMissing,
		ConfigurationProviderCaptureMissing, ConfigurationRoleCaptureMissing, ConfigurationTeamCaptureMissing,
		ConfigurationAssistantCaptureMissing, ConfigurationPluginDisabled, ConfigurationPluginUnavailable,
		ConfigurationPluginBindingMissing, ConfigurationPluginDeclarationMissing, ConfigurationAuthenticationClientMissing:
		return true
	}
	return false
}

func (value ConfigurationReason) MarshalJSON() ([]byte, error) { return marshalEnum(value) }
func (value *ConfigurationReason) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	if !ConfigurationReason(decoded).Known() {
		return unknownEnum(data, value, decoded)
	}
	*value = ConfigurationReason(decoded)
	return nil
}

type FaultDetailsConfigurationPayload struct {
	Reason ConfigurationReason `json:"reason"`
}

func (value *FaultDetailsConfigurationPayload) UnmarshalJSON(data []byte) error {
	type plain FaultDetailsConfigurationPayload
	var decoded plain
	if err := DecodeObject(data, &decoded, []string{"reason"}, false); err != nil {
		return err
	}
	*value = FaultDetailsConfigurationPayload(decoded)
	return nil
}

func NewConfigurationFault(reason ConfigurationReason, message string) *Fault {
	return &Fault{Code: ErrorCodeNotConfigured, Message: message,
		Details: &FaultDetails{Kind: "configuration", Data: FaultDetailsConfigurationPayload{Reason: reason}}}
}

func (value Fault) ConfigurationReason() ConfigurationReason {
	if value.Details != nil && value.Details.Kind == "configuration" {
		switch detail := value.Details.Data.(type) {
		case FaultDetailsConfigurationPayload:
			return detail.Reason
		case *FaultDetailsConfigurationPayload:
			if detail != nil {
				return detail.Reason
			}
		}
	}
	return ""
}
