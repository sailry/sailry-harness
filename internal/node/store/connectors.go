package store

import (
	"context"
	"database/sql"
	"encoding/json"
	"maps"
	"net/url"
	"reflect"
	"slices"
	"strings"
	"time"
	"unicode"

	"github.com/sailry/sailry-harness/internal/node/authorization"
	"github.com/sailry/sailry-harness/internal/node/plugins"
	"github.com/sailry/sailry-harness/internal/protocol"
)

type ConnectorBound struct {
	Origin  *protocol.PluginOrigin                  `json:"origin"`
	Auth    protocol.PluginConnectorAuth            `json:"auth"`
	Origins []string                                `json:"origins"`
	Inject  []protocol.PluginConnectorInjection     `json:"inject"`
	Targets map[string]protocol.PluginMcpDefinition `json:"targets"`
}

type ConnectorRecord struct {
	ID            protocol.PluginConnectorID        `json:"id"`
	Plugin        string                            `json:"plugin"`
	Connector     string                            `json:"connector"`
	Revision      uint64                            `json:"revision"`
	AuthRevision  uint64                            `json:"auth_revision"`
	Method        protocol.PluginConnectorKind      `json:"method"`
	Enabled       bool                              `json:"enabled"`
	Fields        map[string]string                 `json:"fields"`
	Secrets       map[string]protocol.Secret        `json:"secrets"`
	Grant         *authorization.Issued             `json:"grant"`
	GrantRevision uint64                            `json:"grant_revision"`
	Identity      *protocol.PluginConnectorIdentity `json:"identity"`
	Bound         *ConnectorBound                   `json:"bound"`
	State         protocol.PluginConnectorState     `json:"state"`
	CheckedAtMs   *uint64                           `json:"checked_at_ms"`
	Login         *protocol.RequestID               `json:"login"`
}

type ConnectorLoginCapture struct {
	Attempt     protocol.PluginConnectorAttempt
	Package     protocol.PluginInfo
	Declaration protocol.PluginConnectorDeclaration
	Previous    uint64
	Record      ConnectorRecord
	Input       protocol.PluginConnectorInput
}

func ConnectorDeclarations(info protocol.PluginInfo) []protocol.PluginConnectorDeclaration {
	declarations := []protocol.PluginConnectorDeclaration{}
	if info.Extension != nil {
		declarations = append(declarations, info.Extension.Connectors...)
	}
	if info.McpSource != nil && (info.McpSource.Kind == "streamable-http" || info.McpSource.Kind == "sse") {
		var target struct {
			URL string `json:"url"`
		}
		body, _ := json.Marshal(info.McpSource.Data)
		if json.Unmarshal(body, &target) == nil {
			address, err := url.Parse(target.URL)
			if err == nil && address.Host != "" {
				origin, server, header := address.Scheme+"://"+address.Host, "server", "Authorization"
				declarations = append(declarations, protocol.PluginConnectorDeclaration{
					ID: "server", Display: protocol.PluginDesktopNavigation{Label: info.Summary.Name}, Accounts: protocol.PluginConnectorAccountsSingle,
					Origins: []string{origin}, Auth: []protocol.PluginConnectorAuth{{Kind: "mcp_oauth", Data: protocol.PluginConnectorAuthMcpOauthPayload{Server: server}}},
					Fields: []protocol.PluginConnectorField{}, Inject: []protocol.PluginConnectorInjection{{Server: &server, Header: &header, Prefix: "Bearer "}},
				})
			}
		}
	}
	return declarations
}

func ConnectorTypes(ctx context.Context, db *sql.Conn) ([]protocol.PluginConnectorType, error) {
	infos, err := ReadPluginInfos(ctx, db)
	if err != nil {
		return nil, err
	}
	result := []protocol.PluginConnectorType{}
	for _, info := range infos {
		for _, declaration := range ConnectorDeclarations(info) {
			result = append(result, protocol.PluginConnectorType{Package: plugins.PackageReference(info), Declaration: declaration, Enabled: info.Summary.Enabled && info.Summary.Compatibility.Kind == "compatible"})
		}
	}
	return result, nil
}

func ConnectorDeclaration(ctx context.Context, db *sql.Conn, reference protocol.PluginReference, connector string) (protocol.PluginInfo, protocol.PluginConnectorDeclaration, error) {
	info, err := ReadPlugin(ctx, db, reference.Name)
	if err != nil {
		return protocol.PluginInfo{}, protocol.PluginConnectorDeclaration{}, err
	}
	if info == nil || !info.Summary.Enabled || info.Summary.Compatibility.Kind != "compatible" {
		return protocol.PluginInfo{}, protocol.PluginConnectorDeclaration{}, fault(protocol.ErrorCodeNotConfigured, "connector package is unavailable")
	}
	if !reflect.DeepEqual(plugins.PackageReference(*info), reference) {
		return *info, protocol.PluginConnectorDeclaration{}, fault(protocol.ErrorCodeRevisionConflict, "connector package configuration changed")
	}
	for _, declaration := range ConnectorDeclarations(*info) {
		if declaration.ID == connector {
			return *info, declaration, nil
		}
	}
	return *info, protocol.PluginConnectorDeclaration{}, fault(protocol.ErrorCodeNotFound, "connector declaration is unavailable")
}

func ReadConnectorRecord(ctx context.Context, db *sql.Conn, id protocol.PluginConnectorID) (ConnectorRecord, error) {
	record, err := readBody[ConnectorRecord](ctx, db, "SELECT body FROM plugin_connections WHERE id=?1", id.String())
	if err != nil {
		return ConnectorRecord{}, err
	}
	if record == nil {
		return ConnectorRecord{}, fault(protocol.ErrorCodeNotFound, "connection is unavailable")
	}
	return *record, nil
}

func saveConnectorRecord(ctx context.Context, db *sql.Conn, record ConnectorRecord) error {
	body, err := json.Marshal(record)
	if err != nil {
		return err
	}
	_, err = db.ExecContext(ctx, `INSERT INTO plugin_connections(id,plugin,connector,revision,body) VALUES(?1,?2,?3,?4,?5)
ON CONFLICT(id) DO UPDATE SET revision=excluded.revision,body=excluded.body WHERE plugin=excluded.plugin AND connector=excluded.connector`,
		record.ID.String(), record.Plugin, record.Connector, record.Revision, body)
	return err
}

func connectorBound(info protocol.PluginInfo, declaration protocol.PluginConnectorDeclaration, method protocol.PluginConnectorKind) *ConnectorBound {
	for _, auth := range declaration.Auth {
		if auth.Kind != string(method) || method == protocol.PluginConnectorKindLocalCli || method == protocol.PluginConnectorKindSecret {
			continue
		}
		origins := slices.Clone(declaration.Origins)
		slices.Sort(origins)
		return &ConnectorBound{Origin: info.Origin, Auth: auth, Origins: origins, Inject: slices.Clone(declaration.Inject), Targets: map[string]protocol.PluginMcpDefinition{}}
	}
	return nil
}

func ConnectionProjection(ctx context.Context, db *sql.Conn, record ConnectorRecord) (protocol.PluginConnectorConnection, error) {
	info, err := ReadPlugin(ctx, db, record.Plugin)
	if err != nil {
		return protocol.PluginConnectorConnection{}, err
	}
	value := protocol.PluginConnectorConnection{
		ID: record.ID, Connector: record.Connector, Method: record.Method, Revision: record.Revision, Enabled: record.Enabled,
		State: record.State, Identity: record.Identity, CheckedAtMs: record.CheckedAtMs, Fields: maps.Clone(record.Fields), Configured: []string{},
		CanDisconnect: record.State != protocol.PluginConnectorStateConnecting && record.Login == nil &&
			(record.Grant != nil || len(record.Secrets) > 0 || record.Method == protocol.PluginConnectorKindLocalCli && record.Identity != nil),
	}
	for name := range record.Secrets {
		value.Configured = append(value.Configured, name)
	}
	slices.Sort(value.Configured)
	if record.Grant != nil {
		value.ExpiresAtMs = record.Grant.ExpiresAtMs
		if value.ExpiresAtMs != nil && *value.ExpiresAtMs <= uint64(time.Now().UnixMilli()) && value.State == protocol.PluginConnectorStateConnected {
			value.State = protocol.PluginConnectorStateExpired
		}
	}
	if info == nil {
		value.Package = protocol.PluginReference{Name: record.Plugin}
		value.State = protocol.PluginConnectorStateNotConnected
		return value, nil
	}
	value.Package = plugins.PackageReference(*info)
	var declaration *protocol.PluginConnectorDeclaration
	for _, candidate := range ConnectorDeclarations(*info) {
		if candidate.ID == record.Connector {
			declaration = &candidate
			break
		}
	}
	if declaration == nil || !info.Summary.Enabled || !slices.ContainsFunc(declaration.Auth, func(auth protocol.PluginConnectorAuth) bool { return auth.Kind == string(record.Method) }) {
		value.State = protocol.PluginConnectorStateNotConnected
		return value, nil
	}
	bound := connectorBound(*info, *declaration, record.Method)
	if record.Bound != nil && bound != nil {
		bound.Targets = record.Bound.Targets
	}
	if value.Consent = connectorConsent(record.Plugin, record.Bound, bound); value.Consent != nil {
		value.State = protocol.PluginConnectorStateNeedsConfirmation
	}
	return value, nil
}

func Connections(ctx context.Context, db *sql.Conn) ([]protocol.PluginConnectorConnection, error) {
	records, err := readBodies[ConnectorRecord](ctx, db, "SELECT body FROM plugin_connections ORDER BY plugin,connector,id")
	if err != nil {
		return nil, err
	}
	values := make([]protocol.PluginConnectorConnection, 0, len(records))
	for _, record := range records {
		value, err := ConnectionProjection(ctx, db, record)
		if err != nil {
			return nil, err
		}
		values = append(values, value)
	}
	return values, nil
}

func PrepareConnectorConsent(ctx context.Context, db *sql.Conn, id protocol.PluginConnectorID, expected uint64) (ConnectorLoginCapture, error) {
	var capture ConnectorLoginCapture
	record, err := ReadConnectorRecord(ctx, db, id)
	if err != nil {
		return capture, err
	}
	if err := checkRevision(record.Revision, expected); err != nil {
		return capture, err
	}
	info, err := ReadPlugin(ctx, db, record.Plugin)
	if err != nil {
		return capture, err
	}
	if info == nil {
		return capture, fault(protocol.ErrorCodeNotConfigured, "connector package is unavailable")
	}
	reference := plugins.PackageReference(*info)
	_, declaration, err := ConnectorDeclaration(ctx, db, reference, record.Connector)
	if err != nil {
		return capture, err
	}
	record.Bound = connectorBound(*info, declaration, record.Method)
	return ConnectorLoginCapture{Attempt: protocol.PluginConnectorAttempt{Connection: record.ID, Package: reference, Revision: record.Revision},
		Package: *info, Declaration: declaration, Previous: record.Revision, Record: record}, nil
}

func ProjectConnectorConsent(value protocol.PluginConnectorConnection, previous, current *ConnectorBound) protocol.PluginConnectorConnection {
	value.Consent = connectorConsent(value.Package.Name, previous, current)
	if value.Consent != nil {
		value.State = protocol.PluginConnectorStateNeedsConfirmation
	}
	return value
}

func RecoverConnectorLogins(ctx context.Context, db *sql.Conn) error {
	records, err := readBodies[ConnectorRecord](ctx, db, "SELECT body FROM plugin_connections ORDER BY plugin,connector,id")
	if err != nil {
		return err
	}
	for _, record := range records {
		if record.Login == nil {
			continue
		}
		record.Login, record.State = nil, protocol.PluginConnectorStateNotConnected
		record.Revision, err = nextRevision(record.Revision)
		if err != nil {
			return err
		}
		if err := saveConnectorRecord(ctx, db, record); err != nil {
			return err
		}
	}
	return nil
}

func connectorFields(record *ConnectorRecord, input protocol.PluginConnectorInput, declaration protocol.PluginConnectorDeclaration) error {
	for name, value := range input.Fields {
		field := slices.IndexFunc(declaration.Fields, func(field protocol.PluginConnectorField) bool { return field.Name == name && !field.Secret })
		if field < 0 || len(value) > 16*1024 || strings.ContainsFunc(value, unicode.IsControl) {
			return invalid("connector field is not declared or is invalid")
		}
		record.Fields[name] = value
	}
	for name, value := range input.Secrets {
		if !slices.ContainsFunc(declaration.Fields, func(field protocol.PluginConnectorField) bool { return field.Name == name && field.Secret }) || len(value.Expose()) > 16*1024 {
			return invalid("connector secret is not declared or is too large")
		}
		if value.Expose() == "" {
			delete(record.Secrets, name)
		} else {
			record.Secrets[name] = value
		}
	}
	for _, field := range declaration.Fields {
		if !field.Secret && record.Fields[field.Name] == "" && field.Default != nil {
			record.Fields[field.Name] = *field.Default
		}
	}
	if len(record.Fields) > 8 || len(record.Secrets) > 8 {
		return invalid("connector field capacity is exceeded")
	}
	return nil
}

func PrepareConnectorLogin(ctx context.Context, db *sql.Conn, attempt protocol.RequestID, input protocol.CommandStartConnectorLoginPayload) (ConnectorLoginCapture, error) {
	var capture ConnectorLoginCapture
	info, declaration, err := ConnectorDeclaration(ctx, db, input.Package, input.Connector)
	if err != nil {
		return capture, err
	}
	if !slices.ContainsFunc(declaration.Auth, func(auth protocol.PluginConnectorAuth) bool { return auth.Kind == string(input.Method) }) {
		return capture, invalid("connector authentication method is unavailable")
	}
	record := ConnectorRecord{
		ID: protocol.NewRequestID(), Plugin: input.Package.Name, Connector: input.Connector, Revision: 1, AuthRevision: 1,
		Method: input.Method, Enabled: true, Fields: map[string]string{}, Secrets: map[string]protocol.Secret{}, GrantRevision: 1,
	}
	previous := uint64(0)
	if input.Connection != nil {
		record, err = ReadConnectorRecord(ctx, db, *input.Connection)
		if err != nil {
			return capture, err
		}
		if record.Plugin != input.Package.Name || record.Connector != input.Connector || input.ExpectedRevision == nil {
			return capture, invalid("connector connection does not match its package")
		}
		if err := checkRevision(record.Revision, *input.ExpectedRevision); err != nil {
			return capture, err
		}
		if record.Login != nil {
			return capture, fault(protocol.ErrorCodeConflict, "connector authorization is already running")
		}
		previous = record.Revision
		record.Revision, err = nextRevision(record.Revision)
		if err != nil {
			return capture, err
		}
		record.Fields, record.Secrets = maps.Clone(record.Fields), maps.Clone(record.Secrets)
		if record.Fields == nil {
			record.Fields = map[string]string{}
		}
		if record.Secrets == nil {
			record.Secrets = map[string]protocol.Secret{}
		}
	} else {
		var exists bool
		if err := db.QueryRowContext(ctx, "SELECT EXISTS(SELECT 1 FROM plugin_connections WHERE plugin=?1 AND connector=?2)", input.Package.Name, input.Connector).Scan(&exists); err != nil {
			return capture, err
		}
		if exists {
			return capture, fault(protocol.ErrorCodeConflict, "connector already has a connection")
		}
	}
	record.Method, record.AuthRevision = input.Method, record.Revision
	if err := connectorFields(&record, input.Input, declaration); err != nil {
		return capture, err
	}
	record.Grant, record.Identity, record.CheckedAtMs = nil, nil, nil
	record.State, record.Login, record.Bound = protocol.PluginConnectorStateConnecting, &attempt, connectorBound(info, declaration, input.Method)
	capture = ConnectorLoginCapture{
		Attempt: protocol.PluginConnectorAttempt{ID: attempt, Connection: record.ID, Package: input.Package, Revision: record.Revision},
		Package: info, Declaration: declaration, Previous: previous, Record: record, Input: input.Input,
	}
	return capture, nil
}

func StartConnectorLogin(ctx context.Context, db *sql.Conn, capture ConnectorLoginCapture) error {
	if _, _, err := ConnectorDeclaration(ctx, db, capture.Attempt.Package, capture.Record.Connector); err != nil {
		return err
	}
	if capture.Previous == 0 {
		var exists bool
		if err := db.QueryRowContext(ctx, "SELECT EXISTS(SELECT 1 FROM plugin_connections WHERE plugin=?1 AND connector=?2)", capture.Record.Plugin, capture.Record.Connector).Scan(&exists); err != nil {
			return err
		}
		if exists {
			return fault(protocol.ErrorCodeConflict, "connector already has a connection")
		}
	} else {
		previous, err := ReadConnectorRecord(ctx, db, capture.Record.ID)
		if err != nil {
			return err
		}
		if err := checkRevision(previous.Revision, capture.Previous); err != nil {
			return err
		}
	}
	return saveConnectorRecord(ctx, db, capture.Record)
}

func CompleteConnectorLogin(ctx context.Context, db *sql.Conn, capture ConnectorLoginCapture, grant *authorization.Issued, identity *protocol.PluginConnectorIdentity, failed bool) (protocol.PluginConnectorConnection, error) {
	record, err := ReadConnectorRecord(ctx, db, capture.Record.ID)
	if err != nil {
		return protocol.PluginConnectorConnection{}, err
	}
	if record.Login == nil || *record.Login != capture.Attempt.ID || record.AuthRevision != capture.Record.AuthRevision || record.Method != capture.Record.Method {
		return protocol.PluginConnectorConnection{}, fault(protocol.ErrorCodeRevisionConflict, "connection authorization changed during sign-in")
	}
	if _, _, err := ConnectorDeclaration(ctx, db, capture.Attempt.Package, capture.Record.Connector); err != nil && !failed {
		return protocol.PluginConnectorConnection{}, err
	}
	record.Revision, err = nextRevision(record.Revision)
	if err != nil {
		return protocol.PluginConnectorConnection{}, err
	}
	record.Login, record.Grant, record.Identity = nil, grant, identity
	if !failed {
		record.Fields, record.Secrets = capture.Record.Fields, capture.Record.Secrets
	}
	record.State = protocol.PluginConnectorStateConnected
	if failed {
		record.State = protocol.PluginConnectorStateNotConnected
	}
	now := uint64(time.Now().UnixMilli())
	record.CheckedAtMs = &now
	record.GrantRevision, err = nextRevision(record.GrantRevision)
	if err != nil {
		return protocol.PluginConnectorConnection{}, err
	}
	if err := saveConnectorRecord(ctx, db, record); err != nil {
		return protocol.PluginConnectorConnection{}, err
	}
	return ConnectionProjection(ctx, db, record)
}

func ProbeConnector(ctx context.Context, db *sql.Conn, previous ConnectorRecord, identity *protocol.PluginConnectorIdentity, checked uint64, failure *protocol.Fault) (protocol.PluginConnectorConnection, error) {
	record, err := ReadConnectorRecord(ctx, db, previous.ID)
	if err != nil {
		return protocol.PluginConnectorConnection{}, err
	}
	if record.AuthRevision != previous.AuthRevision || record.GrantRevision != previous.GrantRevision {
		return protocol.PluginConnectorConnection{}, fault(protocol.ErrorCodeRevisionConflict, "connection changed while checking authorization")
	}
	if failure == nil {
		record.Identity, record.State = identity, protocol.PluginConnectorStateConnected
	} else {
		record.State = protocol.PluginConnectorStateFailed
		if failure.Code == protocol.ErrorCodeExpired || failure.Code == protocol.ErrorCodePermissionDenied {
			record.State = protocol.PluginConnectorStateExpired
		}
	}
	record.CheckedAtMs = &checked
	record.Revision, err = nextRevision(record.Revision)
	if err != nil {
		return protocol.PluginConnectorConnection{}, err
	}
	if err := saveConnectorRecord(ctx, db, record); err != nil {
		return protocol.PluginConnectorConnection{}, err
	}
	return ConnectionProjection(ctx, db, record)
}

func ConnectorAuthorization(ctx context.Context, db *sql.Conn, reference protocol.PluginReference, connector string, selected *protocol.PluginConnectorID, turn *protocol.TurnID) (ConnectorRecord, protocol.PluginConnectorDeclaration, error) {
	info, err := ReadPluginVersion(ctx, db, reference)
	if err != nil {
		return ConnectorRecord{}, protocol.PluginConnectorDeclaration{}, err
	}
	var declaration protocol.PluginConnectorDeclaration
	found := false
	for _, candidate := range ConnectorDeclarations(info) {
		if candidate.ID == connector {
			declaration, found = candidate, true
		}
	}
	if !found {
		return ConnectorRecord{}, declaration, fault(protocol.ErrorCodePermissionDenied, "connector is not declared by the captured package")
	}
	var record ConnectorRecord
	if selected != nil {
		record, err = ReadConnectorRecord(ctx, db, *selected)
	} else {
		var saved *ConnectorRecord
		saved, err = readBody[ConnectorRecord](ctx, db, "SELECT body FROM plugin_connections WHERE plugin=?1 AND connector=?2", reference.Name, connector)
		if err == nil && saved == nil {
			err = fault(protocol.ErrorCodeNotConfigured, "connector is not connected")
		}
		if saved != nil {
			record = *saved
		}
	}
	if err != nil {
		return record, declaration, err
	}
	if record.Plugin != reference.Name || record.Connector != connector {
		return record, declaration, fault(protocol.ErrorCodePermissionDenied, "connection belongs to another connector")
	}
	projection, err := ConnectionProjection(ctx, db, record)
	if err != nil {
		return record, declaration, err
	}
	if !record.Enabled || (projection.State != protocol.PluginConnectorStateConnected && projection.State != protocol.PluginConnectorStateExpired) {
		return record, declaration, fault(protocol.ErrorCodeNotConfigured, "connector authorization is unavailable")
	}
	capturedBound := connectorBound(info, declaration, record.Method)
	if capturedBound != nil && record.Bound != nil {
		capturedBound.Targets = record.Bound.Targets
	}
	if connectorConsent(record.Plugin, record.Bound, capturedBound) != nil {
		return record, declaration, fault(protocol.ErrorCodePermissionDenied, "connector permissions changed; confirm the connection")
	}
	if turn != nil {
		var previousID string
		var previousRevision uint64
		err := db.QueryRowContext(ctx, "SELECT connection,revision FROM turn_connections WHERE turn=?1 AND plugin=?2 AND connector=?3", turn.String(), record.Plugin, record.Connector).Scan(&previousID, &previousRevision)
		if err != nil && err != sql.ErrNoRows {
			return record, declaration, err
		}
		if err == nil && (previousID != record.ID.String() || previousRevision != record.AuthRevision) {
			return record, declaration, fault(protocol.ErrorCodeRevisionConflict, "connector authorization changed during the turn")
		}
		if err == sql.ErrNoRows {
			if _, err := db.ExecContext(ctx, "INSERT INTO turn_connections(turn,plugin,connector,connection,revision) VALUES(?1,?2,?3,?4,?5)", turn.String(), record.Plugin, record.Connector, record.ID.String(), record.AuthRevision); err != nil {
				return record, declaration, err
			}
		}
	}
	return record, declaration, nil
}

func SaveConnectorGrant(ctx context.Context, db *sql.Conn, previous ConnectorRecord, grant authorization.Issued) (ConnectorRecord, error) {
	record, err := ReadConnectorRecord(ctx, db, previous.ID)
	if err != nil {
		return ConnectorRecord{}, err
	}
	if record.AuthRevision != previous.AuthRevision || record.GrantRevision != previous.GrantRevision {
		return ConnectorRecord{}, fault(protocol.ErrorCodeRevisionConflict, "connection changed during credential renewal")
	}
	record.GrantRevision, err = nextRevision(record.GrantRevision)
	if err != nil {
		return ConnectorRecord{}, err
	}
	record.Grant, record.State = &grant, protocol.PluginConnectorStateConnected
	if err := saveConnectorRecord(ctx, db, record); err != nil {
		return ConnectorRecord{}, err
	}
	return record, nil
}
