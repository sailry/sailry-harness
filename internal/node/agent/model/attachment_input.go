package model

import (
	"strings"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// AttachmentFileMIME selects the wire MIME for a byte-detected chat attachment.
// An adapter's generic file part or a gateway's fallback encoder does not
// establish that its downstream API understands that media.
func AttachmentFileMIME(binding Binding, mime string) string {
	provider := binding.Provider
	if PluginBound(provider) {
		return ""
	}
	selected := metadata(binding)
	if mime == "application/pdf" {
		if selected != nil && selected.Inputs != nil && !selected.AcceptsInput("pdf") {
			return ""
		}
	} else {
		kind, _, _ := strings.Cut(mime, "/")
		if (kind != "audio" && kind != "video") || selected == nil || !selected.AcceptsInput(kind) {
			return ""
		}
	}
	keyed := provider.Authentication == protocol.AuthenticationApiKey
	switch provider.Api.Family() {
	case protocol.ConversationModelApiGemini:
		if keyed {
			switch mime {
			case "application/pdf", "audio/wav", "audio/aiff", "audio/aac", "audio/flac", "audio/ogg",
				"video/mp4", "video/mpeg", "video/x-flv", "video/webm", "video/3gpp":
				return mime
			case "audio/mpeg":
				return "audio/mp3"
			case "video/quicktime":
				return "video/mov"
			case "video/x-msvideo":
				return "video/avi"
			}
		}
	case protocol.ConversationModelApiAnthropic:
		if mime == "application/pdf" && (keyed || provider.Authentication == protocol.AuthenticationClaudeOAuth) {
			return mime
		}
	case protocol.ConversationModelApiChatCompletions, protocol.ConversationModelApiResponses:
		if mime == "application/pdf" && (keyed || provider.Authentication == protocol.AuthenticationChatGpt) {
			return mime
		}
		if provider.Api == protocol.ConversationModelApiChatCompletions && keyed && chatAudioFormat(mime) != "" {
			return mime
		}
	}
	return ""
}

func chatAudioFormat(mime string) string {
	switch mime {
	case "audio/wav":
		return "wav"
	case "audio/mpeg", "audio/mp3":
		return "mp3"
	default:
		return ""
	}
}
