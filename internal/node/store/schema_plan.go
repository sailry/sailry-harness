package store

import (
	"context"
	"database/sql"
	"encoding/hex"
	"fmt"
	"strings"
)

type schemaObject struct {
	kind string
	name string
}

type schemaObjects map[schemaObject]string

type schemaPlan struct {
	statements []string
	created    map[string]bool
	added      map[schemaObject]bool
}

func prepareSchemaPlan(ctx context.Context, connection *sql.Conn, path string, expected schemaObjects) (schemaPlan, error) {
	actual, err := readSchemaObjects(ctx, connection)
	if err != nil {
		return schemaPlan{}, err
	}
	if err := removeRetainedTables(actual, path); err != nil {
		return schemaPlan{}, err
	}
	plan := schemaPlan{created: make(map[string]bool), added: make(map[schemaObject]bool)}
	for _, identity := range sortedObjects(expected) {
		if identity.kind != "table" {
			continue
		}
		wantedSQL := expected[identity]
		storedSQL, exists := actual[identity]
		if !exists {
			plan.statements = append(plan.statements, wantedSQL)
			plan.created[identity.name] = true
			continue
		}
		wanted, err := parseTable(wantedSQL)
		if err != nil {
			return schemaPlan{}, err
		}
		stored, err := parseTable(storedSQL)
		if err != nil {
			return schemaPlan{}, err
		}
		for _, field := range sortedKeys(stored.columns) {
			if _, exists := wanted.columns[field]; !exists {
				return schemaPlan{}, incompatibleDatabase(path, fmt.Sprintf("column %s.%s is not part of the current schema", identity.name, field))
			}
		}
		for _, field := range sortedKeys(wanted.columns) {
			if existing, exists := stored.columns[field]; exists && schemaColumn(identity.name, field, existing) != schemaColumn(identity.name, field, wanted.columns[field]) {
				return schemaPlan{}, incompatibleDatabase(path, fmt.Sprintf("column %s.%s does not match the current definition", identity.name, field))
			}
		}
		if !sameTableShape(wanted, stored) {
			return schemaPlan{}, incompatibleDatabase(path, fmt.Sprintf("table %q does not match the current constraints", identity.name))
		}
		for _, field := range sortedKeys(wanted.columns) {
			if _, exists := stored.columns[field]; exists {
				continue
			}
			declaration := wanted.columns[field]
			defaultValue, hasDefault := upgradeColumnDefault(identity.name, field)
			normalized := normalizeSQL(declaration)
			if !hasDefault && strings.Contains(normalized, "NOT NULL") && !strings.Contains(normalized, "DEFAULT") {
				var populated bool
				if err := connection.QueryRowContext(ctx, "SELECT EXISTS(SELECT 1 FROM "+quoteIdentifier(identity.name)+")").Scan(&populated); err != nil {
					return schemaPlan{}, err
				}
				if populated {
					return schemaPlan{}, incompatibleDatabase(path, fmt.Sprintf("column %s.%s requires a default for existing records", identity.name, field))
				}
			}
			if hasDefault && !strings.Contains(normalized, "DEFAULT") {
				declaration += " DEFAULT X'" + strings.ToUpper(hex.EncodeToString(defaultValue)) + "'"
			}
			plan.statements = append(plan.statements, "ALTER TABLE "+quoteIdentifier(identity.name)+" ADD COLUMN "+declaration)
			plan.added[schemaObject{identity.name, field}] = true
		}
	}
	for _, identity := range sortedObjects(expected) {
		if identity.kind == "table" {
			continue
		}
		stored, exists := actual[identity]
		if !exists {
			plan.statements = append(plan.statements, expected[identity])
		} else if normalizeSQL(stored) != normalizeSQL(expected[identity]) {
			return schemaPlan{}, incompatibleDatabase(path, fmt.Sprintf("schema object %q does not match the current definition", identity.name))
		}
	}
	for _, identity := range sortedObjects(actual) {
		if _, exists := expected[identity]; !exists {
			return schemaPlan{}, incompatibleDatabase(path, fmt.Sprintf("schema object %q (%s) is not part of the current schema", identity.name, identity.kind))
		}
	}
	return plan, nil
}

func (plan schemaPlan) apply(ctx context.Context, connection *sql.Conn) error {
	for _, statement := range plan.statements {
		if _, err := connection.ExecContext(ctx, statement); err != nil {
			return err
		}
	}
	return nil
}

func schemaColumn(table, name, declaration string) string {
	normalized := normalizeSQL(declaration)
	if _, supported := upgradeColumnDefault(table, name); !supported {
		return normalized
	}
	index := strings.LastIndex(normalized, "DEFAULT X'")
	if index < 0 || !strings.HasSuffix(normalized, "'") {
		return normalized
	}
	literal := normalized[index+len("DEFAULT X'") : len(normalized)-1]
	value, err := hex.DecodeString(literal)
	if err != nil || !matchesUpgradeDefault(table, name, value) {
		return normalized
	}
	return strings.TrimSpace(normalized[:index])
}
