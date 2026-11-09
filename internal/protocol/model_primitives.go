package protocol

import (
	"bytes"
	"encoding/json"
	"fmt"
	"unicode/utf8"
)

const (
	VERSION                   = 1
	MAX_FRAME_BYTES           = 16 * 1024 * 1024
	MAX_FILE_BYTES            = 128 * 1024
	MAX_EDIT_BYTES            = 64 * 1024 * 1024
	MAX_FILE_LINES            = 2000
	MAX_FIND_MATCHES          = 500
	MAX_DIRECTORY_ENTRIES     = 500
	MAX_SEARCH_MATCHES        = 200
	FILE_TRANSFER_CHUNK_BYTES = 64 * 1024
	FILE_UPLOAD_READY         = 1
	MAX_IMAGE_BYTES           = 5 * 1024 * 1024
	MAX_IMAGE_SIDE            = 1568
	MAX_CHANGES               = 16
	MAX_CHANGE_BYTES          = 2 * MAX_EDIT_BYTES
	MAX_PREVIEW_BYTES         = 32 * 1024
	MAX_GIT_ENTRIES           = 500
	MAX_DIFF_BYTES            = 48 * 1024
	MAX_GIT_COMMITS           = 100
	MAX_GIT_WORKTREES         = 100
	MAX_GIT_LINES             = 500
	MAX_GIT_SHOW_LINES        = 2000
	MAX_GIT_FILE_BYTES        = 32 * 1024 * 1024
)

// Unit carries the JSON null used by Rust's unit type.
type Unit struct{}

func (Unit) MarshalJSON() ([]byte, error) { return []byte("null"), nil }

func (*Unit) UnmarshalJSON(data []byte) error {
	if !bytes.Equal(bytes.TrimSpace(data), []byte("null")) {
		return fmt.Errorf("expected unit null")
	}
	return nil
}

// Char is one Unicode scalar serialized as a JSON string.
type Char string

func (value Char) MarshalJSON() ([]byte, error) {
	if !utf8.ValidString(string(value)) || utf8.RuneCountInString(string(value)) != 1 {
		return nil, fmt.Errorf("expected one Unicode scalar")
	}
	return json.Marshal(string(value))
}

func (value *Char) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	if !utf8.ValidString(decoded) || utf8.RuneCountInString(decoded) != 1 {
		return fmt.Errorf("expected one Unicode scalar")
	}
	*value = Char(decoded)
	return nil
}

// Range preserves Serde's start/end representation for a half-open range.
type Range[T any] struct {
	Start T `json:"start"`
	End   T `json:"end"`
}

func (value *Range[T]) UnmarshalJSON(data []byte) error {
	type plain Range[T]
	var decoded plain
	if err := DecodeObject(data, &decoded, []string{"start", "end"}, false); err != nil {
		return err
	}
	*value = Range[T](decoded)
	return nil
}

// Secret exposes its contents only through an explicit access or wire encoding.
type Secret struct{ value string }

func NewSecret(value string) Secret               { return Secret{value: value} }
func (value Secret) Expose() string               { return value.value }
func (Secret) String() string                     { return "Secret([REDACTED])" }
func (Secret) GoString() string                   { return "Secret([REDACTED])" }
func (value Secret) MarshalJSON() ([]byte, error) { return json.Marshal(value.value) }
func (value *Secret) UnmarshalJSON(data []byte) error {
	if bytes.Equal(bytes.TrimSpace(data), []byte("null")) {
		return fmt.Errorf("expected secret string")
	}
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	value.value = decoded
	return nil
}

func (value Fault) Error() string { return fmt.Sprintf("%s: %s", value.Code, value.Message) }

func modelPointer[T any](value T) *T { return &value }

func defaultSearchOrder() []SearchDriver {
	return []SearchDriver{SearchDriverExa, SearchDriverTinyfish, SearchDriverTavily, SearchDriverBrave, SearchDriverSearxng, SearchDriverBocha, SearchDriverVolcengine, SearchDriverDeepseek, SearchDriverModel}
}

func defaultSearchCapabilities() map[SearchDriver]SearchCapability {
	values := make(map[SearchDriver]SearchCapability)
	for _, driver := range defaultSearchOrder() {
		capability := SearchCapability{Quota: SearchQuotaFreeQuota}
		switch driver {
		case SearchDriverExa:
			capability.Quota = SearchQuotaAnonymousQuota
		case SearchDriverSearxng:
			capability.Quota = SearchQuotaKeyless
		case SearchDriverBocha, SearchDriverDeepseek, SearchDriverModel:
			capability.Quota = SearchQuotaPaid
		}
		switch driver {
		case SearchDriverTinyfish, SearchDriverTavily, SearchDriverBrave, SearchDriverBocha, SearchDriverVolcengine, SearchDriverDeepseek:
			capability.KeyRequired = true
		}
		values[driver] = capability
	}
	return values
}

func defaultSearchProviders() map[SearchDriver]SearchConfig {
	values := make(map[SearchDriver]SearchConfig)
	for _, driver := range defaultSearchOrder() {
		values[driver] = SearchConfig{Enabled: driver == SearchDriverExa}
	}
	return values
}

func defaultModelsLocalChoices() map[ModelsKind]ModelsLocalChoice {
	return map[ModelsKind]ModelsLocalChoice{
		ModelsKindEmbedding: {Enabled: true, Model: modelPointer("granite-multi")},
		ModelsKindOcr:       {Enabled: true},
		ModelsKindDecision:  {Enabled: true},
	}
}
