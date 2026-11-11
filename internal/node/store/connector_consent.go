package store

import (
	"encoding/json"
	"reflect"
	"slices"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func connectorAuthShape(auth protocol.PluginConnectorAuth) ([]string, string, string) {
	var scopes []string
	var client, flow string
	body, _ := json.Marshal(auth.Data)
	switch auth.Kind {
	case "oauth2":
		var value protocol.PluginConnectorAuthOauth2Payload
		_ = json.Unmarshal(body, &value)
		scopes, client = slices.Clone(value.Scopes), connectorClientShape(value.Client)
		encoded, _ := json.Marshal([]any{value.Flow, value.Authorize, value.Token, value.Device, value.Refresh})
		flow = string(encoded)
	case "application":
		var value protocol.PluginConnectorAuthApplicationPayload
		_ = json.Unmarshal(body, &value)
		client = value.Fields.ClientID
		if value.Fields.ClientSecret != nil {
			client += " · " + *value.Fields.ClientSecret
		}
		if value.Registration != nil && *value.Registration != nil {
			scopes = slices.Clone((**value.Registration).Scopes)
		}
		encoded, _ := json.Marshal([]any{value.Token, value.Registration})
		flow = string(encoded)
	case "mcp_oauth":
		var value protocol.PluginConnectorAuthMcpOauthPayload
		_ = json.Unmarshal(body, &value)
		if value.Client != nil {
			client = connectorClientShape(*value.Client)
		}
		flow = value.Server
	}
	slices.Sort(scopes)
	return scopes, client, flow
}

func connectorClientShape(client protocol.PluginConnectorClient) string {
	body, _ := json.Marshal(client.Data)
	if client.Kind == "Bundled" {
		var value protocol.PluginConnectorClientBundledPayload
		_ = json.Unmarshal(body, &value)
		return value.Bundled.ClientID
	}
	var value protocol.PluginConnectorClientFieldsPayload
	_ = json.Unmarshal(body, &value)
	result := value.Fields.ClientID
	if value.Fields.ClientSecret != nil {
		result += " · " + *value.Fields.ClientSecret
	}
	return result
}

func connectorConsent(previous, current *ConnectorBound) *protocol.PluginConnectorConsent {
	if reflect.DeepEqual(previous, current) {
		return nil
	}
	consent := &protocol.PluginConnectorConsent{Changes: []protocol.PluginConnectorChange{}}
	if previous == nil || current == nil {
		consent.Changes = append(consent.Changes, protocol.PluginConnectorChange{Kind: "source"})
		return consent
	}
	if !reflect.DeepEqual(previous.Origins, current.Origins) {
		consent.Changes = append(consent.Changes, protocol.PluginConnectorChange{Kind: "origins", Data: protocol.PluginConnectorChangeOriginsPayload{Previous: previous.Origins, Current: current.Origins}})
	}
	oldScopes, oldClient, oldFlow := connectorAuthShape(previous.Auth)
	newScopes, newClient, newFlow := connectorAuthShape(current.Auth)
	if !reflect.DeepEqual(oldScopes, newScopes) {
		consent.Changes = append(consent.Changes, protocol.PluginConnectorChange{Kind: "scopes", Data: protocol.PluginConnectorChangeScopesPayload{Previous: oldScopes, Current: newScopes}})
	}
	if oldClient != newClient {
		consent.Changes = append(consent.Changes, protocol.PluginConnectorChange{Kind: "client", Data: protocol.PluginConnectorChangeClientPayload{Previous: oldClient, Current: newClient}})
	}
	if oldFlow != newFlow {
		consent.Changes = append(consent.Changes, protocol.PluginConnectorChange{Kind: "flow", Data: protocol.PluginConnectorChangeFlowPayload{Previous: oldFlow, Current: newFlow}})
	}
	targets := func(bound *ConnectorBound) []string {
		result := []string{}
		for _, target := range bound.Inject {
			body, _ := json.Marshal(target)
			result = append(result, string(body))
		}
		return result
	}
	oldTargets, newTargets := targets(previous), targets(current)
	if !reflect.DeepEqual(oldTargets, newTargets) {
		consent.Changes = append(consent.Changes, protocol.PluginConnectorChange{Kind: "targets", Data: protocol.PluginConnectorChangeTargetsPayload{Previous: oldTargets, Current: newTargets}})
	}
	if !reflect.DeepEqual(previous.Targets, current.Targets) {
		names := []string{}
		for name := range previous.Targets {
			names = append(names, name)
		}
		for name := range current.Targets {
			names = append(names, name)
		}
		slices.Sort(names)
		consent.Changes = append(consent.Changes, protocol.PluginConnectorChange{Kind: "mcp", Data: protocol.PluginConnectorChangeMcpPayload{Servers: slices.Compact(names)}})
	}
	oldOrigin := pluginAccountOrigin(protocol.PluginInfo{Origin: previous.Origin})
	newOrigin := pluginAccountOrigin(protocol.PluginInfo{Origin: current.Origin})
	if !reflect.DeepEqual(oldOrigin, newOrigin) && !(oldOrigin.Kind == "imported" && newOrigin.Kind == "imported") {
		consent.Changes = append(consent.Changes, protocol.PluginConnectorChange{Kind: "source"})
	}
	if len(consent.Changes) == 0 {
		return nil
	}
	return consent
}
