package store

import (
	"math"
	"net/url"
	"strings"
	"unicode/utf8"

	"github.com/sailry/sailry-harness/internal/node/agent/canonical"
	"github.com/sailry/sailry-harness/internal/protocol"
)

func projectSources(message *canonical.Message) []protocol.ConversationCitation {
	length := 0
	for _, block := range message.Content {
		if text, ok := block.Data.(*canonical.TextContent); ok {
			length += utf8.RuneCountInString(text.Text)
		}
	}
	var results []protocol.ConversationCitation
	canonicalURL := func(value string) (string, bool) {
		parsed, err := url.Parse(value)
		if err != nil || parsed.Hostname() == "" || !strings.EqualFold(parsed.Scheme, "http") && !strings.EqualFold(parsed.Scheme, "https") {
			return "", false
		}
		return parsed.String(), true
	}
	for _, source := range message.Sources.Citations {
		if source.URI == nil {
			continue
		}
		uri, valid := canonicalURL(*source.URI)
		if !valid {
			continue
		}
		citation := protocol.ConversationCitation{Uri: uri, Title: source.Title}
		if source.Start != nil && source.End != nil && *source.Start >= 0 && *source.End >= *source.Start && *source.End <= int64(length) && *source.End <= math.MaxUint32 {
			start, end := uint32(*source.Start), uint32(*source.End)
			citation.Start, citation.End = &start, &end
		}
		results = append(results, citation)
	}
	for _, source := range message.Sources.Grounding {
		if uri, valid := canonicalURL(source.URI); valid {
			results = append(results, protocol.ConversationCitation{Uri: uri, Title: source.Title})
		}
	}
	return results
}
