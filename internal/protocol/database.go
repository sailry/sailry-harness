// Hand-maintained v1 protocol declarations; keep them consistent with the fixtures and tolerance tests.

package protocol

import (
	"encoding/json"
)

type DatabaseEngine string

const (
	DatabaseEngineSqlite   DatabaseEngine = "sqlite"
	DatabaseEngineMysql    DatabaseEngine = "mysql"
	DatabaseEnginePostgres DatabaseEngine = "postgres"
)

func (value DatabaseEngine) MarshalJSON() ([]byte, error) { return marshalEnum(value) }
func (value *DatabaseEngine) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch DatabaseEngine(decoded) {
	case DatabaseEngineSqlite, DatabaseEngineMysql, DatabaseEnginePostgres:
		*value = DatabaseEngine(decoded)
		return nil
	default:
		return unknownEnum(data, value, decoded)
	}
}
func (value DatabaseEngine) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *DatabaseEngine) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type DatabaseTls string

const (
	DatabaseTlsDisable DatabaseTls = "disable"
	DatabaseTlsPrefer  DatabaseTls = "prefer"
	DatabaseTlsRequire DatabaseTls = "require"
)

func (value DatabaseTls) MarshalJSON() ([]byte, error) { return marshalEnum(value) }
func (value *DatabaseTls) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch DatabaseTls(decoded) {
	case DatabaseTlsDisable, DatabaseTlsPrefer, DatabaseTlsRequire:
		*value = DatabaseTls(decoded)
		return nil
	default:
		return unknownEnum(data, value, decoded)
	}
}
func (value DatabaseTls) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *DatabaseTls) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type DatabaseConnection struct {
	Kind string
	Data any
}
type DatabaseConnectionSqlitePayload struct {
	Path string `json:"path"`
}

func DefaultDatabaseConnectionSqlitePayload() DatabaseConnectionSqlitePayload {
	return DatabaseConnectionSqlitePayload{
		Path: "",
	}
}
func (value *DatabaseConnectionSqlitePayload) UnmarshalJSON(data []byte) error {
	type plain DatabaseConnectionSqlitePayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"path"}, false); err != nil {
		return err
	}
	*value = DatabaseConnectionSqlitePayload(decoded)
	return nil
}
func (value DatabaseConnectionSqlitePayload) MarshalJSON() ([]byte, error) {
	type plain DatabaseConnectionSqlitePayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type DatabaseConnectionSocketPayload struct {
	Engine   DatabaseEngine `json:"engine"`
	Path     string         `json:"path"`
	Port     uint16         `json:"port"`
	Database string         `json:"database"`
	Username string         `json:"username"`
}

func DefaultDatabaseConnectionSocketPayload() DatabaseConnectionSocketPayload {
	return DatabaseConnectionSocketPayload{
		Engine:   DatabaseEngine(""),
		Path:     "",
		Port:     0,
		Database: "",
		Username: "",
	}
}
func (value *DatabaseConnectionSocketPayload) UnmarshalJSON(data []byte) error {
	type plain DatabaseConnectionSocketPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"engine", "path", "port", "database", "username"}, false); err != nil {
		return err
	}
	*value = DatabaseConnectionSocketPayload(decoded)
	return nil
}
func (value DatabaseConnectionSocketPayload) MarshalJSON() ([]byte, error) {
	type plain DatabaseConnectionSocketPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type DatabaseConnectionSshPayload struct {
	Engine   DatabaseEngine `json:"engine"`
	Host     string         `json:"host"`
	Port     uint16         `json:"port"`
	Database string         `json:"database"`
	Username string         `json:"username"`
	Tls      DatabaseTls    `json:"tls"`
	Ssh      SshID          `json:"ssh"`
}

func DefaultDatabaseConnectionSshPayload() DatabaseConnectionSshPayload {
	return DatabaseConnectionSshPayload{
		Engine:   DatabaseEngine(""),
		Host:     "",
		Port:     0,
		Database: "",
		Username: "",
		Tls:      DatabaseTls(""),
		Ssh:      SshID{},
	}
}
func (value *DatabaseConnectionSshPayload) UnmarshalJSON(data []byte) error {
	type plain DatabaseConnectionSshPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"engine", "host", "port", "database", "username", "tls", "ssh"}, false); err != nil {
		return err
	}
	*value = DatabaseConnectionSshPayload(decoded)
	return nil
}
func (value DatabaseConnectionSshPayload) MarshalJSON() ([]byte, error) {
	type plain DatabaseConnectionSshPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

type DatabaseConnectionNetworkPayload struct {
	Engine   DatabaseEngine `json:"engine"`
	Host     string         `json:"host"`
	Port     uint16         `json:"port"`
	Database string         `json:"database"`
	Username string         `json:"username"`
	Tls      DatabaseTls    `json:"tls"`
}

func DefaultDatabaseConnectionNetworkPayload() DatabaseConnectionNetworkPayload {
	return DatabaseConnectionNetworkPayload{
		Engine:   DatabaseEngine(""),
		Host:     "",
		Port:     0,
		Database: "",
		Username: "",
		Tls:      DatabaseTls(""),
	}
}
func (value *DatabaseConnectionNetworkPayload) UnmarshalJSON(data []byte) error {
	type plain DatabaseConnectionNetworkPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"engine", "host", "port", "database", "username", "tls"}, false); err != nil {
		return err
	}
	*value = DatabaseConnectionNetworkPayload(decoded)
	return nil
}
func (value DatabaseConnectionNetworkPayload) MarshalJSON() ([]byte, error) {
	type plain DatabaseConnectionNetworkPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

var databaseConnectionVariants = variantFactories{"sqlite": func() any { return new(DatabaseConnectionSqlitePayload) },
	"socket":  func() any { return new(DatabaseConnectionSocketPayload) },
	"ssh":     func() any { return new(DatabaseConnectionSshPayload) },
	"network": func() any { return new(DatabaseConnectionNetworkPayload) }}

func (value DatabaseConnection) MarshalJSON() ([]byte, error) {
	return MarshalUnion("kind", "", value.Kind, value.Data, databaseConnectionVariants)
}
func (value *DatabaseConnection) UnmarshalJSON(data []byte) error {
	kind, decoded, err := UnmarshalUnion(data, "kind", "", databaseConnectionVariants, false)
	if err != nil {
		return err
	}
	value.Kind, value.Data = kind, decoded
	return nil
}

type DatabaseServer struct {
	Host     string      `json:"host"`
	Port     uint16      `json:"port"`
	Database string      `json:"database"`
	Username string      `json:"username"`
	Tls      DatabaseTls `json:"tls"`
	Socket   *string     `json:"socket"`
}

func DefaultDatabaseServer() DatabaseServer {
	return DatabaseServer{
		Host:     "",
		Port:     0,
		Database: "",
		Username: "",
		Tls:      DatabaseTls(""),
		Socket:   nil,
	}
}
func (value *DatabaseServer) UnmarshalJSON(data []byte) error {
	type plain DatabaseServer
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"host", "port", "database", "username", "tls"}, false); err != nil {
		return err
	}
	*value = DatabaseServer(decoded)
	return nil
}
func (value DatabaseServer) MarshalJSON() ([]byte, error) {
	type plain DatabaseServer
	encoded := plain(value)
	return json.Marshal(encoded)
}

type DatabaseProfile struct {
	Sharing    *ConnectionSharing `json:"sharing,omitempty"`
	ID         DatabaseID         `json:"id"`
	Revision   uint64             `json:"revision"`
	Name       string             `json:"name"`
	Connection DatabaseConnection `json:"connection"`
	ReadOnly   bool               `json:"read_only"`
}

func DefaultDatabaseProfile() DatabaseProfile {
	return DatabaseProfile{
		Sharing:    nil,
		ID:         DatabaseID{},
		Revision:   0,
		Name:       "",
		Connection: DatabaseConnection{},
		ReadOnly:   false,
	}
}
func (value *DatabaseProfile) UnmarshalJSON(data []byte) error { return Decode(data, value) }
func (value *DatabaseProfile) DecodeJSON(decoder *Decoder) error {
	type plain DatabaseProfile
	decoded := plain{}
	if err := decoder.Object(&decoded, []string{"id", "revision", "name", "connection", "read_only"}, false); err != nil {
		return err
	}
	*value = DatabaseProfile(decoded)
	return nil
}
func (value DatabaseProfile) MarshalJSON() ([]byte, error) {
	type plain DatabaseProfile
	encoded := plain(value)
	return json.Marshal(encoded)
}

type DatabaseValue struct {
	Kind string
	Data any
}

var databaseValueVariants = variantFactories{"null": nil,
	"integer": func() any { return new(int64) },
	"real":    func() any { return new(string) },
	"text":    func() any { return new(string) },
	"blob":    func() any { return new(Bytes) }}

func (value DatabaseValue) MarshalJSON() ([]byte, error) {
	return MarshalUnion("kind", "value", value.Kind, value.Data, databaseValueVariants)
}
func (value *DatabaseValue) UnmarshalJSON(data []byte) error {
	kind, decoded, err := UnmarshalUnion(data, "kind", "value", databaseValueVariants, false)
	if err != nil {
		return err
	}
	value.Kind, value.Data = kind, decoded
	return nil
}

type DatabaseResultSet struct {
	Columns      []string          `json:"columns"`
	Rows         [][]DatabaseValue `json:"rows"`
	AffectedRows uint64            `json:"affected_rows"`
	Truncated    bool              `json:"truncated"`
}

func DefaultDatabaseResultSet() DatabaseResultSet {
	return DatabaseResultSet{
		Columns:      []string{},
		Rows:         [][]DatabaseValue{},
		AffectedRows: 0,
		Truncated:    false,
	}
}
func (value *DatabaseResultSet) UnmarshalJSON(data []byte) error {
	type plain DatabaseResultSet
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"columns", "rows", "affected_rows", "truncated"}, false); err != nil {
		return err
	}
	*value = DatabaseResultSet(decoded)
	return nil
}
func (value DatabaseResultSet) MarshalJSON() ([]byte, error) {
	type plain DatabaseResultSet
	encoded := plain(value)
	encoded.Columns = normalizeCollections(encoded.Columns)
	encoded.Rows = normalizeCollections(encoded.Rows)
	return json.Marshal(encoded)
}

type DatabaseOutcome struct {
	Kind string
	Data any
}

var databaseOutcomeVariants = variantFactories{"connected": nil,
	"query":   func() any { return new(DatabaseResultSet) },
	"catalog": func() any { return new(DatabaseCatalog) }}

func (value DatabaseOutcome) MarshalJSON() ([]byte, error) {
	return MarshalUnion("kind", "data", value.Kind, value.Data, databaseOutcomeVariants)
}
func (value *DatabaseOutcome) UnmarshalJSON(data []byte) error {
	kind, decoded, err := UnmarshalUnion(data, "kind", "data", databaseOutcomeVariants, false)
	if err != nil {
		return err
	}
	value.Kind, value.Data = kind, decoded
	return nil
}

type DatabaseTable struct {
	Schema string `json:"schema"`
	Name   string `json:"name"`
}

func DefaultDatabaseTable() DatabaseTable {
	return DatabaseTable{
		Schema: "",
		Name:   "",
	}
}
func (value *DatabaseTable) UnmarshalJSON(data []byte) error {
	type plain DatabaseTable
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"schema", "name"}, false); err != nil {
		return err
	}
	*value = DatabaseTable(decoded)
	return nil
}
func (value DatabaseTable) MarshalJSON() ([]byte, error) {
	type plain DatabaseTable
	encoded := plain(value)
	return json.Marshal(encoded)
}

type DatabaseCatalog struct {
	Kind string
	Data any
}
type DatabaseCatalogTablesPayload struct {
	Database string          `json:"database"`
	Tables   []DatabaseTable `json:"tables"`
}

func DefaultDatabaseCatalogTablesPayload() DatabaseCatalogTablesPayload {
	return DatabaseCatalogTablesPayload{
		Database: "",
		Tables:   []DatabaseTable{},
	}
}
func (value *DatabaseCatalogTablesPayload) UnmarshalJSON(data []byte) error {
	type plain DatabaseCatalogTablesPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"database", "tables"}, false); err != nil {
		return err
	}
	*value = DatabaseCatalogTablesPayload(decoded)
	return nil
}
func (value DatabaseCatalogTablesPayload) MarshalJSON() ([]byte, error) {
	type plain DatabaseCatalogTablesPayload
	encoded := plain(value)
	encoded.Tables = normalizeCollections(encoded.Tables)
	return json.Marshal(encoded)
}

var databaseCatalogVariants = variantFactories{"databases": func() any { return new([]string) },
	"tables": func() any { return new(DatabaseCatalogTablesPayload) }}

func (value DatabaseCatalog) MarshalJSON() ([]byte, error) {
	return MarshalUnion("kind", "data", value.Kind, value.Data, databaseCatalogVariants)
}
func (value *DatabaseCatalog) UnmarshalJSON(data []byte) error {
	kind, decoded, err := UnmarshalUnion(data, "kind", "data", databaseCatalogVariants, false)
	if err != nil {
		return err
	}
	value.Kind, value.Data = kind, decoded
	return nil
}
