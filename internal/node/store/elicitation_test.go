package store

import (
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func TestMCPAuthorizationLink(t *testing.T) {
	t.Parallel()
	for link, allowed := range map[string]bool{
		"https://example.invalid/continue?task=fixture": true,
		"http://localhost:8080/callback":                true,
		"http://LOCALHOST/callback":                     true,
		"http://127.0.0.1:8080/callback":                true,
		"http://127.8.9.1/callback":                     true,
		"http://[::1]:8080/callback":                    true,
		"http://example.invalid/continue":               false,
		"http://192.168.1.10/continue":                  false,
		"http://localhost.example.invalid/continue":     false,
		"http://127.0.0.1.example.invalid/continue":     false,
		"https://user@example.invalid/continue":         false,
		"https://user:secret@example.invalid/continue":  false,
		"http://localhost@example.invalid/continue":     false,
		"https://example.invalid/continue\u0085":        false,
		"https:///continue":                             false,
		"file:///etc/passwd":                            false,
		"javascript:alert(1)":                           false,
		"example.invalid/continue":                      false,
	} {
		spec := protocol.ConversationQuestionSpec{Prompt: "Authorize", Input: protocol.ConversationQuestionInput{Kind: "url", Data: &protocol.ConversationQuestionInputUrlPayload{Url: link, ElicitationID: "authorization"}}}
		if err := validateMCPSpec(spec); (err == nil) != allowed {
			t.Errorf("%q: allowed=%v, %v", link, allowed, err)
		}
	}
}
