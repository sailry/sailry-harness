// Hand-maintained v1 protocol declarations; keep them consistent with the fixtures and tolerance tests.

package protocol

import (
	"encoding/json"
)

type ProcessOutputOutputStream string

const (
	ProcessOutputOutputStreamStdout ProcessOutputOutputStream = "stdout"
	ProcessOutputOutputStreamStderr ProcessOutputOutputStream = "stderr"
)

func (value ProcessOutputOutputStream) MarshalJSON() ([]byte, error) { return marshalEnum(value) }
func (value *ProcessOutputOutputStream) UnmarshalJSON(data []byte) error {
	var decoded string
	if err := json.Unmarshal(data, &decoded); err != nil {
		return err
	}
	switch ProcessOutputOutputStream(decoded) {
	case ProcessOutputOutputStreamStdout, ProcessOutputOutputStreamStderr:
		*value = ProcessOutputOutputStream(decoded)
		return nil
	default:
		return unknownEnum(data, value, decoded)
	}
}
func (value ProcessOutputOutputStream) MarshalText() ([]byte, error) {
	if _, err := value.MarshalJSON(); err != nil {
		return nil, err
	}
	return []byte(value), nil
}
func (value *ProcessOutputOutputStream) UnmarshalText(data []byte) error {
	encoded, err := json.Marshal(string(data))
	if err != nil {
		return err
	}
	return value.UnmarshalJSON(encoded)
}

type ProcessOutputOutputPage struct {
	Info        ProcessInfo               `json:"info"`
	Stream      ProcessOutputOutputStream `json:"stream"`
	Offset      uint64                    `json:"offset"`
	TotalBytes  uint64                    `json:"total_bytes"`
	NextOffset  *uint64                   `json:"next_offset"`
	Text        string                    `json:"text"`
	Truncated   bool                      `json:"truncated"`
	InvalidUtf8 bool                      `json:"invalid_utf8"`
}

func DefaultProcessOutputOutputPage() ProcessOutputOutputPage {
	return ProcessOutputOutputPage{
		Info:        DefaultProcessInfo(),
		Stream:      ProcessOutputOutputStreamStdout,
		Offset:      0,
		TotalBytes:  0,
		NextOffset:  nil,
		Text:        "",
		Truncated:   false,
		InvalidUtf8: false,
	}
}
func (value *ProcessOutputOutputPage) UnmarshalJSON(data []byte) error {
	type plain ProcessOutputOutputPage
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"info", "stream", "offset", "total_bytes", "text", "truncated", "invalid_utf8"}, false); err != nil {
		return err
	}
	*value = ProcessOutputOutputPage(decoded)
	return nil
}
func (value ProcessOutputOutputPage) MarshalJSON() ([]byte, error) {
	type plain ProcessOutputOutputPage
	encoded := plain(value)
	return json.Marshal(encoded)
}
