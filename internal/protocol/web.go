// Hand-maintained v1 protocol declarations; keep them consistent with the fixtures and tolerance tests.

package protocol

import (
	"encoding/json"
)

type WebRequest struct {
	Url    string `json:"url"`
	Offset uint64 `json:"offset"`
}

func DefaultWebRequest() WebRequest {
	return WebRequest{
		Url:    "",
		Offset: 0,
	}
}
func (value *WebRequest) UnmarshalJSON(data []byte) error {
	type plain WebRequest
	decoded := plain{}
	decoded.Offset = 0
	if err := DecodeObject(data, &decoded, []string{"url"}, true); err != nil {
		return err
	}
	*value = WebRequest(decoded)
	return nil
}
func (value WebRequest) MarshalJSON() ([]byte, error) {
	type plain WebRequest
	encoded := plain(value)
	return json.Marshal(encoded)
}

type WebPage struct {
	Url         string  `json:"url"`
	Status      uint16  `json:"status"`
	ContentType string  `json:"content_type"`
	Content     string  `json:"content"`
	Title       *string `json:"title"`
	NextOffset  *uint64 `json:"next_offset"`
	Truncated   bool    `json:"truncated"`
	Untrusted   bool    `json:"untrusted"`
	Notice      *string `json:"notice,omitempty"`
}

func DefaultWebPage() WebPage {
	return WebPage{
		Url:         "",
		Status:      0,
		ContentType: "",
		Content:     "",
		Title:       nil,
		NextOffset:  nil,
		Truncated:   false,
		Untrusted:   false,
		Notice:      nil,
	}
}
func (value *WebPage) UnmarshalJSON(data []byte) error {
	type plain WebPage
	decoded := plain{}
	decoded.Notice = nil
	if err := DecodeObject(data, &decoded, []string{"url", "status", "content_type", "content", "truncated", "untrusted"}, false); err != nil {
		return err
	}
	*value = WebPage(decoded)
	return nil
}
func (value WebPage) MarshalJSON() ([]byte, error) {
	type plain WebPage
	encoded := plain(value)
	return json.Marshal(encoded)
}
