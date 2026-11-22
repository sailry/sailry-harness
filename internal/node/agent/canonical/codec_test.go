package canonical

import "testing"

type countedPayload struct{ calls int }

func (p *countedPayload) MarshalJSON() ([]byte, error) {
	p.calls++
	return []byte(`{"z":1,"a":2}`), nil
}

func TestTaggedEncodesPayloadOnce(t *testing.T) {
	variants := map[string]func() any{"record": func() any { return new(countedPayload) }}
	payload := &countedPayload{}
	value, err := tagged("kind", "record", payload, variants)
	if err != nil || string(value) != `{"kind":"record","z":1,"a":2}` {
		t.Fatalf("tagged encoding = %s, %v", value, err)
	}
	if payload.calls != 1 {
		t.Fatalf("payload was encoded %d times", payload.calls)
	}
	for _, invalid := range []any{nil, (*countedPayload)(nil), &TextContent{}} {
		if _, err := tagged("kind", "record", invalid, variants); err == nil {
			t.Fatalf("tagged encoding accepted %T", invalid)
		}
	}
}
