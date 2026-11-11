package client

import (
	"context"
	"encoding/base64"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// SynthesizeSpeech reads text aloud with the Node's assigned speech model and
// returns the audio bytes with their media type. A nil voice selects the
// Node's default. The Node answers not_configured when no speech model is
// assigned or the assigned one is unavailable.
func (client *Client) SynthesizeSpeech(ctx context.Context, text string, voice *string) (string, []byte, error) {
	output, err := client.Do(ctx, protocol.Command{Kind: "synthesize_speech", Data: protocol.CommandSynthesizeSpeechPayload{Text: text, Voice: voice}})
	if err != nil {
		return "", nil, err
	}
	audio, err := payload[protocol.SpeechAudio](output.Data)
	if output.Kind != "speech" || err != nil {
		return "", nil, fault(protocol.ErrorCodeInternal, "speech response expected")
	}
	data, err := base64.StdEncoding.DecodeString(audio.DataBase64)
	if err != nil {
		return "", nil, fault(protocol.ErrorCodeInternal, "speech response expected")
	}
	return audio.MediaType, data, nil
}
