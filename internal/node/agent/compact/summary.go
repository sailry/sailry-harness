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
	seen := map[string]struct{}{}
	for i := len(cuts) - 1; i >= 0; i-- {
		tail := text[cuts[i]:]
		later := strings.Count(tail, "</analysis>")
		appendCandidate := func(candidate string) {
			if candidate == "" || strings.Count(candidate, "</analysis>") != later {
				return
			}
			if _, ok := seen[candidate]; ok {
				return
			}
			seen[candidate] = struct{}{}
			candidates = append(candidates, candidate)
		}
		for _, block := range fencedBlocks(tail) {
			appendCandidate(object(block))
		}
		appendCandidate(object(tail))
		appendCandidate(xmlWrappedObject(tail))
	}
	if candidate := object(text); candidate != "" {
		if _, ok := seen[candidate]; !ok {
			candidates = append(candidates, candidate)
		}
	}
	for _, candidate := range candidates {
		var fields map[string]any
		stripped := stripTrailingCommas(candidate)
		variants := []string{
			candidate,
			stripped,
			repair(candidate),
			escape(candidate),
			stripTrailingCommas(escape(candidate)),
			repair(escape(candidate)),
		}
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
		if !summary.nonEmpty() {
			continue
		}
		summary.Bound(limit)
		if tokens.Text(summary.Render()) > limit {
			continue
		}
		return summary, nil
	}
	if summary, ok := parseMarkdownHandoff(text[cuts[len(cuts)-1]:]); ok {
		summary.Bound(limit)
		if tokens.Text(summary.Render()) <= limit {
			return summary, nil
		}
	}
	return Summary{}, fmt.Errorf("compaction summary was not a structured handoff")
}

func (s Summary) nonEmpty() bool {
	return len(s.UserIntent)+len(s.TechnicalConcepts)+len(s.ErrorsAndFixes)+len(s.ProblemSolving)+len(s.UserMessages)+len(s.PendingTasks)+len(s.Files) > 0 || s.CurrentWork != nil || s.NextStep != nil
}

// fencedBlocks returns fenced code block bodies in reverse order so the final
// handoff block is preferred over any examples or quoted snippets above it.
func fencedBlocks(text string) []string {
	var blocks []string
	var width, offset, start int
	for line := range strings.SplitAfterSeq(text, "\n") {
		next := markdownFence(line, width)
		if next != width {
			if width == 0 {
				start = offset + len(line)
			} else {
				blocks = append(blocks, text[start:offset])
			}
			width = next
		}
		offset += len(line)
	}
	if width != 0 {
		blocks = append(blocks, text[start:])
	}
	slices.Reverse(blocks)
	return blocks
}

// A closer must be bare and at least as long as the opener. Shorter
// delimiters may themselves be part of the captured code.
func markdownFence(line string, width int) int {
	line = strings.TrimSpace(line)
	rest := strings.TrimLeft(line, "`")
	size := len(line) - len(rest)
	if size >= 3 {
		if width == 0 {
			return size
		}
		if size >= width && rest == "" {
			return 0
		}
	}
	return width
}

// xmlWrappedObject extracts a top-level JSON object wrapped in a single XML
// tag pair such as <handoff>{...}</handoff> or <summary>{...}</summary>.
func xmlWrappedObject(text string) string {
	trimmed := strings.TrimSpace(text)
	for _, tag := range []string{"handoff", "summary", "json"} {
		openTag, closeTag := "<"+tag+">", "</"+tag+">"
		if strings.HasPrefix(strings.ToLower(trimmed), openTag) && strings.HasSuffix(strings.ToLower(trimmed), closeTag) {
			inner := trimmed[len(openTag) : len(trimmed)-len(closeTag)]
			return object(inner)
		}
	}
	return ""
}

// stripTrailingCommas removes trailing commas before closing '}' or ']' tokens
// outside JSON string literals.
func stripTrailingCommas(text string) string {
	var out strings.Builder
	out.Grow(len(text))
	quoted, escaped := false, false
	for i := 0; i < len(text); i++ {
		ch := text[i]
		if quoted {
			out.WriteByte(ch)
			if escaped {
				escaped = false
			} else if ch == '\\' {
				escaped = true
			} else if ch == '"' {
				quoted = false
			}
			continue
		}
		if ch == '"' {
			quoted = true
			out.WriteByte(ch)
			continue
		}
		if ch == ',' {
			j := i + 1
			for j < len(text) && (text[j] == ' ' || text[j] == '\t' || text[j] == '\r' || text[j] == '\n') {
				j++
			}
			if j < len(text) && (text[j] == '}' || text[j] == ']') {
				continue
			}
		}
		out.WriteByte(ch)
	}
	return out.String()
}

// parseMarkdownHandoff recovers a structured handoff when a model continues the
// rendered Markdown "# Handoff Summary" format of an earlier compaction instead
// of emitting JSON. At least two recognized handoff section headings are required
// so ordinary prose replies are still rejected.
func parseMarkdownHandoff(text string) (Summary, bool) {
	lines := strings.Split(strings.ReplaceAll(text, "\r\n", "\n"), "\n")
	summary := Summary{Files: []File{}}
	section := ""
	subSection := ""
	matchedHeadings := 0
	var stateLines, nextLines []string
	fenceWidth := 0
	var codeLines []string
	appendItem := func(target *[]string, line string) {
		item := strings.TrimSpace(line)
		item = strings.TrimPrefix(item, "- ")
		item = strings.TrimPrefix(item, "* ")
		item = strings.TrimSpace(item)
		if item != "" {
			*target = append(*target, item)
		}
	}
	flushCode := func() {
		if len(codeLines) == 0 || len(summary.Files) == 0 {
			codeLines = nil
			return
		}
		code := strings.Join(codeLines, "\n")
		if strings.TrimSpace(code) != "" {
			summary.Files[len(summary.Files)-1].KeyCode = &code
		}
		codeLines = nil
	}
	for _, raw := range lines {
		trimmed := strings.TrimSpace(raw)
		if next := markdownFence(trimmed, fenceWidth); next != fenceWidth {
			fenceWidth = next
			if fenceWidth == 0 {
				flushCode()
			} else {
				codeLines = nil
			}
			continue
		}
		if fenceWidth != 0 {
			if section == "essential" && len(summary.Files) > 0 {
				codeLines = append(codeLines, raw)
			}
			continue
		}
		if strings.HasPrefix(trimmed, "## ") || strings.HasPrefix(trimmed, "### ") {
			heading := strings.ToLower(strings.TrimSpace(strings.TrimLeft(trimmed, "#")))
			subSection = ""
			switch heading {
			case "goal", "goals", "user intent":
				section = "goal"
				matchedHeadings++
				continue
			case "open tasks", "pending tasks":
				section = "pending"
				matchedHeadings++
				continue
			case "constraints", "user messages", "requirements":
				section = "constraints"
				matchedHeadings++
				continue
			case "state", "current work", "current state":
				section = "state"
				matchedHeadings++
				continue
			case "decisions", "problem solving":
				section = "decisions"
				matchedHeadings++
				continue
			case "next steps", "next step":
				section = "next"
				matchedHeadings++
				continue
			case "essential context", "files", "key files", "context":
				section = "essential"
				matchedHeadings++
				continue
			case "errors and fixes", "errors":
				section = "errors"
				matchedHeadings++
				continue
			default:
				if strings.HasPrefix(trimmed, "## ") {
					section = ""
				}
				continue
			}
		}
		if section == "" || trimmed == "" || strings.HasPrefix(trimmed, "# ") {
			continue
		}
		switch section {
		case "goal":
			if strings.EqualFold(trimmed, "open tasks:") || strings.EqualFold(trimmed, "pending tasks:") {
				subSection = "pending"
				continue
			}
			if subSection == "pending" {
				appendItem(&summary.PendingTasks, trimmed)
			} else {
				appendItem(&summary.UserIntent, trimmed)
			}
		case "pending":
			appendItem(&summary.PendingTasks, trimmed)
		case "constraints":
			appendItem(&summary.UserMessages, trimmed)
		case "state":
			stateLines = append(stateLines, trimmed)
		case "decisions":
			appendItem(&summary.ProblemSolving, trimmed)
		case "next":
			nextLines = append(nextLines, trimmed)
		case "errors":
			appendItem(&summary.ErrorsAndFixes, trimmed)
		case "essential":
			item := strings.TrimSpace(strings.TrimPrefix(strings.TrimPrefix(trimmed, "- "), "* "))
			if strings.HasPrefix(item, "`") {
				if end := strings.IndexByte(item[1:], '`'); end > 0 {
					path := strings.TrimSpace(item[1 : 1+end])
					rest := strings.TrimSpace(strings.TrimPrefix(strings.TrimSpace(item[2+end:]), ":"))
					if path != "" {
						summary.Files = append(summary.Files, File{Path: path, Summary: rest})
						continue
					}
				}
			}
			if item != "" {
				summary.TechnicalConcepts = append(summary.TechnicalConcepts, item)
			}
		}
	}
	if len(stateLines) > 0 {
		joined := strings.Join(stateLines, "\n")
		summary.CurrentWork = &joined
	}
	if len(nextLines) > 0 {
		joined := strings.Join(nextLines, "\n")
		summary.NextStep = &joined
	}
	if matchedHeadings < 2 || !summary.nonEmpty() {
		return Summary{}, false
	}
	return summary, true
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
