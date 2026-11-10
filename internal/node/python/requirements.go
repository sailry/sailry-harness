package python

import (
	"crypto/sha256"
	"encoding/hex"
	"strings"
	"unicode/utf8"

	"github.com/sailry/sailry-harness/internal/protocol"
)

type Requirements struct {
	Package protocol.PluginReference
	Content string
}

func NewRequirements(reference protocol.PluginReference, content string) (*Requirements, error) {
	if reference.Name == "" || len(reference.Digest) != 64 || len(content) > 512*1024 || strings.ContainsRune(content, '\x00') || !utf8.ValidString(content) {
		return nil, invalid("invalid captured Python requirements")
	}
	if _, err := hex.DecodeString(reference.Digest); err != nil {
		return nil, invalid("invalid captured Python requirements")
	}
	for _, line := range strings.Split(content, "\n") {
		line = strings.TrimSpace(line)
		if strings.HasPrefix(line, "-") && !strings.HasPrefix(line, "--hash=sha256:") {
			return nil, invalid("Python requirements cannot contain installer options or file includes")
		}
		if !strings.HasPrefix(line, "#") {
			for _, part := range strings.Fields(line) {
				if strings.HasPrefix(part, "--") && !strings.HasPrefix(part, "--hash=sha256:") || strings.HasPrefix(part, "file:") || strings.HasPrefix(part, "git+") {
					return nil, invalid("Python requirements must use hashed, prebuilt packages")
				}
			}
		}
	}
	return &Requirements{Package: reference, Content: content}, nil
}

func (requirements Requirements) Key(identity string) string {
	content := sha256.Sum256([]byte(requirements.Content))
	digest := sha256.New()
	_, _ = digest.Write([]byte(identity))
	_, _ = digest.Write(content[:])
	return hex.EncodeToString(digest.Sum(nil))
}
