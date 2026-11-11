package protocol

import "encoding/json"

// CommandSynthesizeSpeechPayload asks the assigned speech model to read text aloud.
type CommandSynthesizeSpeechPayload struct {
	Text  string  `json:"text"`
	Voice *string `json:"voice,omitempty"`
}

func DefaultCommandSynthesizeSpeechPayload() CommandSynthesizeSpeechPayload {
	return CommandSynthesizeSpeechPayload{
		Text:  "",
		Voice: nil,
	}
}
func (value *CommandSynthesizeSpeechPayload) UnmarshalJSON(data []byte) error {
	type plain CommandSynthesizeSpeechPayload
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"text"}, true); err != nil {
		return err
	}
	*value = CommandSynthesizeSpeechPayload(decoded)
	return nil
}
func (value CommandSynthesizeSpeechPayload) MarshalJSON() ([]byte, error) {
	type plain CommandSynthesizeSpeechPayload
	encoded := plain(value)
	return json.Marshal(encoded)
}

// SpeechAudio is one synthesized clip and the model that produced it.
type SpeechAudio struct {
	MediaType  string     `json:"media_type"`
	DataBase64 string     `json:"data_base64"`
	Provider   ProviderID `json:"provider"`
	Model      string     `json:"model"`
}

func DefaultSpeechAudio() SpeechAudio {
	return SpeechAudio{
		MediaType:  "",
		DataBase64: "",
		Provider:   ProviderID{},
		Model:      "",
	}
}
func (value *SpeechAudio) UnmarshalJSON(data []byte) error {
	type plain SpeechAudio
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"media_type", "data_base64", "provider", "model"}, false); err != nil {
		return err
	}
	*value = SpeechAudio(decoded)
	return nil
}
func (value SpeechAudio) MarshalJSON() ([]byte, error) {
	type plain SpeechAudio
	encoded := plain(value)
	return json.Marshal(encoded)
}

func init() {
	commandVariants["synthesize_speech"] = func() any { return new(CommandSynthesizeSpeechPayload) }
	outputVariants["speech"] = func() any { return new(SpeechAudio) }
}
