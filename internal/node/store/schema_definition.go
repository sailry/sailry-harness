package store

import (
	"errors"
	"fmt"
	"maps"
	"slices"
	"strings"
	"unicode"
)

type tableDefinition struct {
	header      string
	columns     map[string]string
	constraints map[string]struct{}
	options     string
}

func parseTable(sql string) (tableDefinition, error) {
	sql = stripSQLComments(sql)
	start, end := strings.IndexByte(sql, '('), strings.LastIndexByte(sql, ')')
	if start < 0 || end < start {
		return tableDefinition{}, invalidDefinition()
	}
	clauses, err := tableClauses(sql[start+1 : end])
	if err != nil {
		return tableDefinition{}, err
	}
	result := tableDefinition{
		header: normalizeSQL(sql[:start]), columns: make(map[string]string),
		constraints: make(map[string]struct{}), options: normalizeSQL(sql[end+1:]),
	}
	var references []string
	for _, clause := range clauses {
		words := strings.Fields(clause)
		if len(words) == 0 {
			return tableDefinition{}, invalidDefinition()
		}
		first := words[0]
		keyword, _, _ := strings.Cut(strings.ToUpper(first), "(")
		switch keyword {
		case "FOREIGN":
			references = append(references, normalizeSQL(clause))
		case "PRIMARY", "UNIQUE", "CHECK", "CONSTRAINT":
			result.constraints[normalizeSQL(clause)] = struct{}{}
		default:
			name := strings.Trim(first, "\"`[]")
			if _, exists := result.columns[name]; exists {
				return tableDefinition{}, invalidDefinition()
			}
			result.columns[name] = clause
		}
	}
	for _, reference := range references {
		body, prefixed := strings.CutPrefix(reference, "FOREIGN KEY(")
		field, clause, separated := strings.Cut(body, ")")
		matched := false
		if prefixed && separated && !strings.Contains(field, ",") && strings.HasPrefix(clause, "REFERENCES ") {
			for name, declaration := range result.columns {
				if normalizeSQL(name) == normalizeSQL(strings.Trim(field, "\"`[]")) {
					result.columns[name] = declaration + " " + clause
					matched = true
					break
				}
			}
		}
		if !matched {
			result.constraints[reference] = struct{}{}
		}
	}
	return result, nil
}

func tableClauses(body string) ([]string, error) {
	var clauses []string
	var quote rune
	depth, start := 0, 0
	characters := []rune(body)
	for index := 0; index < len(characters); index++ {
		character := characters[index]
		if quote != 0 {
			if character == quote {
				if index+1 < len(characters) && characters[index+1] == quote {
					index++
				} else {
					quote = 0
				}
			}
			continue
		}
		switch character {
		case '\'', '"', '`':
			quote = character
		case '[':
			quote = ']'
		case '(':
			depth++
		case ')':
			depth--
			if depth < 0 {
				return nil, invalidDefinition()
			}
		case ',':
			if depth == 0 {
				clauses = append(clauses, strings.TrimSpace(string(characters[start:index])))
				start = index + 1
			}
		}
	}
	if quote != 0 || depth != 0 {
		return nil, invalidDefinition()
	}
	return append(clauses, strings.TrimSpace(string(characters[start:]))), nil
}

func normalizeSQL(sql string) string {
	characters := []rune(stripSQLComments(sql))
	result := make([]rune, 0, len(characters))
	var quote rune
	space := false
	for index := 0; index < len(characters); index++ {
		character := characters[index]
		if quote != 0 {
			result = append(result, character)
			if character == quote {
				if index+1 < len(characters) && characters[index+1] == quote {
					index++
					result = append(result, characters[index])
				} else {
					quote = 0
				}
			}
		} else if character == '\'' || character == '"' || character == '`' || character == '[' {
			quote = character
			if quote == '[' {
				quote = ']'
			}
			result = append(result, character)
		} else if unicode.IsSpace(character) {
			space = true
		} else if character != ';' {
			if space && (unicode.IsLetter(character) || unicode.IsNumber(character)) && len(result) > 0 {
				previous := result[len(result)-1]
				if unicode.IsLetter(previous) || unicode.IsNumber(previous) || previous == '_' {
					result = append(result, ' ')
				}
			}
			result = append(result, unicode.ToUpper(character))
			space = false
		}
	}
	normalized := string(result)
	for _, prefix := range []string{"CREATE TABLE", "CREATE INDEX", "CREATE UNIQUE INDEX"} {
		if rest, ok := strings.CutPrefix(normalized, prefix+" IF NOT EXISTS"); ok {
			return prefix + rest
		}
	}
	return normalized
}

func stripSQLComments(sql string) string {
	characters := []rune(sql)
	var result strings.Builder
	var quote rune
	for index := 0; index < len(characters); index++ {
		character := characters[index]
		if quote != 0 {
			result.WriteRune(character)
			if character == quote {
				if index+1 < len(characters) && characters[index+1] == quote {
					index++
					result.WriteRune(characters[index])
				} else {
					quote = 0
				}
			}
		} else if character == '\'' || character == '"' || character == '`' || character == '[' {
			quote = character
			if quote == '[' {
				quote = ']'
			}
			result.WriteRune(character)
		} else if character == '-' && index+1 < len(characters) && characters[index+1] == '-' {
			index += 2
			for index < len(characters) && characters[index] != '\n' {
				index++
			}
			result.WriteByte(' ')
		} else if character == '/' && index+1 < len(characters) && characters[index+1] == '*' {
			index += 2
			for index+1 < len(characters) && !(characters[index] == '*' && characters[index+1] == '/') {
				index++
			}
			index++
			result.WriteByte(' ')
		} else {
			result.WriteRune(character)
		}
	}
	return result.String()
}

func schemaTableNames(ddl string) ([]string, error) {
	var names []string
	for _, statement := range strings.Split(stripSQLComments(ddl), ";") {
		statement = strings.TrimSpace(statement)
		if !strings.HasPrefix(strings.ToUpper(statement), "CREATE TABLE ") {
			continue
		}
		definition, err := parseTable(statement)
		if err != nil {
			return nil, err
		}
		words := strings.Fields(definition.header)
		if len(words) == 0 {
			return nil, invalidDefinition()
		}
		names = append(names, strings.ToLower(strings.Trim(words[len(words)-1], "\"`[]")))
	}
	return names, nil
}

func sameTableShape(wanted, stored tableDefinition) bool {
	return wanted.header == stored.header && wanted.options == stored.options && maps.Equal(wanted.constraints, stored.constraints)
}

func sortedKeys[M ~map[K]V, K ~string, V any](values M) []K {
	keys := make([]K, 0, len(values))
	for key := range values {
		keys = append(keys, key)
	}
	slices.Sort(keys)
	return keys
}

func quoteIdentifier(identifier string) string {
	return `"` + strings.ReplaceAll(identifier, `"`, `""`) + `"`
}

func invalidDefinition() error {
	return errors.New("database table declaration could not be inspected; existing data was not changed")
}

func incompatibleDatabase(path, reason string) error {
	return fmt.Errorf("incompatible development database at %q: %s; existing data was not changed; use --data-dir <new absolute private directory> to start with a fresh profile", path, reason)
}
