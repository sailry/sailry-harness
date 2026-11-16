package protocol

import "encoding/json"

// KnowledgeCandidate identifies a ranked storage source without returning its
// value. An executor rechecks the source hash against its current local view.
type KnowledgeCandidate struct {
	Collection KnowledgeID `json:"collection"`
	Key        string      `json:"key"`
	Revision   string      `json:"revision"`
	Score      uint32      `json:"score"`
}

type KnowledgeCandidates struct {
	Candidates []KnowledgeCandidate `json:"candidates"`
	Indexing   bool                 `json:"indexing"`
	Available  bool                 `json:"available"`
}

func (value *KnowledgeCandidates) UnmarshalJSON(data []byte) error {
	type plain KnowledgeCandidates
	var decoded plain
	if err := DecodeObject(data, &decoded, []string{"candidates", "indexing", "available"}, false); err != nil {
		return err
	}
	*value = KnowledgeCandidates(decoded)
	return nil
}

func (value KnowledgeCandidates) MarshalJSON() ([]byte, error) {
	type plain KnowledgeCandidates
	encoded := plain(value)
	encoded.Candidates = normalizeCollections(encoded.Candidates)
	return json.Marshal(encoded)
}
