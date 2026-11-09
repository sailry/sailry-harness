package observation

import (
	"encoding/json"

	"github.com/sailry/sailry-harness/internal/protocol"
)

func (inventory Inventory) MarshalJSON() ([]byte, error) {
	type plain Inventory
	return json.Marshal(plain(inventory))
}

func (inventory *Inventory) UnmarshalJSON(raw []byte) error {
	type plain Inventory
	var value plain
	if err := json.Unmarshal(raw, &value); err != nil {
		return err
	}
	*inventory = Inventory(value)
	return nil
}

type contextJSON struct {
	Text      string      `json:"text"`
	Ranges    []rangeJSON `json:"ranges"`
	Persisted bool        `json:"persisted"`
}

type rangeJSON struct {
	Start int                                          `json:"start"`
	End   int                                          `json:"end"`
	Kind  protocol.ConversationObservationCategoryKind `json:"kind"`
}

func (context Context) MarshalJSON() ([]byte, error) {
	value := contextJSON{Text: context.text, Persisted: context.persisted}
	if context.ranges != nil {
		value.Ranges = make([]rangeJSON, len(context.ranges))
	}
	for index, span := range context.ranges {
		value.Ranges[index] = rangeJSON{Start: span.start, End: span.end, Kind: span.kind}
	}
	return json.Marshal(value)
}

func (context *Context) UnmarshalJSON(raw []byte) error {
	var value contextJSON
	if err := json.Unmarshal(raw, &value); err != nil {
		return err
	}
	decoded := Context{text: value.Text, persisted: value.Persisted}
	if value.Ranges != nil {
		decoded.ranges = make([]sourceRange, len(value.Ranges))
	}
	for index, span := range value.Ranges {
		decoded.ranges[index] = sourceRange{start: span.Start, end: span.End, kind: span.Kind}
	}
	*context = decoded
	return nil
}
