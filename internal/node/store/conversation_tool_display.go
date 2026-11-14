package store

import (
	"encoding/hex"
	"encoding/json"
	"reflect"
	"slices"
	"strconv"
	"strings"
	"unicode"
	"unicode/utf8"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func agentReference(ref protocol.ToolDisplayProjectionReference, args, result, item any) any {
	switch ref.Source {
	case protocol.ToolDisplayProjectionSourceArguments:
		return agentPointer(args, ref.Path)
	case protocol.ToolDisplayProjectionSourceResult:
		return agentPointer(result, ref.Path)
	case protocol.ToolDisplayProjectionSourceItem:
		return agentPointer(item, ref.Path)
	}
	return nil
}
func agentCondition(condition *protocol.ToolDisplayProjectionCondition, args, result, item any) bool {
	if condition == nil {
		return true
	}
	matches := func(test protocol.ToolDisplayProjectionComparison) bool {
		actual := agentReference(test.Value, args, result, item)
		want := agentJSON(test.Equals)
		matched := reflect.DeepEqual(actual, want)
		if test.Item != nil {
			matched = false
			if values, ok := actual.([]any); ok {
				for _, v := range values {
					if reflect.DeepEqual(agentPointer(v, *test.Item), want) {
						matched = true
						break
					}
				}
			}
		}
		return matched != test.Negate
	}
	any := len(condition.Any) == 0
	for _, test := range condition.Any {
		any = any || matches(test)
	}
	if !any {
		return false
	}
	for _, test := range condition.All {
		if !matches(test) {
			return false
		}
	}
	return true
}
func agentTextDeferred(text protocol.ToolDisplayProjectionText) bool {
	for _, part := range text.Parts {
		if part.Format != nil || part.Rows != nil && agentTextDeferred(part.Rows.Text) {
			return true
		}
	}
	return false
}
func agentTextVisible(text protocol.ToolDisplayProjectionText, args, result, item any) bool {
	for _, p := range text.Parts {
		if !agentCondition(p.When, args, result, item) {
			continue
		}
		if p.Text != nil && *p.Text != "" {
			return true
		}
		if p.Value != nil {
			v := agentReference(*p.Value, args, result, item)
			if p.Format != nil && v != nil {
				return true
			}
			if t := agentLiteral(v, false); t != nil && *t != "" {
				return true
			}
		}
		if p.Rows != nil {
			if values, ok := agentReference(p.Rows.Value, args, result, item).([]any); ok {
				for _, v := range values {
					if agentTextVisible(p.Rows.Text, args, result, v) {
						return true
					}
				}
			}
		}
	}
	return false
}
func agentText(text protocol.ToolDisplayProjectionText, args, result, item any) string {
	parts := []string{}
	for _, p := range text.Parts {
		if !agentCondition(p.When, args, result, item) {
			continue
		}
		if p.Text != nil {
			parts = append(parts, *p.Text)
			continue
		}
		if p.Value != nil {
			v := agentReference(*p.Value, args, result, item)
			var text *string
			if p.Format != nil {
				if v != nil {
					b, e := json.MarshalIndent(v, "", "  ")
					if e == nil {
						s := string(b)
						text = &s
					}
				}
			} else {
				text = agentLiteral(v, false)
			}
			if text != nil {
				if p.Take != nil {
					runes := []rune(*text)
					parts = append(parts, string(runes[:min(uint64(len(runes)), *p.Take)]))
				} else {
					parts = append(parts, *text)
				}
			}
			continue
		}
		if p.Rows != nil {
			if values, ok := agentReference(p.Rows.Value, args, result, item).([]any); ok && len(values) > 0 {
				rows := []string{}
				for _, v := range values {
					rows = append(rows, agentText(p.Rows.Text, args, result, v))
				}
				parts = append(parts, strings.Join(rows, p.Rows.Separator))
			}
		}
	}
	return strings.Join(parts, text.Separator)
}
func agentLocal(n protocol.PluginDesktopNavigation) protocol.ToolResultLocalText {
	return protocol.ToolResultLocalText{Label: n.Label, Locales: n.Locales}
}
func agentPrompt(p protocol.ToolDisplayProjectionPrompt, args any) *protocol.ToolResultLocalText {
	if !agentCondition(p.When, args, nil, nil) {
		return nil
	}
	render := func(template string) string {
		var out strings.Builder
		for {
			start := strings.Index(template, "%{")
			if start < 0 {
				out.WriteString(template)
				return out.String()
			}
			out.WriteString(template[:start])
			token := template[start+2:]
			end := strings.IndexByte(token, '}')
			if end < 0 {
				out.WriteString(template[start:])
				return out.String()
			}
			ref, ok := p.Values[token[:end]]
			text := (*string)(nil)
			if ok {
				text = agentLiteral(agentReference(ref, args, nil, nil), false)
			}
			if text != nil {
				out.WriteString(*text)
			} else {
				out.WriteString(template[start : start+end+3])
			}
			template = token[end+1:]
		}
	}
	value := protocol.ToolResultLocalText{Label: render(p.Message.Label), Locales: map[string]string{}}
	for locale, text := range p.Message.Locales {
		value.Locales[locale] = render(text)
	}
	return &value
}
func agentField(field *protocol.ToolDisplayField, args any) *protocol.ToolDetailText {
	if field == nil {
		return nil
	}
	text := agentString(agentPointer(args, field.Path))
	if text == nil || strings.TrimSpace(*text) == "" || slices.Contains(field.Omit, *text) {
		return nil
	}
	return &protocol.ToolDetailText{Text: *text, Code: field.Code}
}
func agentCommandAliases(command string, aliases []protocol.ToolDisplayAlias) string {
	candidates := slices.Clone(aliases)
	slices.SortStableFunc(candidates, func(a, b protocol.ToolDisplayAlias) int { return len(b.Path) - len(a.Path) })
	delimiter := func(c rune) bool { return unicode.IsSpace(c) || strings.ContainsRune("'\"=;|&()<>", c) }
	var out strings.Builder
	var previous rune
	for offset := 0; offset < len(command); {
		matched := false
		for _, alias := range candidates {
			if alias.Path == "" || !strings.HasPrefix(command[offset:], alias.Path) || offset > 0 && !delimiter(previous) {
				continue
			}
			suffix := command[offset+len(alias.Path):]
			next, _ := utf8.DecodeRuneInString(suffix)
			if suffix != "" && !delimiter(next) && !(alias.Kind == protocol.ToolDisplayAliasKindHome && (next == '/' || next == '\\')) {
				continue
			}
			replacement := "~"
			if alias.Kind == protocol.ToolDisplayAliasKindPython {
				replacement = "python"
			}
			out.WriteString(replacement)
			offset += len(alias.Path)
			previous, _ = utf8.DecodeLastRuneInString(alias.Path)
			matched = true
			break
		}
		if !matched {
			c, width := utf8.DecodeRuneInString(command[offset:])
			out.WriteRune(c)
			offset += width
			previous = c
		}
	}
	return out.String()
}
func (raw *rawAgentTool) resolved(full bool) *protocol.ToolDetailResolved {
	args, result := agentJSON(raw.Arguments), raw.value()
	metadata := agentResultDisplay(result)
	display := raw.Display
	if display == nil && metadata == nil {
		return nil
	}
	resolved := protocol.DefaultToolDetailResolved()
	if metadata != nil {
		resolved.Status = metadata.Status
		resolved.Diagnostics = metadata.Diagnostics
	}
	if display == nil {
		return &resolved
	}
	resolved.Label = &protocol.ToolResultLocalText{Label: display.Label, Locales: display.Locales}
	if input := display.Input; input != nil {
		resolved.Input.Summary = agentField(input.Summary, args)
		resolved.Input.Context = agentField(input.Context, args)
		resolved.Input.Target = agentField(input.Target, args)
		resolved.Input.Content = agentField(input.Content, args)
		if field := input.Summary; field != nil {
			var name *string
			if field.ResultPath != nil {
				name = agentString(agentPointer(result, *field.ResultPath))
				if name != nil && strings.TrimSpace(*name) == "" {
					name = nil
				}
			}
			if name != nil {
				resolved.Input.Summary = &protocol.ToolDetailText{Text: *name, Code: field.Code}
			}
			if summary := resolved.Input.Summary; summary != nil && field.Format != nil {
				switch *field.Format {
				case protocol.ToolDisplayInputFormatCommand:
					summary.Text = agentCommandAliases(summary.Text, display.Aliases)
					body, _ := json.Marshal(result)
					var output protocol.Output
					if json.Unmarshal(body, &output) == nil && output.Kind == "command_result" && raw.Presentation == protocol.ToolPresentationContent && raw.content() != nil {
						if completed, ok := output.Data.(*protocol.ProcessCompletion); ok {
							resolved.Command = &completed.Outcome
						}
					}
				case protocol.ToolDisplayInputFormatSession:
					if name == nil {
						if text, ok := field.Names[summary.Text]; ok && strings.TrimSpace(text) != "" {
							summary.Text = text
						} else {
							resolved.Input.Summary = nil
						}
					}
				case protocol.ToolDisplayInputFormatConnection:
					if name == nil {
						if text, ok := field.Names[summary.Text]; ok && strings.TrimSpace(text) != "" {
							summary.Text = text
						}
					}
				}
			}
		}
	}
	for _, prompt := range display.Approval {
		if v := agentPrompt(prompt, args); v != nil {
			resolved.Approval = append(resolved.Approval, *v)
		}
	}
	// Summary rows carry input and status, not result previews. Explicit detail
	// requests still resolve the complete declared output.
	if recipe := display.Output; recipe != nil && (full || raw.Presentation != protocol.ToolPresentationSummary && raw.Presentation != protocol.ToolPresentationHidden) {
		out := protocol.DefaultToolDetailOutput()
		if recipe.Preview != nil {
			if full || !agentTextDeferred(*recipe.Preview) {
				out.Preview = agentText(*recipe.Preview, args, result, nil)
			} else {
				out.PreviewDeferred = agentTextVisible(*recipe.Preview, args, result, nil)
			}
		}
		for _, notice := range recipe.Notices {
			if agentCondition(notice.When, args, result, nil) {
				out.Notices = append(out.Notices, agentLocal(notice.Message))
			}
		}
		if paths := recipe.Paths; paths != nil {
			values := []any{agentReference(paths.Value, args, result, nil)}
			if paths.Item != nil {
				values = nil
				if items, ok := agentReference(paths.Value, args, result, nil).([]any); ok {
					for _, v := range items {
						values = append(values, agentPointer(v, *paths.Item))
					}
				}
			}
			seen := map[string]bool{}
			for _, v := range values {
				path := agentString(v)
				if path == nil || *path == "" || seen[*path] {
					continue
				}
				seen[*path] = true
				if full || len(out.Paths) < 32 {
					out.Paths = append(out.Paths, *path)
				}
			}
			out.PathCount = uint64(len(seen))
		}
		if full {
			if body := recipe.Body; body != nil {
				if text := agentString(agentReference(body.Text, args, result, nil)); text != nil {
					b := protocol.ToolDetailBody{Text: *text}
					if body.Path != nil {
						b.Path = agentString(agentReference(*body.Path, args, result, nil))
						if b.Path != nil && *b.Path == "" {
							b.Path = nil
						}
					}
					if body.Diff != nil && body.Diff.Format != protocol.ToolDisplayProjectionFormatCheckpoint && agentCondition(body.Diff.When, args, result, nil) {
						b.Diff = &body.Diff.Format
					}
					if body.Empty != nil {
						v := agentLocal(*body.Empty)
						b.Empty = &v
					}
					out.Body = &b
				}
			}
			if recipe.Table != nil {
				out.Table = agentTable(*recipe.Table, args, result)
			}
		}
		resolved.Output = &out
	}
	return &resolved
}
func agentTable(recipe protocol.ToolDisplayProjectionTable, args, result any) *protocol.ToolContentTable {
	value := protocol.DefaultToolContentTable()
	columns, ok := agentReference(recipe.Columns, args, result, nil).([]any)
	if !ok || len(columns) == 0 || len(columns) > 64 {
		return nil
	}
	for _, v := range columns {
		text := agentString(v)
		if text == nil || len(*text) > 1024 {
			return nil
		}
		value.Columns = append(value.Columns, *text)
	}
	rows, ok := agentReference(recipe.Rows, args, result, nil).([]any)
	if !ok || len(rows) > 1000 {
		return nil
	}
	for _, row := range rows {
		cells, ok := row.([]any)
		if !ok || len(cells) != len(columns) {
			return nil
		}
		line := []string{}
		for _, cell := range cells {
			v := agentPointer(cell, recipe.Cell.Value)
			text := agentLiteral(v, false)
			if v == nil {
				t := recipe.Cell.Missing
				text = &t
			}
			if text == nil && recipe.Cell.Bytes != nil {
				if items, ok := v.([]any); ok {
					data := []byte{}
					for _, item := range items {
						n, ok := item.(json.Number)
						if !ok {
							return nil
						}
						b, e := strconv.ParseUint(string(n), 10, 8)
						if e != nil {
							return nil
						}
						data = append(data, byte(b))
					}
					t := hex.EncodeToString(data)
					text = &t
				}
			}
			if text == nil || len(*text) > 16*1024 {
				return nil
			}
			line = append(line, *text)
		}
		value.Rows = append(value.Rows, line)
	}
	if recipe.Truncated != nil {
		value.Truncated = agentReference(*recipe.Truncated, args, result, nil) == true
	}
	return &value
}
func boundAgentResolved(value *protocol.ToolDetailResolved) {
	boundLocal := func(v *protocol.ToolResultLocalText, limit int) {
		if v == nil {
			return
		}
		v.Label = agentClip(v.Label, limit, true)
		out := map[string]string{}
		for locale, text := range v.Locales {
			out[agentClip(locale, 32, true)] = agentClip(text, limit, true)
		}
		v.Locales = out
	}
	boundLocal(value.Label, 200)
	for _, field := range []*protocol.ToolDetailText{value.Input.Summary, value.Input.Context, value.Input.Content, value.Input.Target} {
		if field != nil {
			field.Text = agentClip(field.Text, 200, true)
		}
	}
	value.Approval = value.Approval[:min(4, len(value.Approval))]
	for i := range value.Approval {
		boundLocal(&value.Approval[i], 1024)
	}
	boundLocal(value.Status, 200)
	value.Diagnostics = value.Diagnostics[:min(4, len(value.Diagnostics))]
	for i := range value.Diagnostics {
		value.Diagnostics[i].Text = agentClip(value.Diagnostics[i].Text, 256, false)
	}
	if out := value.Output; out != nil {
		out.Preview = agentClip(out.Preview, 2048, false)
		paths := []string{}
		for _, path := range out.Paths {
			if strings.ContainsAny(path, "\r\n") {
				continue
			}
			paths = append(paths, agentClip(path, 200, true))
			if len(paths) == 32 {
				break
			}
		}
		out.Paths = paths
		out.Notices = out.Notices[:min(4, len(out.Notices))]
		for i := range out.Notices {
			boundLocal(&out.Notices[i], 200)
		}
		out.Body = nil
		out.Table = nil
	}
}
