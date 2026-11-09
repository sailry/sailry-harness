// Code generated from the reviewed v1 protocol declarations. DO NOT EDIT.

package protocol

import (
	"encoding/json"
)

type ConversationSearchQuery struct {
	Text          string  `json:"text"`
	CaseSensitive bool    `json:"case_sensitive"`
	Before        *uint64 `json:"before"`
	Limit         uint16  `json:"limit"`
}

func DefaultConversationSearchQuery() ConversationSearchQuery {
	return ConversationSearchQuery{
		Text:          "",
		CaseSensitive: false,
		Before:        nil,
		Limit:         0,
	}
}
func (value *ConversationSearchQuery) UnmarshalJSON(data []byte) error {
	type plain ConversationSearchQuery
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"text", "case_sensitive", "limit"}, false); err != nil {
		return err
	}
	*value = ConversationSearchQuery(decoded)
	return nil
}
func (value ConversationSearchQuery) MarshalJSON() ([]byte, error) {
	type plain ConversationSearchQuery
	encoded := plain(value)
	return json.Marshal(encoded)
}

type ConversationSearchMatch struct {
	Turn         TurnID        `json:"turn"`
	TurnSequence uint64        `json:"turn_sequence"`
	Entry        string        `json:"entry"`
	Sequence     uint64        `json:"sequence"`
	Part         uint64        `json:"part"`
	Author       string        `json:"author"`
	Snippet      string        `json:"snippet"`
	Highlight    Range[uint64] `json:"highlight"`
}

func DefaultConversationSearchMatch() ConversationSearchMatch {
	return ConversationSearchMatch{
		Turn:         TurnID{},
		TurnSequence: 0,
		Entry:        "",
		Sequence:     0,
		Part:         0,
		Author:       "",
		Snippet:      "",
		Highlight:    Range[uint64]{},
	}
}
func (value *ConversationSearchMatch) UnmarshalJSON(data []byte) error {
	type plain ConversationSearchMatch
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"turn", "turn_sequence", "entry", "sequence", "part", "author", "snippet", "highlight"}, false); err != nil {
		return err
	}
	*value = ConversationSearchMatch(decoded)
	return nil
}
func (value ConversationSearchMatch) MarshalJSON() ([]byte, error) {
	type plain ConversationSearchMatch
	encoded := plain(value)
	return json.Marshal(encoded)
}

type ConversationSearchPage struct {
	Session    SessionID                 `json:"session"`
	Revision   uint64                    `json:"revision"`
	Matches    []ConversationSearchMatch `json:"matches"`
	NextBefore *uint64                   `json:"next_before"`
}

func DefaultConversationSearchPage() ConversationSearchPage {
	return ConversationSearchPage{
		Session:    SessionID{},
		Revision:   0,
		Matches:    []ConversationSearchMatch{},
		NextBefore: nil,
	}
}
func (value *ConversationSearchPage) UnmarshalJSON(data []byte) error {
	type plain ConversationSearchPage
	decoded := plain{}
	if err := DecodeObject(data, &decoded, []string{"session", "revision", "matches"}, false); err != nil {
		return err
	}
	*value = ConversationSearchPage(decoded)
	return nil
}
func (value ConversationSearchPage) MarshalJSON() ([]byte, error) {
	type plain ConversationSearchPage
	encoded := plain(value)
	encoded.Matches = normalizeCollections(encoded.Matches)
	return json.Marshal(encoded)
}
