// Package compact owns bounded structured handoffs, never canonical history.
// Adapted from Goose 14741e9a (Apache-2.0); see ../model/LICENSE.goose.
package compact

import (
	"encoding/json"
	"fmt"
	"slices"
	"strings"

	"github.com/sailry/sailry-harness/internal/node/agent/tokens"
)

type File struct {
	Path    string  `json:"path"`
	Summary string  `json:"summary"`
	KeyCode *string `json:"key_code"`
}
type Summary struct {
	UserIntent        []string `json:"user_intent"`
	TechnicalConcepts []string `json:"technical_concepts"`
	Files             []File   `json:"files"`
	ErrorsAndFixes    []string `json:"errors_and_fixes"`
	ProblemSolving    []string `json:"problem_solving"`
	UserMessages      []string `json:"user_messages"`
	PendingTasks      []string `json:"pending_tasks"`
	CurrentWork       *string  `json:"current_work"`
	NextStep          *string  `json:"next_step"`
}

func stringify(value any) string {
	switch value := value.(type) {
	case nil:
		return ""
	case string:
		return value
	case []any:
		var parts []string
		for _, v := range value {
			parts = append(parts, stringify(v))
		}
		return strings.Join(parts, "; ")
	case map[string]any:
		keys := make([]string, 0, len(value))
		for key := range value {
			keys = append(keys, key)
		}
		slices.Sort(keys)
		var parts []string
		for _, key := range keys {
			parts = append(parts, key+": "+stringify(value[key]))
		}
		return strings.Join(parts, "; ")
	default:
		raw, _ := json.Marshal(value)
		return string(raw)
	}
}
func texts(value any) []string {
	var values []any
	if v, ok := value.([]any); ok {
		values = v
	} else if value != nil {
		values = []any{value}
	}
	out := []string{}
	for _, v := range values {
		if text := stringify(v); strings.TrimSpace(text) != "" {
			out = append(out, text)
		}
	}
	return out
}
func optional(value any) *string {
	if value == nil {
		return nil
	}
	text := stringify(value)
	if strings.TrimSpace(text) == "" {
		return nil
	}
	return &text
}

// Parse reads the handoff of a summary response and bounds its rendered text
// to limit tokens.
func Parse(text string, limit uint64) (Summary, error) {
	var candidates []string
	cuts := []int{}
	for offset := 0; ; {
		i := strings.Index(text[offset:], "</analysis>")
		if i < 0 {
			break
		}
		offset += i + len("</analysis>")
		cuts = append(cuts, offset)
	}
	if len(cuts) == 0 {
		cuts = []int{0}
	}
	for i := len(cuts) - 1; i >= 0; i-- {
		tail := text[cuts[i]:]
		later := strings.Count(tail, "</analysis>")
		appendCandidate := func(candidate string) {
			if candidate != "" && strings.Count(candidate, "</analysis>") == later {
				candidates = append(candidates, candidate)
			}
		}
		markers := []int{}
		for offset := 0; ; {
			index := strings.Index(tail[offset:], "```json")
			if index < 0 {
				break
			}
			offset += index + len("```json")
			markers = append(markers, offset)
		}
		for j := len(markers) - 1; j >= 0; j-- {
			appendCandidate(object(tail[markers[j]:]))
		}
		appendCandidate(object(tail))
	}
	if candidate := object(text); candidate != "" {
		candidates = append(candidates, candidate)
	}
	for _, candidate := range candidates {
		var fields map[string]any
		variants := []string{candidate, repair(candidate), escape(candidate), repair(escape(candidate))}
		parsed := false
		for _, variant := range variants {
			decoder := json.NewDecoder(strings.NewReader(variant))
			decoder.UseNumber()
			if decoder.Decode(&fields) == nil && fields != nil {
				parsed = true
				break
			}
		}
		if !parsed {
			continue
		}
		summary := Summary{UserIntent: texts(fields["user_intent"]), TechnicalConcepts: texts(fields["technical_concepts"]), ErrorsAndFixes: texts(fields["errors_and_fixes"]), ProblemSolving: texts(fields["problem_solving"]), UserMessages: texts(fields["user_messages"]), PendingTasks: texts(fields["pending_tasks"]), CurrentWork: optional(fields["current_work"]), NextStep: optional(fields["next_step"]), Files: []File{}}
		files := []any{}
		if items, ok := fields["files"].([]any); ok {
			files = items
		} else if fields["files"] != nil {
			files = append(files, fields["files"])
		}
		for _, file := range files {
			value := File{}
			if fields, ok := file.(map[string]any); ok {
				value = File{Path: stringify(fields["path"]), Summary: stringify(fields["summary"]), KeyCode: optional(fields["key_code"])}
			} else {
				value.Path = stringify(file)
			}
			if strings.TrimSpace(value.Path) != "" || strings.TrimSpace(value.Summary) != "" || value.KeyCode != nil {
				summary.Files = append(summary.Files, value)
			}
		}
		if len(summary.UserIntent)+len(summary.TechnicalConcepts)+len(summary.ErrorsAndFixes)+len(summary.ProblemSolving)+len(summary.UserMessages)+len(summary.PendingTasks)+len(summary.Files) == 0 && summary.CurrentWork == nil && summary.NextStep == nil {
			continue
		}
		summary.Bound(limit)
		if tokens.Text(summary.Render()) > limit {
			continue
		}
		return summary, nil
	}
	return Summary{}, fmt.Errorf("compaction summary was not a structured handoff")
}

func object(text string) string {
	text = strings.TrimSpace(text)
	if !strings.HasPrefix(text, "{") {
		return ""
	}
	depth := 0
	quoted, escaped := false, false
	for i, r := range text {
		if quoted {
			if escaped {
				escaped = false
			} else if r == '\\' {
				escaped = true
			} else if r == '"' {
				quoted = false
			}
			continue
		}
		switch r {
		case '"':
			quoted = true
		case '{':
			depth++
		case '}':
			depth--
			if depth == 0 {
				return text[:i+1]
			}
		}
	}
	return ""
}
func repair(text string) string {
	var out strings.Builder
	var closers []rune
	quoted, escaped := false, false
	for _, r := range text {
		out.WriteRune(r)
		if quoted {
			if escaped {
				escaped = false
			} else if r == '\\' {
				escaped = true
			} else if r == '"' {
				quoted = false
			}
			continue
		}
		switch r {
		case '"':
			quoted = true
		case '{':
			closers = append(closers, '}')
		case '[':
			closers = append(closers, ']')
		case '}', ']':
			if len(closers) > 0 && closers[len(closers)-1] == r {
				closers = closers[:len(closers)-1]
			}
		}
	}
	if quoted {
		if escaped {
			out.WriteByte('\\')
		}
		out.WriteByte('"')
	}
	for i := len(closers) - 1; i >= 0; i-- {
		out.WriteRune(closers[i])
	}
	return out.String()
}
func escape(text string) string {
	var out strings.Builder
	for _, r := range text {
		if r < 32 {
			switch r {
			case '\b':
				out.WriteString(`\b`)
			case '\f':
				out.WriteString(`\f`)
			case '\n':
				out.WriteString(`\n`)
			case '\r':
				out.WriteString(`\r`)
			case '\t':
				out.WriteString(`\t`)
			default:
				fmt.Fprintf(&out, `\u%04x`, r)
			}
		} else {
			out.WriteRune(r)
		}
	}
	return out.String()
}
func (s Summary) Render() string {
	var out strings.Builder
	out.WriteString("# Handoff Summary\n\n")
	list := func(items []string) {
		for _, item := range items {
			out.WriteString("- " + item + "\n")
		}
	}
	if len(s.UserIntent)+len(s.PendingTasks) > 0 {
		out.WriteString("## Goal\n")
		list(s.UserIntent)
		if len(s.PendingTasks) > 0 {
			out.WriteString("\nOpen tasks:\n")
			list(s.PendingTasks)
		}
		out.WriteByte('\n')
	}
	if len(s.UserMessages) > 0 {
		out.WriteString("## Constraints\n")
		list(s.UserMessages)
		out.WriteByte('\n')
	}
	if s.CurrentWork != nil {
		out.WriteString("## State\n" + *s.CurrentWork + "\n\n")
	}
	if len(s.ProblemSolving) > 0 {
		out.WriteString("## Decisions\n")
		list(s.ProblemSolving)
		out.WriteByte('\n')
	}
	if s.NextStep != nil {
		out.WriteString("## Next Steps\n" + *s.NextStep + "\n\n")
	}
	if len(s.Files)+len(s.ErrorsAndFixes)+len(s.TechnicalConcepts) > 0 {
		out.WriteString("## Essential Context\n")
		for _, file := range s.Files {
			out.WriteString("- ")
			if file.Path != "" {
				out.WriteString("`" + file.Path + "`")
			}
			if file.Path != "" && file.Summary != "" {
				out.WriteString(": ")
			}
			out.WriteString(file.Summary + "\n")
			if file.KeyCode != nil {
				longest, run := 0, 0
				for _, r := range *file.KeyCode {
					if r == '`' {
						run++
						longest = max(longest, run)
					} else {
						run = 0
					}
				}
				fence := strings.Repeat("`", max(3, longest+1))
				out.WriteString(fence + "\n" + strings.TrimRight(*file.KeyCode, "\n") + "\n" + fence + "\n")
			}
		}
		list(s.ErrorsAndFixes)
		list(s.TechnicalConcepts)
	}
	return strings.TrimSpace(out.String())
}

// Bound cuts the least essential sections toward limit, retaining the last
// essential fields. Parse rejects a handoff that still cannot fit.
func (s *Summary) Bound(limit uint64) {
	fits := func() bool { return tokens.Text(s.Render()) <= limit }
	if fits() {
		return
	}
	// The droppable lists can be long, and counting the whole handoff after
	// every dropped entry is quadratic, so each is cut by bisection to the
	// same length that dropping entries one at a time arrives at.
	if cut(&s.TechnicalConcepts, fits) || cut(&s.ErrorsAndFixes, fits) || cut(&s.Files, fits) || cut(&s.ProblemSolving, fits) {
		return
	}
	for !fits() {
		if !s.trim() {
			return
		}
	}
}

// cut drops the fewest trailing entries of list that make the handoff fit
// and reports whether it then fits; a list that cannot make it fit is emptied.
func cut[T any](list *[]T, fits func() bool) bool {
	full := *list
	if len(full) == 0 {
		return false
	}
	*list = full[:0]
	if !fits() {
		return false
	}
	kept, over := 0, len(full)
	for over-kept > 1 {
		middle := (kept + over) / 2
		if *list = full[:middle]; fits() {
			kept = middle
		} else {
			over = middle
		}
	}
	*list = full[:kept]
	return true
}
func (s *Summary) trim() bool {
	for _, list := range []*[]string{&s.TechnicalConcepts, &s.ErrorsAndFixes} {
		if len(*list) > 0 {
			*list = (*list)[:len(*list)-1]
			return true
		}
	}
	if len(s.Files) > 0 {
		s.Files = s.Files[:len(s.Files)-1]
		return true
	}
	if len(s.ProblemSolving) > 0 {
		s.ProblemSolving = s.ProblemSolving[:len(s.ProblemSolving)-1]
		return true
	}
	if s.shorten(500) {
		return true
	}
	for _, list := range []*[]string{&s.PendingTasks, &s.UserMessages, &s.UserIntent} {
		if len(*list) > 1 {
			*list = (*list)[:len(*list)-1]
			return true
		}
	}
	return s.shorten(48)
}
func (s *Summary) shorten(minimum uint64) bool {
	var longest *string
	var size uint64
	for _, list := range []*[]string{&s.UserIntent, &s.UserMessages, &s.PendingTasks} {
		for i := range *list {
			if current := tokens.Text((*list)[i]); current >= size {
				size = current
				longest = &(*list)[i]
			}
		}
	}
	for _, text := range []*string{s.CurrentWork, s.NextStep} {
		if text != nil {
			if current := tokens.Text(*text); current >= size {
				size = current
				longest = text
			}
		}
	}
	if longest == nil || size <= minimum {
		return false
	}
	characters := []rune(*longest)
	keep := max(1, uint64(len(characters))*max(size/2, minimum)/size)
	*longest = strings.TrimRight(string(characters[:keep]), "…") + "…"
	return true
}
