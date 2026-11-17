package protocol

import "encoding/json"

// PersonalKVWrite is the normalized final write of one key in an admitted
// transaction. A predecessor binds successive offline writes to their receipt.
type PersonalKVWrite struct {
	Key          string              `json:"key"`
	BaseRevision uint64              `json:"base_revision"`
	Predecessor  *RequestID          `json:"predecessor,omitempty"`
	Value        json.RawMessage     `json:"value,omitempty"`
	ValueBytes   []byte              `json:"value_bytes,omitempty"`
	Index        *PluginStorageIndex `json:"index,omitempty"`
	Present      bool                `json:"present"`
}

type PersonalKVGroup struct {
	Controller NodeID            `json:"controller"`
	Executor   NodeID            `json:"executor"`
	Request    RequestID         `json:"request"`
	Sequence   uint64            `json:"sequence"`
	Package    PluginReference   `json:"package"`
	Writes     []PersonalKVWrite `json:"writes"`
	Digest     string            `json:"digest"`
}

type PersonalKVRecord struct {
	Plugin     string              `json:"plugin"`
	Key        string              `json:"key"`
	Revision   uint64              `json:"revision"`
	Value      json.RawMessage     `json:"value,omitempty"`
	ValueBytes []byte              `json:"value_bytes,omitempty"`
	Index      *PluginStorageIndex `json:"index,omitempty"`
	Present    bool                `json:"present"`
}

// Values travel as bytes so JSON encoding cannot change whitespace or escaping.
// The legacy value member remains readable for persisted outbox groups and their
// original digests; new records populate only value_bytes.
func (value PersonalKVWrite) ValueJSON() json.RawMessage {
	if value.ValueBytes != nil {
		return value.ValueBytes
	}
	return value.Value
}

func (value PersonalKVRecord) ValueJSON() json.RawMessage {
	if value.ValueBytes != nil {
		return value.ValueBytes
	}
	return value.Value
}

type PersonalKVReceipt struct {
	Executor NodeID             `json:"executor"`
	Request  RequestID          `json:"request"`
	Sequence uint64             `json:"sequence"`
	Digest   string             `json:"digest"`
	Applied  bool               `json:"applied"`
	Records  []PersonalKVRecord `json:"records"`
	Reason   string             `json:"reason,omitempty"`
}

type PersonalKVConflict struct {
	Package string    `json:"package"`
	Request RequestID `json:"request"`
	Reason  string    `json:"reason"`
}

type PersonalKVCursor struct {
	Plugin string `json:"plugin"`
	Key    string `json:"key"`
}

// Snapshot pages are staged before publication. A generation change aborts
// the partial snapshot instead of treating its missing pages as deletions.
type PersonalKVSnapshot struct {
	Generation uint64             `json:"generation"`
	After      *PersonalKVCursor  `json:"after,omitempty"`
	Next       *PersonalKVCursor  `json:"next,omitempty"`
	Records    []PersonalKVRecord `json:"records"`
}

type PersonalKVPending struct {
	Group   *PersonalKVGroup `json:"group,omitempty"`
	Through uint64           `json:"through,omitempty"`
}

func init() {
	outputVariants["personal_kv_pending"] = func() any { return new(PersonalKVPending) }
}
