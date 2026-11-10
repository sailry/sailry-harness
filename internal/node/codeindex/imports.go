package codeindex

import (
	"slices"
	"strings"
)

const (
	maxKeys     = 256
	maxKeyBytes = 512
)

var scriptExtensions = []string{".js", ".jsx", ".mjs", ".cjs", ".ts", ".tsx", ".mts", ".cts"}

// modules turns import statements into slash-separated module keys.
// Resolution prefers definitions whose file or directory path ends with a
// key; keys are hints, not a module resolver.
func modules(language, file string, imported []statement) []string {
	directory := ""
	if index := strings.LastIndexByte(file, '/'); index >= 0 {
		directory = file[:index]
	}
	var keys []string
	for _, statement := range imported {
		text := strings.TrimSpace(statement.source)
		var found []string
		switch language {
		case "python":
			found = pythonModules(text, directory, statement.items)
		case "javascript", "typescript", "tsx":
			if specifier, ok := quoted(text); ok {
				found = []string{scriptModule(specifier, directory)}
			}
		case "rust":
			found = rustModules(text)
		case "go":
			if specifier, ok := quoted(text); ok {
				found = []string{strings.Trim(specifier, "/")}
			}
		case "java", "kotlin":
			found = dottedModules(text)
		}
		for _, key := range found {
			if key != "" && len(key) <= maxKeyBytes {
				keys = append(keys, key)
			}
		}
	}
	slices.Sort(keys)
	keys = slices.Compact(keys)
	return keys[:min(len(keys), maxKeys)]
}

// aliases maps local names to the one differently named item they import.
func aliases(imported []statement) map[string]string {
	found := map[string]string{}
	for _, statement := range imported {
		if len(statement.items) == 1 && statement.alias != "" && statement.items[0] != statement.alias {
			found[statement.alias] = last(statement.items[0])
		}
	}
	return found
}

// last returns the final segment of a qualified name.
func last(name string) string {
	parts := strings.FieldsFunc(name, func(character rune) bool { return strings.ContainsRune(`.:\/`, character) })
	if len(parts) == 0 {
		return name
	}
	return parts[len(parts)-1]
}

func pythonModules(text, directory string, items []string) []string {
	if rest, ok := strings.CutPrefix(text, "from "); ok {
		module, _, ok := strings.Cut(rest, " import")
		if !ok {
			return nil
		}
		module = strings.TrimSpace(module)
		dots := len(module) - len(strings.TrimLeft(module, "."))
		var base []string
		if dots > 0 {
			base = segments(directory, "/")
			base = base[:max(len(base)-(dots-1), 0)]
		}
		base = append(base, segments(module[dots:], ".")...)
		keys := []string{strings.Join(base, "/")}
		for _, entry := range items[:min(len(items), maxItems)] {
			keys = append(keys, strings.Join(append(slices.Clone(base), segments(entry, ".")...), "/"))
		}
		return keys
	}
	rest, ok := strings.CutPrefix(text, "import ")
	if !ok {
		return nil
	}
	var keys []string
	for _, entry := range strings.Split(rest, ",") {
		name, _, _ := strings.Cut(entry, " as ")
		keys = append(keys, strings.Join(segments(strings.Trim(name, "() "), "."), "/"))
	}
	return keys
}

func scriptModule(specifier, directory string) string {
	for _, extension := range scriptExtensions {
		if stem, ok := strings.CutSuffix(specifier, extension); ok {
			specifier = stem
			break
		}
	}
	if !strings.HasPrefix(specifier, ".") {
		if rest, ok := strings.CutPrefix(specifier, "@/"); ok {
			specifier = rest
		} else if rest, ok := strings.CutPrefix(specifier, "~/"); ok {
			specifier = rest
		}
		return strings.Trim(specifier, "/")
	}
	resolved := segments(directory, "/")
	for _, segment := range strings.Split(specifier, "/") {
		switch segment {
		case "", ".":
		case "..":
			resolved = resolved[:max(len(resolved)-1, 0)]
		default:
			resolved = append(resolved, segment)
		}
	}
	return strings.Join(resolved, "/")
}

func rustModules(text string) []string {
	_, rest, ok := strings.Cut(text, "use ")
	if !ok {
		return nil
	}
	rest = strings.TrimSpace(strings.TrimRight(strings.TrimSpace(rest), ";"))
	prefix, group, grouped := strings.Cut(rest, "{")
	name, _, _ := strings.Cut(prefix, " as ")
	base := rustPath(name)
	keys := []string{strings.Join(base, "/")}
	if !grouped {
		if len(base) > 1 {
			keys = append(keys, strings.Join(base[:len(base)-1], "/"))
		}
		return keys
	}
	entries := strings.Split(strings.TrimRight(group, "}"), ",")
	for _, entry := range entries[:min(len(entries), maxItems)] {
		entry = strings.TrimFunc(entry, func(character rune) bool {
			return isSpace(character) || character == '{' || character == '}'
		})
		name, _, _ := strings.Cut(entry, " as ")
		keys = append(keys, strings.Join(append(slices.Clone(base), rustPath(name)...), "/"))
	}
	return keys
}

// rustPath returns path segments without crate-relative markers or globs.
func rustPath(name string) []string {
	var found []string
	for _, segment := range strings.Split(name, "::") {
		switch segment = strings.TrimSpace(segment); segment {
		case "", "crate", "self", "super", "*":
		default:
			found = append(found, segment)
		}
	}
	return found
}

func dottedModules(text string) []string {
	name := ""
	for _, word := range strings.Fields(strings.TrimRight(text, ";")) {
		if word != "import" && word != "static" {
			name = word
			break
		}
	}
	for strings.HasSuffix(name, ".*") {
		name = strings.TrimSuffix(name, ".*")
	}
	parts := segments(name, ".")
	keys := []string{strings.Join(parts, "/")}
	if len(parts) > 1 {
		keys = append(keys, strings.Join(parts[:len(parts)-1], "/"))
	}
	return keys
}

func quoted(text string) (string, bool) {
	start := strings.IndexAny(text, "\"'`")
	if start < 0 {
		return "", false
	}
	rest := text[start+1:]
	end := strings.IndexByte(rest, text[start])
	if end < 0 {
		return "", false
	}
	return rest[:end], true
}

func segments(text, separator string) []string {
	var found []string
	for _, segment := range strings.Split(text, separator) {
		if segment = strings.TrimSpace(segment); segment != "" {
			found = append(found, segment)
		}
	}
	return found
}
