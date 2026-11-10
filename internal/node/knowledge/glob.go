package knowledge

import (
	"regexp"
	"strings"
)

// Glob compiles a collection filter with the reference matcher's defaults:
// "*" and "?" cross path separators, "**" spans whole directories, classes
// negate with "!" or "^", braces alternate and a backslash escapes. doublestar
// stops "*" at a separator, so the pattern is translated instead.
func Glob(pattern string) (*regexp.Regexp, error) {
	var expression strings.Builder
	expression.WriteString(`(?s)\A`)
	depth := 0
	for index := 0; index < len(pattern); {
		character := pattern[index]
		index++
		switch character {
		case '?':
			expression.WriteByte('.')
		case '*':
			stars := 1
			for index < len(pattern) && pattern[index] == '*' {
				stars++
				index++
			}
			start := index - stars
			leading := start == 0 || pattern[start-1] == '/'
			switch {
			case stars == 2 && leading && index < len(pattern) && pattern[index] == '/':
				// A leading directory span may be empty.
				expression.WriteString(`(?:.*/)?`)
				index++
			default:
				expression.WriteString(`.*`)
			}
		case '[':
			end := strings.IndexByte(pattern[index:], ']')
			if end == 0 {
				// A leading bracket is a member of the class.
				end = strings.IndexByte(pattern[index+1:], ']') + 1
			}
			if end <= 0 {
				return nil, invalid("invalid knowledge glob")
			}
			class := pattern[index : index+end]
			index += end + 1
			expression.WriteByte('[')
			if class[0] == '!' || class[0] == '^' {
				expression.WriteByte('^')
				class = class[1:]
			}
			for position := 0; position < len(class); position++ {
				if class[position] == '-' && position > 0 && position < len(class)-1 {
					expression.WriteByte('-')
				} else {
					literal(&expression, class[position])
				}
			}
			expression.WriteByte(']')
		case '{':
			depth++
			expression.WriteString(`(?:`)
		case '}':
			if depth == 0 {
				return nil, invalid("invalid knowledge glob")
			}
			depth--
			expression.WriteByte(')')
		case ',':
			if depth > 0 {
				expression.WriteByte('|')
			} else {
				expression.WriteByte(',')
			}
		case '\\':
			if index == len(pattern) {
				return nil, invalid("invalid knowledge glob")
			}
			literal(&expression, pattern[index])
			index++
		default:
			literal(&expression, character)
		}
	}
	if depth != 0 {
		return nil, invalid("invalid knowledge glob")
	}
	expression.WriteString(`\z`)
	compiled, err := regexp.Compile(expression.String())
	if err != nil {
		return nil, invalid("invalid knowledge glob")
	}
	return compiled, nil
}

// literal writes one pattern byte; bytes of a multi-byte character pass through.
func literal(expression *strings.Builder, character byte) {
	if character < 0x80 {
		expression.WriteString(regexp.QuoteMeta(string(rune(character))))
	} else {
		expression.WriteByte(character)
	}
}
