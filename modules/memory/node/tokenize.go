package memorynode

import (
	"slices"
	"strings"
	"unicode"
)

func isCJK(r rune) bool {
	return (r >= 0x3400 && r <= 0x4dbf) ||
		(r >= 0x4e00 && r <= 0x9fff) ||
		(r >= 0xf900 && r <= 0xfaff) ||
		(r >= 0x20000 && r <= 0x323af)
}

func isAlphaNum(r rune) bool {
	return unicode.IsLetter(r) || unicode.IsNumber(r)
}

// terms tokenizes text identically to plugins/memory/dev.sailry.platform/host/tokenize.js.
func terms(text string) []string {
	var tokens []string
	var word []rune
	var previous rune
	hasPrevious := false

	flush := func() {
		if len(word) == 0 {
			return
		}
		start := 0
		split := false
		for i := 1; i < len(word); i++ {
			nextLower := i+1 < len(word) && unicode.IsLower(word[i+1])
			if unicode.IsUpper(word[i]) && (unicode.IsLower(word[i-1]) || (unicode.IsUpper(word[i-1]) && nextLower)) {
				tokens = append(tokens, strings.ToLower(string(word[start:i])))
				start = i
				split = true
			}
		}
		if split {
			tokens = append(tokens, strings.ToLower(string(word[start:])))
		}
		tokens = append(tokens, strings.ToLower(string(word)))
		word = word[:0]
	}

	for _, ch := range text {
		if isCJK(ch) {
			flush()
			s := string(ch)
			tokens = append(tokens, s)
			if hasPrevious {
				tokens = append(tokens, string([]rune{previous, ch}))
			}
			previous = ch
			hasPrevious = true
		} else {
			hasPrevious = false
			if isAlphaNum(ch) || (len(word) > 0 && (ch == '+' || ch == '#')) {
				word = append(word, ch)
			} else {
				flush()
			}
		}
	}
	flush()
	return tokens
}

var stopWords = map[string]bool{
	"a": true, "an": true, "and": true, "are": true, "as": true, "at": true,
	"be": true, "by": true, "do": true, "does": true, "for": true, "from": true,
	"how": true, "i": true, "in": true, "is": true, "it": true, "of": true,
	"on": true, "or": true, "our": true, "that": true, "the": true, "these": true,
	"this": true, "to": true, "was": true, "we": true, "what": true, "when": true,
	"where": true, "which": true, "with": true, "you": true,
	"\u5982\u4f55": true, "\u4ec0\u4e48": true, "\u600e\u4e48": true, "\u6211\u4eec": true,
	"\u8fd9\u4e2a": true, "\u54ea\u4e9b": true, "\u662f\u5426": true,
}

func queryTerms(text string) []string {
	raw := terms(text)
	seen := make(map[string]bool, len(raw))
	selected := make([]string, 0, len(raw))
	bigram := false
	for _, t := range raw {
		if seen[t] {
			continue
		}
		seen[t] = true
		selected = append(selected, t)
		rs := []rune(t)
		if len(rs) == 2 && isCJK(rs[0]) && isCJK(rs[1]) {
			bigram = true
		}
	}
	filtered := make([]string, 0, len(selected))
	for _, t := range selected {
		if stopWords[t] {
			continue
		}
		if bigram {
			rs := []rune(t)
			if len(rs) == 1 && isCJK(rs[0]) {
				continue
			}
		}
		filtered = append(filtered, t)
	}
	slices.Sort(filtered)
	return filtered
}

func isNearDuplicate(left, right string) bool {
	lt := uniqueSortedTerms(left)
	rt := uniqueSortedTerms(right)
	return nearSets(lt, rt)
}

func uniqueSortedTerms(text string) []string {
	raw := terms(text)
	slices.Sort(raw)
	return slices.Compact(raw)
}

func nearSets(left, right []string) bool {
	if len(left) == 0 || len(right) == 0 {
		return false
	}
	required := (4*(len(left)+len(right)) + 8) / 9
	a, b, common := 0, 0, 0
	for a < len(left) && b < len(right) {
		if common+min(len(left)-a, len(right)-b) < required {
			return false
		}
		switch strings.Compare(left[a], right[b]) {
		case -1:
			a++
		case 1:
			b++
		case 0:
			a++
			b++
			common++
			if common >= required {
				return true
			}
		}
	}
	return false
}
