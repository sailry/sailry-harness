package knowledge

import (
	"slices"
	"strings"
	"unicode"

	"golang.org/x/text/unicode/norm"
)

func cjk(character rune) bool {
	if !unicode.IsLetter(character) && !unicode.Is(unicode.Other_Alphabetic, character) {
		return false
	}
	return character >= 0x1100 && character <= 0x11ff ||
		character >= 0x3040 && character <= 0x30ff ||
		character >= 0x3130 && character <= 0x318f ||
		character >= 0x31f0 && character <= 0x31ff ||
		character >= 0x3400 && character <= 0x4dbf ||
		character >= 0x4e00 && character <= 0x9fff ||
		character >= 0xa960 && character <= 0xa97f ||
		character >= 0xac00 && character <= 0xd7ff ||
		character >= 0xf900 && character <= 0xfaff ||
		character >= 0xff66 && character <= 0xff9d ||
		character >= 0x1b000 && character <= 0x1b16f ||
		character >= 0x20000 && character <= 0x323af
}

func separated(text string) string {
	var result strings.Builder
	for _, character := range text {
		if cjk(character) {
			result.WriteRune(' ')
			result.WriteRune(character)
			result.WriteRune(' ')
		} else {
			result.WriteRune(character)
		}
	}
	return result.String()
}

// Field folds only derived search fields; source text stays intact.
func Field(text string) string {
	var result strings.Builder
	separate := true
	for _, character := range norm.NFKD.String(text) {
		if unicode.IsMark(character) {
			continue
		}
		character = unicode.ToLower(character)
		if unicode.IsLetter(character) || unicode.IsNumber(character) || unicode.Is(unicode.Other_Alphabetic, character) || character == '+' || character == '#' {
			result.WriteRune(character)
			separate = false
		} else if !separate {
			result.WriteByte(' ')
			separate = true
		}
	}
	return strings.TrimRight(result.String(), " ")
}

// Index preserves a phrase stream followed by adjacent CJK pairs.
func Index(text string) string {
	if !strings.ContainsFunc(text, cjk) {
		return ""
	}
	var result strings.Builder
	result.WriteString(separated(text))
	var previous rune
	for _, character := range text {
		if cjk(character) {
			if previous != 0 {
				result.WriteRune(' ')
				result.WriteRune(previous)
				result.WriteRune(character)
			}
			previous = character
		} else {
			previous = 0
		}
	}
	return Field(result.String())
}

func quoted(text string) string {
	text = Field(text)
	if text == "" {
		return ""
	}
	return `"` + strings.NewReplacer(`\`, `\\`, `"`, `\"`).Replace(text) + `"`
}

// Phrase treats one explicit search term as a literal phrase.
func Phrase(text string) string {
	if strings.ContainsFunc(text, cjk) {
		text = separated(text)
	}
	return quoted(text)
}

// Query matches independent words and adjacent CJK pairs in stable order.
func Query(text string) string {
	terms := make(map[string]struct{})
	var word strings.Builder
	var ideographs []rune
	takeWord := func() {
		if word.Len() != 0 {
			terms[Field(word.String())] = struct{}{}
			word.Reset()
		}
	}
	takeBigrams := func() {
		if len(ideographs) == 1 {
			terms[Field(string(ideographs))] = struct{}{}
		} else {
			for index := 1; index < len(ideographs); index++ {
				terms[Field(string(ideographs[index-1:index+1]))] = struct{}{}
			}
		}
		ideographs = ideographs[:0]
	}
	for _, character := range text {
		if unicode.IsMark(character) {
			continue
		}
		if cjk(character) {
			takeWord()
			ideographs = append(ideographs, character)
		} else {
			takeBigrams()
			if unicode.IsLetter(character) || unicode.IsNumber(character) || unicode.Is(unicode.Other_Alphabetic, character) || character == '+' || character == '#' {
				word.WriteRune(character)
			} else {
				takeWord()
			}
		}
	}
	takeWord()
	takeBigrams()
	ordered := make([]string, 0, len(terms))
	for term := range terms {
		ordered = append(ordered, term)
	}
	slices.Sort(ordered)
	for index, term := range ordered {
		ordered[index] = quoted(term)
	}
	return strings.Join(ordered, " OR ")
}
