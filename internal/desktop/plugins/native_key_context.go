package plugins

import (
	"strings"
	"unicode"
	"unicode/utf8"
)

type nativeKeyContext map[string]*string

type nativeContextPredicate struct {
	operator string
	name     string
	value    string
	left     *nativeContextPredicate
	right    *nativeContextPredicate
}

func nativeContextIdentifier(value string, operators bool) (string, string) {
	end := 0
	for offset, character := range value {
		if !unicode.IsLetter(character) && !unicode.IsNumber(character) && character != '_' && character != '-' && (!operators || !strings.ContainsRune("><~\"?", character)) {
			break
		}
		end = offset + utf8.RuneLen(character)
	}
	return value[:end], strings.TrimSpace(value[end:])
}

func parseNativeKeyContext(source string) (nativeKeyContext, error) {
	values := nativeKeyContext{}
	for source = strings.TrimSpace(source); source != ""; {
		name, remaining := nativeContextIdentifier(source, false)
		if name == "" {
			return nil, hostInvalid("invalid plugin key context")
		}
		source = remaining
		var value *string
		if strings.HasPrefix(source, "=") {
			text, rest := nativeContextIdentifier(strings.TrimSpace(source[1:]), false)
			if text == "" {
				return nil, hostInvalid("invalid plugin key context")
			}
			value, source = &text, rest
		}
		values[name] = value
	}
	return values, nil
}

func parseNativeContextPredicate(source string) (*nativeContextPredicate, error) {
	predicate, rest, err := nativeContextExpression(strings.TrimSpace(source), 0)
	if err != nil || rest != "" {
		return nil, hostInvalid("invalid plugin shortcut context")
	}
	return predicate, nil
}

func nativeContextExpression(source string, minimum int) (*nativeContextPredicate, string, error) {
	var predicate *nativeContextPredicate
	var err error
	if source == "" {
		return nil, source, hostInvalid("invalid plugin shortcut context")
	}
	switch source[0] {
	case '(':
		predicate, source, err = nativeContextExpression(strings.TrimSpace(source[1:]), 0)
		if err != nil || !strings.HasPrefix(source, ")") {
			return nil, source, hostInvalid("invalid plugin shortcut context")
		}
		source = strings.TrimSpace(source[1:])
	case '!':
		var child *nativeContextPredicate
		child, source, err = nativeContextExpression(strings.TrimSpace(source[1:]), 5)
		predicate = &nativeContextPredicate{operator: "!", left: child}
	default:
		name, rest := nativeContextIdentifier(source, true)
		if name == "" {
			return nil, source, hostInvalid("invalid plugin shortcut context")
		}
		predicate, source = &nativeContextPredicate{name: name}, rest
	}
	if err != nil {
		return nil, source, err
	}
	for {
		operator, precedence := "", 0
		for _, candidate := range []struct {
			name       string
			precedence int
		}{{">", 1}, {"&&", 3}, {"||", 2}, {"==", 4}, {"!=", 4}} {
			if candidate.precedence >= minimum && strings.HasPrefix(source, candidate.name) {
				operator, precedence = candidate.name, candidate.precedence
				break
			}
		}
		if operator == "" {
			return predicate, source, nil
		}
		var right *nativeContextPredicate
		right, source, err = nativeContextExpression(strings.TrimSpace(source[len(operator):]), precedence+1)
		if err != nil {
			return nil, source, err
		}
		if operator == "==" || operator == "!=" {
			if predicate.operator != "" || right.operator != "" {
				return nil, source, hostInvalid("invalid plugin shortcut context")
			}
			predicate = &nativeContextPredicate{operator: operator, name: predicate.name, value: right.name}
		} else {
			predicate = &nativeContextPredicate{operator: operator, left: predicate, right: right}
		}
	}
}

func (predicate *nativeContextPredicate) depth(contexts []nativeKeyContext) int {
	for depth := len(contexts); depth > 0; depth-- {
		if predicate.evaluate(contexts[:depth], contexts) {
			return depth
		}
	}
	return -1
}

func (predicate *nativeContextPredicate) evaluate(contexts, all []nativeKeyContext) bool {
	if len(contexts) == 0 {
		return false
	}
	context := contexts[len(contexts)-1]
	switch predicate.operator {
	case "":
		_, exists := context[predicate.name]
		return exists
	case "==":
		value := context[predicate.name]
		return value != nil && *value == predicate.value
	case "!=":
		value := context[predicate.name]
		return value == nil || *value != predicate.value
	case "!":
		for depth := range all {
			if predicate.left.evaluate(all[:depth+1], all) {
				return false
			}
		}
		return true
	case ">":
		for depth := 1; depth < len(contexts); depth++ {
			if predicate.left.evaluate(contexts[:depth], all) {
				return predicate.right.evaluate(contexts[depth:], contexts[depth:])
			}
		}
		return false
	case "&&":
		return predicate.left.evaluate(contexts, all) && predicate.right.evaluate(contexts, all)
	case "||":
		return predicate.left.evaluate(contexts, all) || predicate.right.evaluate(contexts, all)
	}
	return false
}
