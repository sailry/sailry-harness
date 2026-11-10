package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"reflect"
	"strings"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func ExecuteConnectorCommand(ctx context.Context, db *sql.Conn, command protocol.Command, consent *ConnectorLoginCapture) (protocol.Output, error) {
	var id protocol.PluginConnectorID
	var expected uint64
	switch input := command.Data.(type) {
	case *protocol.CommandUpdateConnectionPayload:
		id, expected = input.Connection, input.ExpectedRevision
	case *protocol.CommandSetConnectionEnabledPayload:
		id, expected = input.Connection, input.ExpectedRevision
	case *protocol.CommandDisconnectConnectionPayload:
		id, expected = input.Connection, input.ExpectedRevision
	case *protocol.CommandRemoveConnectionPayload:
		id, expected = input.Connection, input.ExpectedRevision
	case *protocol.CommandConfirmConnectorConsentPayload:
		id, expected = input.Connection, input.ExpectedRevision
	default:
		return protocol.Output{}, fault(protocol.ErrorCodeNotConfigured, "connector command is unavailable")
	}
	record, err := ReadConnectorRecord(ctx, db, id)
	if err != nil {
		return protocol.Output{}, err
	}
	if err := checkRevision(record.Revision, expected); err != nil {
		return protocol.Output{}, err
	}
	projection, err := ConnectionProjection(ctx, db, record)
	if err != nil {
		return protocol.Output{}, err
	}
	if command.Kind == "confirm_connector_consent" {
		if consent == nil || consent.Record.ID != record.ID || consent.Previous != record.Revision {
			return protocol.Output{}, fault(protocol.ErrorCodeRevisionConflict, "connector consent capture is unavailable")
		}
		if _, _, err := ConnectorDeclaration(ctx, db, consent.Attempt.Package, record.Connector); err != nil {
			return protocol.Output{}, err
		}
		projection = ProjectConnectorConsent(projection, record.Bound, consent.Record.Bound)
	}
	if command.Kind == "remove_connection" {
		if _, err := db.ExecContext(ctx, "DELETE FROM plugin_connections WHERE id=?1", id.String()); err != nil {
			return protocol.Output{}, err
		}
		values, err := Connections(ctx, db)
		return protocol.Output{Kind: "connections", Data: values}, err
	}
	switch input := command.Data.(type) {
	case *protocol.CommandSetConnectionEnabledPayload:
		record.Enabled = input.Enabled
	case *protocol.CommandDisconnectConnectionPayload:
		if !projection.CanDisconnect {
			return protocol.Output{}, fault(protocol.ErrorCodeConflict, "connection cannot disconnect while authorization is running")
		}
		record.Grant, record.Identity, record.Secrets = nil, nil, map[string]protocol.Secret{}
		record.State, record.Login = protocol.PluginConnectorStateNotConnected, nil
		record.AuthRevision = record.Revision + 1
	case *protocol.CommandUpdateConnectionPayload:
		_, declaration, err := ConnectorDeclaration(ctx, db, projection.Package, record.Connector)
		if err != nil {
			return protocol.Output{}, err
		}
		body, _ := json.Marshal(record)
		var previous ConnectorRecord
		_ = json.Unmarshal(body, &previous)
		if err := connectorFields(&record, input.Input, declaration); err != nil {
			return protocol.Output{}, err
		}
		if connectorAuthorizationChanged(previous, record, declaration) {
			record.Grant, record.Identity, record.Login = nil, nil, nil
			record.State, record.AuthRevision = protocol.PluginConnectorStateNotConnected, record.Revision+1
		}
	case *protocol.CommandConfirmConnectorConsentPayload:
		if projection.Consent == nil {
			return protocol.Output{}, fault(protocol.ErrorCodeConflict, "connection does not require confirmation")
		}
		if !input.Accept {
			record.Enabled = false
		} else {
			record.Bound = consent.Record.Bound
		}
		record.AuthRevision = record.Revision + 1
	}
	record.Revision, err = nextRevision(record.Revision)
	if err != nil {
		return protocol.Output{}, err
	}
	if err := saveConnectorRecord(ctx, db, record); err != nil {
		return protocol.Output{}, err
	}
	projection, err = ConnectionProjection(ctx, db, record)
	return protocol.Output{Kind: "connection", Data: projection}, err
}

func connectorAuthorizationChanged(previous, current ConnectorRecord, declaration protocol.PluginConnectorDeclaration) bool {
	if previous.Method != current.Method || !reflect.DeepEqual(previous.Secrets, current.Secrets) {
		return true
	}
	body, _ := json.Marshal(struct {
		Auth     []protocol.PluginConnectorAuth
		Inject   []protocol.PluginConnectorInjection
		Identity *protocol.PluginConnectorIdentityRequest
		Health   *protocol.PluginConnectorHealthRequest
	}{declaration.Auth, declaration.Inject, declaration.Identity, declaration.Health})
	for name, value := range current.Fields {
		if previous.Fields[name] != value && (strings.Contains(string(body), "{"+name+"}") || strings.Contains(string(body), `"`+name+`"`)) {
			return true
		}
	}
	for name := range previous.Fields {
		if _, exists := current.Fields[name]; !exists && strings.Contains(string(body), "{"+name+"}") {
			return true
		}
	}
	return false
}
