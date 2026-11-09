// Code generated from the reviewed v1 protocol declarations. DO NOT EDIT.

package protocol

import (
	"encoding/json"
)

type HostInstallationOutcome struct {
	Kind string
	Data any
}
type HostInstallationOutcomeInstalledPayload struct {
	Node NodeID `json:"node"`
}

func DefaultHostInstallationOutcomeInstalledPayload() HostInstallationOutcomeInstalledPayload {
	return HostInstallationOutcomeInstalledPayload{
		Node: NodeID{},
	}
}
func (value *HostInstallationOutcomeInstalledPayload) UnmarshalJSON(data []byte) error {
	type plain HostInstallationOutcomeInstalledPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"node"}, false); err != nil {
		return err
	}
	*value = HostInstallationOutcomeInstalledPayload(decoded)
	return nil
}
func (value HostInstallationOutcomeInstalledPayload) MarshalJSON() ([]byte, error) {
	type plain HostInstallationOutcomeInstalledPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type HostInstallationOutcomeNetworkUnconfirmedPayload struct {
	Node      NodeID           `json:"node"`
	Selection NetworkSelection `json:"selection"`
}

func DefaultHostInstallationOutcomeNetworkUnconfirmedPayload() HostInstallationOutcomeNetworkUnconfirmedPayload {
	return HostInstallationOutcomeNetworkUnconfirmedPayload{
		Node:      NodeID{},
		Selection: NetworkSelection{},
	}
}
func (value *HostInstallationOutcomeNetworkUnconfirmedPayload) UnmarshalJSON(data []byte) error {
	type plain HostInstallationOutcomeNetworkUnconfirmedPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"node", "selection"}, false); err != nil {
		return err
	}
	*value = HostInstallationOutcomeNetworkUnconfirmedPayload(decoded)
	return nil
}
func (value HostInstallationOutcomeNetworkUnconfirmedPayload) MarshalJSON() ([]byte, error) {
	type plain HostInstallationOutcomeNetworkUnconfirmedPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

var hostInstallationOutcomeVariants = variantFactories{"installed": func() any { return new(HostInstallationOutcomeInstalledPayload) },
	"network_unconfirmed": func() any { return new(HostInstallationOutcomeNetworkUnconfirmedPayload) }}

func (value HostInstallationOutcome) MarshalJSON() ([]byte, error) {
	return MarshalUnion("kind", "", value.Kind, value.Data, hostInstallationOutcomeVariants)
}
func (value *HostInstallationOutcome) UnmarshalJSON(data []byte) error {
	kind, decoded, err := UnmarshalUnion(data, "kind", "", hostInstallationOutcomeVariants, false)
	if err != nil {
		return err
	}
	value.Kind, value.Data = kind, decoded
	return nil
}
