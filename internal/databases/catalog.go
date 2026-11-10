package databases

import (
	"context"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func browse(ctx context.Context, operation Operation) (protocol.DatabaseOutcome, error) {
	target, err := resolve(operation.Profile.Connection)
	if err != nil {
		return protocol.DatabaseOutcome{}, err
	}
	database := operation.Browse.Database
	var sql string
	switch {
	case target.engine == protocol.DatabaseEngineSqlite && database == nil:
		sql = "SELECT 'main'"
	case target.engine == protocol.DatabaseEngineMysql && database == nil:
		sql = "SHOW DATABASES"
	case target.engine == protocol.DatabaseEnginePostgres && database == nil:
		sql = "SELECT datname FROM pg_database WHERE datallowconn AND NOT datistemplate ORDER BY datname"
	case target.engine == protocol.DatabaseEngineSqlite:
		sql = "SELECT 'main', name FROM sqlite_schema WHERE type IN ('table','view') AND name NOT LIKE 'sqlite_%' ORDER BY name"
	case target.engine == protocol.DatabaseEngineMysql:
		sql = "SELECT TABLE_SCHEMA, TABLE_NAME FROM information_schema.TABLES WHERE TABLE_SCHEMA = DATABASE() ORDER BY TABLE_NAME"
	case target.engine == protocol.DatabaseEnginePostgres:
		sql = "SELECT table_schema, table_name FROM information_schema.tables WHERE table_schema NOT IN ('pg_catalog','information_schema') ORDER BY table_schema,table_name"
	default:
		return protocol.DatabaseOutcome{}, invalid("invalid database engine")
	}
	read := operation
	read.Profile.ReadOnly = true
	read.Browse = nil
	read.Query = &Query{SQL: sql, RowLimit: maxRowLimit, TimeoutMS: 30_000}
	outcome, err := Execute(ctx, read)
	if err != nil {
		return protocol.DatabaseOutcome{}, err
	}
	result := outcome.Data.(protocol.DatabaseResultSet)
	if result.Truncated {
		return protocol.DatabaseOutcome{}, invalid("database catalog exceeds the result limit")
	}
	if database == nil {
		names := make([]string, 0, len(result.Rows))
		for _, row := range result.Rows {
			if len(row) == 0 {
				return protocol.DatabaseOutcome{}, fault(protocol.ErrorCodeInternal, "invalid database catalog row")
			}
			name, err := catalogName(row[0])
			if err != nil {
				return protocol.DatabaseOutcome{}, err
			}
			names = append(names, name)
		}
		return catalogOutcome(protocol.DatabaseCatalog{Kind: "databases", Data: names}), nil
	}
	tables := make([]protocol.DatabaseTable, 0, len(result.Rows))
	for _, row := range result.Rows {
		if len(row) != 2 {
			return protocol.DatabaseOutcome{}, fault(protocol.ErrorCodeInternal, "invalid table catalog row")
		}
		schema, err := catalogName(row[0])
		if err != nil {
			return protocol.DatabaseOutcome{}, err
		}
		name, err := catalogName(row[1])
		if err != nil {
			return protocol.DatabaseOutcome{}, err
		}
		tables = append(tables, protocol.DatabaseTable{Schema: schema, Name: name})
	}
	return catalogOutcome(protocol.DatabaseCatalog{
		Kind: "tables",
		Data: protocol.DatabaseCatalogTablesPayload{Database: *database, Tables: tables},
	}), nil
}

func catalogOutcome(catalog protocol.DatabaseCatalog) protocol.DatabaseOutcome {
	return protocol.DatabaseOutcome{Kind: "catalog", Data: catalog}
}

func catalogName(value protocol.DatabaseValue) (string, error) {
	if name, ok := value.Data.(string); ok && value.Kind == "text" {
		return name, nil
	}
	return "", fault(protocol.ErrorCodeInternal, "database catalog returned an invalid name")
}
