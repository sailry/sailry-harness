package client

import (
	"context"
	"encoding/json"
	"html"
	"net/url"
	"slices"
	"sort"
	"strconv"
	"strings"
	"unicode"
	"unicode/utf8"

	"github.com/sailry/sailry-harness/internal/protocol"
)

const maxExportBytes = 64 * 1024 * 1024

type ExportFormat string

const (
	ExportMarkdown ExportFormat = "markdown"
	ExportHTML     ExportFormat = "html"
)

type ExportDocument struct {
	Content     string
	Attachments []protocol.Attachment
	Images      []protocol.ConversationImage
}

func (c *Client) ExportConversation(ctx context.Context, session protocol.SessionID, title string, format ExportFormat, assets string) (ExportDocument, error) {
	return c.exportSelection(ctx, session, nil, title, format, assets)
}
func (c *Client) ExportReply(ctx context.Context, session protocol.SessionID, turn protocol.TurnID, title string, format ExportFormat, assets string) (ExportDocument, error) {
	return c.exportSelection(ctx, session, &turn, title, format, assets)
}
func (c *Client) exportSelection(ctx context.Context, session protocol.SessionID, turn *protocol.TurnID, title string, format ExportFormat, assets string) (ExportDocument, error) {
	if err := ctx.Err(); err != nil {
		return ExportDocument{}, exportCancelled()
	}
	history, err := c.ReadConversation(ctx, session, nil)
	if err != nil {
		if ctx.Err() != nil {
			return ExportDocument{}, exportCancelled()
		}
		return ExportDocument{}, err
	}
	page := history.Page
	bytes := 0
	count := func(value any) error {
		body, e := json.Marshal(value)
		if e != nil {
			return e
		}
		bytes += len(body)
		if bytes > maxExportBytes {
			return exportTooLarge()
		}
		return nil
	}
	if err = count(page); err != nil {
		return ExportDocument{}, err
	}
	hydrate := func(h protocol.ConversationHistory) error {
		for _, id := range h.Missing {
			if turn != nil && id != *turn {
				continue
			}
			var before *uint64
			for _, entry := range page.Entries {
				if entry.Turn == id && (before == nil || entry.Sequence < *before) {
					n := entry.Sequence
					before = &n
				}
			}
			seen := map[uint64]bool{}
			for {
				if ctx.Err() != nil {
					return exportCancelled()
				}
				older, e := c.readConversationTurn(ctx, session, id, page.Revision, before)
				if e != nil {
					if ctx.Err() != nil {
						return exportCancelled()
					}
					return e
				}
				if e = count(older); e != nil {
					return e
				}
				incoming := protocol.ConversationPage{Session: session, Revision: older.Revision, Entries: older.Entries, Runs: []protocol.ConversationRun{older.Run}, Approvals: older.Approvals, Questions: older.Questions, Children: older.Children}
				if e = mergeConversationPage(&page, incoming, true, nil); e != nil {
					return e
				}
				if older.NextBefore == nil {
					break
				}
				if seen[*older.NextBefore] || before != nil && *older.NextBefore >= *before {
					return fault(protocol.ErrorCodeInvalidRequest, "conversation export cursor did not advance")
				}
				seen[*older.NextBefore] = true
				before = older.NextBefore
			}
		}
		return nil
	}
	if err = hydrate(history); err != nil {
		return ExportDocument{}, err
	}
	seen := map[protocol.TurnID]bool{}
	for page.NextBefore != nil {
		if turn != nil && slices.ContainsFunc(page.Entries, func(e protocol.ConversationEntry) bool { return e.Turn == *turn }) {
			break
		}
		cursor := *page.NextBefore
		if seen[cursor] {
			return ExportDocument{}, fault(protocol.ErrorCodeInvalidRequest, "conversation export cursor did not advance")
		}
		seen[cursor] = true
		if ctx.Err() != nil {
			return ExportDocument{}, exportCancelled()
		}
		older, e := c.ReadConversation(ctx, session, &cursor)
		if e != nil {
			if ctx.Err() != nil {
				return ExportDocument{}, exportCancelled()
			}
			return ExportDocument{}, e
		}
		if e = count(older.Page); e != nil {
			return ExportDocument{}, e
		}
		next := older.Page.NextBefore
		if e = mergeConversationPage(&page, older.Page, true, nil); e != nil {
			return ExportDocument{}, e
		}
		page.NextBefore = next
		if e = hydrate(older); e != nil {
			return ExportDocument{}, e
		}
	}
	referenced := exportImages(page)
	if turn != nil {
		page.Entries = slices.DeleteFunc(page.Entries, func(e protocol.ConversationEntry) bool { return e.Turn != *turn })
		if len(page.Entries) == 0 {
			return ExportDocument{}, fault(protocol.ErrorCodeInvalidRequest, "reply is unavailable")
		}
	}
	return composeExport(page, referenced, title, format, assets)
}
func exportCancelled() error {
	return fault(protocol.ErrorCodeCancelled, "conversation export was cancelled")
}
func exportTooLarge() error {
	return fault(protocol.ErrorCodeInvalidRequest, "conversation export exceeds 64 MiB")
}
func ExportAttachmentName(attachment protocol.Attachment) string {
	var out strings.Builder
	count := 0
	for _, c := range attachment.Spec.Name {
		if count >= 100 {
			break
		}
		count++
		if c < 128 && (c >= 'a' && c <= 'z' || c >= 'A' && c <= 'Z' || c >= '0' && c <= '9' || strings.ContainsRune(".-_", c)) {
			out.WriteRune(c)
		} else {
			out.WriteByte('_')
		}
	}
	name := out.String()
	if name == "" {
		name = "attachment"
	}
	return attachment.ID.String() + "-" + name
}
func exportLabel(text string) string {
	text = strings.Join(strings.Split(strings.ReplaceAll(text, "\r\n", "\n"), "\n"), " ")
	return strings.NewReplacer("\\", "\\\\", "[", "\\[", "]", "\\]", "<", "\\<", ">", "\\>").Replace(text)
}
func safeExportURL(text string) bool {
	u, e := url.Parse(text)
	return e == nil && u.Host != "" && (u.Scheme == "http" || u.Scheme == "https") && u.User == nil
}

// workPart marks the parts before a turn's final reply; closing interactions
// leave the preceding prose visible. Exports and transcripts share it.
func workPart(part protocol.ConversationPart) bool {
	switch part.Kind {
	case "text", "image", "attachment", "reference":
		return false
	case "tool_call":
		v, e := payload[protocol.ConversationPartToolCallPayload](part.Data)
		return e != nil || !terminalInteraction(v.Name)
	case "tool_result":
		v, e := payload[protocol.ConversationPartToolResultPayload](part.Data)
		return e != nil || !terminalInteraction(v.Name)
	case "resource":
		var v map[string]json.RawMessage
		body, e := json.Marshal(part.Data)
		if e == nil {
			e = json.Unmarshal(body, &v)
		}
		if e == nil {
			var kind string
			_ = json.Unmarshal(v["type"], &kind)
			return kind != "tool_started" && kind != "provider_context" && kind != "output_limit"
		}
	}
	return true
}

type exportImageKey struct {
	call  string
	index uint64
}

func exportImages(page protocol.ConversationPage) map[exportImageKey]protocol.ConversationImage {
	images := map[exportImageKey]protocol.ConversationImage{}
	for _, entry := range page.Entries {
		for _, part := range entry.Parts {
			if part.Kind == "tool_result" {
				v, e := payload[protocol.ConversationPartToolResultPayload](part.Data)
				if e == nil && v.ID != nil {
					for _, image := range v.Images {
						images[exportImageKey{*v.ID, image.Index}] = image
					}
				}
			}
		}
	}
	return images
}

type exportImageSpan struct {
	start, end int
	key        exportImageKey
}

func exportImageSpans(text string) []exportImageSpan {
	spans := []exportImageSpan{}
	resume := 0
	for colon := 0; colon < len(text); colon++ {
		if text[colon] != ':' {
			continue
		}
		start := colon - len("sailry-image")
		if start < resume || start < 0 || !strings.EqualFold(text[start:colon], "sailry-image") {
			continue
		}
		end := colon
		for end < len(text) {
			c, w := utf8.DecodeRuneInString(text[end:])
			if unicode.IsSpace(c) || strings.ContainsRune("()< >[]\"'", c) {
				break
			}
			end += w
		}
		resume = end
		raw := text[colon+1 : end]
		slash := strings.LastIndexByte(raw, '/')
		if slash < 1 {
			continue
		}
		call, e := url.PathUnescape(raw[:slash])
		if e != nil || call == "" || len(call) > 512 || !utf8.ValidString(call) {
			continue
		}
		number := raw[slash+1:]
		if number == "" || len(number) > 1 && number[0] == '0' {
			continue
		}
		valid := true
		for _, c := range number {
			valid = valid && c >= '0' && c <= '9'
		}
		index, e := strconv.ParseUint(number, 10, 64)
		if e != nil || !valid {
			continue
		}
		spans = append(spans, exportImageSpan{start, end, exportImageKey{call, index}})
	}
	return spans
}
func composeExport(page protocol.ConversationPage, referenced map[exportImageKey]protocol.ConversationImage, title string, format ExportFormat, assets string) (ExportDocument, error) {
	result := ExportDocument{Attachments: []protocol.Attachment{}, Images: []protocol.ConversationImage{}}
	if format != ExportMarkdown && format != ExportHTML {
		return result, fault(protocol.ErrorCodeInvalidRequest, "export format is invalid")
	}
	if assets == "" || strings.IndexFunc(assets, unicode.IsControl) >= 0 || strings.ContainsAny(assets, "\\:<>") {
		return result, fault(protocol.ErrorCodeInvalidRequest, "export attachment directory must be a relative path")
	}
	for _, part := range strings.Split(assets, "/") {
		if part == "" || part == "." || part == ".." {
			return result, fault(protocol.ErrorCodeInvalidRequest, "export attachment directory must be a relative path")
		}
	}
	attachments := map[protocol.AttachmentID]protocol.Attachment{}
	images := map[protocol.AttachmentID]protocol.ConversationImage{}
	add := func(attachment protocol.Attachment) error {
		if previous, ok := attachments[attachment.ID]; ok && previous != attachment {
			return fault(protocol.ErrorCodeInvalidRequest, "export attachment identity changed")
		}
		attachments[attachment.ID] = attachment
		return nil
	}
	type boundary struct {
		sequence uint64
		part     int
	}
	boundaries := map[protocol.TurnID]boundary{}
	conversation := false
	for _, entry := range page.Entries {
		conversation = conversation || entry.Author == "user"
		if entry.Author != "user" {
			for i, part := range entry.Parts {
				if workPart(part) {
					boundaries[entry.Turn] = boundary{entry.Sequence, i}
				}
			}
		}
	}
	var markdown, messages strings.Builder
	for _, entry := range page.Entries {
		user := entry.Author == "user"
		if user && slices.ContainsFunc(page.Runs, func(r protocol.ConversationRun) bool {
			return r.Turn == entry.Turn && (r.Continuation != nil || r.Notice)
		}) {
			continue
		}
		var body strings.Builder
		textRun := false
		finish := func() {
			if textRun {
				body.WriteString("\n\n")
				textRun = false
			}
		}
		for i, part := range entry.Parts {
			if b, ok := boundaries[entry.Turn]; !user && ok && (entry.Sequence < b.sequence || entry.Sequence == b.sequence && i <= b.part) {
				continue
			}
			switch part.Kind {
			case "text":
				text, e := payload[string](part.Data)
				if e != nil {
					return result, e
				}
				cursor := 0
				// Only assistant Markdown names conversation images; what the
				// user typed stays as typed.
				var spans []exportImageSpan
				if !user {
					spans = exportImageSpans(text)
				}
				for _, span := range spans {
					image, ok := referenced[span.key]
					if !ok {
						continue
					}
					if e = add(image.Attachment); e != nil {
						return result, e
					}
					images[image.Attachment.ID] = image
					body.WriteString(text[cursor:span.start])
					body.WriteString(assets + "/" + ExportAttachmentName(image.Attachment))
					cursor = span.end
				}
				body.WriteString(text[cursor:])
				textRun = true
			case "attachment", "image":
				finish()
				var attachment protocol.Attachment
				if part.Kind == "image" {
					image, e := payload[protocol.ConversationImage](part.Data)
					if e != nil {
						return result, e
					}
					attachment = image.Attachment
					images[attachment.ID] = image
					body.WriteByte('!')
				} else {
					v, e := payload[protocol.Attachment](part.Data)
					if e != nil {
						return result, e
					}
					attachment = v
				}
				if e := add(attachment); e != nil {
					return result, e
				}
				body.WriteString("[" + exportLabel(attachment.Spec.Name) + "](<" + assets + "/" + ExportAttachmentName(attachment) + ">)\n\n")
			default:
				finish()
			}
		}
		finish()
		if body.Len() == 0 {
			continue
		}
		if conversation {
			markdown.WriteString("## " + strings.Map(func(c rune) rune {
				if strings.ContainsRune("#<>\r\n", c) {
					return -1
				}
				return c
			}, entry.Author) + "\n\n")
		}
		for _, citation := range entry.Citations {
			if safeExportURL(citation.Uri) {
				label := citation.Uri
				if citation.Title != nil {
					label = *citation.Title
				}
				body.WriteString("[" + exportLabel(label) + "](<" + strings.NewReplacer("<", "", ">", "").Replace(citation.Uri) + ">)\n\n")
			}
		}
		markdown.WriteString(body.String())
		if format == ExportHTML {
			rendered, e := exportHTMLBody(body.String())
			if e != nil {
				return result, e
			}
			class := "assistant"
			if user {
				class = "user"
			}
			messages.WriteString(`<section class="message ` + class + `">` + rendered + `</section>`)
		}
		if markdown.Len() > maxExportBytes || messages.Len() > maxExportBytes {
			return result, exportTooLarge()
		}
	}
	result.Content = markdown.String()
	if format == ExportHTML {
		result.Content = exportHTML(title, messages.String())
	}
	if len(result.Content) > maxExportBytes {
		return result, exportTooLarge()
	}
	ids := []protocol.AttachmentID{}
	for id := range attachments {
		ids = append(ids, id)
	}
	sort.Slice(ids, func(i, j int) bool { return ids[i].String() < ids[j].String() })
	for _, id := range ids {
		result.Attachments = append(result.Attachments, attachments[id])
		if image, ok := images[id]; ok {
			result.Images = append(result.Images, image)
		}
	}
	return result, nil
}
func exportHTML(title, body string) string {
	return `<!doctype html><html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>` + html.EscapeString(title) + `</title><style>body{max-width:900px;margin:3rem auto;padding:0 1rem;font:16px/1.6 system-ui}.message{margin:24px 0;overflow-wrap:anywhere}.user{width:fit-content;max-width:85%;margin-left:auto;padding:10px 18px;border-radius:16px;background:#292929;color:#fff}.user>:first-child{margin-top:0}.user>:last-child{margin-bottom:0}.user a{color:inherit}.user pre{color:#20242b}pre{overflow:auto;padding:1rem;background:#f5f5f5}img{max-width:100%}table{border-collapse:collapse}td,th{padding:.5rem;border:1px solid #ccc}blockquote{margin-left:0;padding-left:1rem;border-left:3px solid #ccc}</style></head><body>` + body + `</body></html>`
}
