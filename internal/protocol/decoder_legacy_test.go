package protocol

import (
	"bytes"
	"encoding/json"
	"encoding/json/jsontext"
	"fmt"
	"io"
	"math/rand/v2"
	"reflect"
	"strconv"
	"strings"
	"testing"
	"time"
)

// The functions below are the reflective helpers the streaming Decoder
// replaced, kept as the reference its results are compared with.

func legacyUnmarshalUnion(data []byte, tag, content string, variants map[string]func() any, denyUnknown bool) (string, any, error) {
	var fields map[string]json.RawMessage
	var kind string
	var payload json.RawMessage
	if tag == "" && len(bytes.TrimSpace(data)) > 0 && bytes.TrimSpace(data)[0] == '"' {
		if err := json.Unmarshal(data, &kind); err != nil {
			return "", nil, err
		}
	} else {
		entries, decoded, err := legacyReadObject(data)
		if err != nil {
			return "", nil, err
		}
		fields = decoded
		if tag == "" {
			if len(entries) != 1 {
				return "", nil, fmt.Errorf("expected one external variant")
			}
			for name, value := range fields {
				kind, payload = name, value
			}
		} else {
			if err := legacyRejectDuplicateFields(entries, map[string]bool{tag: true, content: content != ""}); err != nil {
				return "", nil, err
			}
			encoded, present := fields[tag]
			if !present {
				return "", nil, fmt.Errorf("missing field %q", tag)
			}
			if err := json.Unmarshal(encoded, &kind); err != nil {
				return "", nil, err
			}
			delete(fields, tag)
			if content != "" {
				payload = fields[content]
				delete(fields, content)
				if denyUnknown && len(fields) > 0 {
					return "", nil, fmt.Errorf("unknown enum field")
				}
			} else {
				payload = []byte{'{'}
				for _, entry := range entries {
					if entry.name == tag {
						continue
					}
					if len(payload) > 1 {
						payload = append(payload, ',')
					}
					name, _ := json.Marshal(entry.name)
					payload = append(payload, name...)
					payload = append(payload, ':')
					payload = append(payload, entry.value...)
				}
				payload = append(payload, '}')
			}
		}
	}
	factory, exists := variants[kind]
	if !exists {
		return "", nil, fmt.Errorf("unknown variant %q", kind)
	}
	if factory == nil {
		if tag != "" && content == "" && denyUnknown && len(fields) > 0 {
			return "", nil, fmt.Errorf("unknown unit variant field")
		}
		if (tag == "" || content != "") && len(payload) > 0 && !bytes.Equal(bytes.TrimSpace(payload), []byte("null")) {
			return "", nil, fmt.Errorf("expected unit variant")
		}
		return kind, nil, nil
	}
	if len(payload) == 0 {
		return "", nil, fmt.Errorf("missing variant content")
	}
	value := factory()
	if err := checkJSONNulls(payload, reflect.TypeOf(value).Elem()); err != nil {
		return "", nil, err
	}
	if err := json.Unmarshal(payload, value); err != nil {
		return "", nil, err
	}
	return kind, value, nil
}

func legacyDecodeObject(data []byte, target any, required []string, denyUnknown bool) error {
	entries, fields, err := legacyReadObject(data)
	if err != nil {
		return err
	}
	for _, name := range required {
		_, present := fields[name]
		if !present {
			return fmt.Errorf("missing field %q", name)
		}
	}
	targetType := reflect.TypeOf(target).Elem()
	known := make(map[string]bool, targetType.NumField())
	for index := 0; index < targetType.NumField(); index++ {
		field := targetType.Field(index)
		name := strings.Split(field.Tag.Get("json"), ",")[0]
		if name == "-" || field.PkgPath != "" {
			continue
		}
		if name == "" {
			name = field.Name
		}
		known[name] = true
		value, present := fields[name]
		if !present {
			continue
		}
		if err := checkJSONNulls(value, field.Type); err != nil {
			return fmt.Errorf("field %q: %w", name, err)
		}
		if field.Type.Kind() == reflect.Pointer && bytes.Equal(bytes.TrimSpace(value), []byte("null")) {
			for _, key := range required {
				if key != name {
					continue
				}
				return fmt.Errorf("field %q must not be null", name)
			}
		}
	}
	if err := legacyRejectDuplicateFields(entries, known); err != nil {
		return err
	}
	for name := range fields {
		if !known[name] {
			if denyUnknown {
				return fmt.Errorf("unknown field %q", name)
			}
			delete(fields, name)
		}
	}
	filtered, err := json.Marshal(fields)
	if err != nil {
		return err
	}
	if denyUnknown {
		decoder := json.NewDecoder(bytes.NewReader(filtered))
		decoder.DisallowUnknownFields()
		return decoder.Decode(target)
	}
	return json.Unmarshal(filtered, target)
}

type legacyField struct {
	name  string
	value json.RawMessage
}

func legacyReadObject(data []byte) ([]legacyField, map[string]json.RawMessage, error) {
	decoder := json.NewDecoder(bytes.NewReader(data))
	start, err := decoder.Token()
	if err != nil {
		return nil, nil, err
	}
	if start != json.Delim('{') {
		return nil, nil, fmt.Errorf("expected object")
	}
	var entries []legacyField
	fields := make(map[string]json.RawMessage)
	for decoder.More() {
		token, err := decoder.Token()
		if err != nil {
			return nil, nil, err
		}
		name, ok := token.(string)
		if !ok {
			return nil, nil, fmt.Errorf("expected field name")
		}
		var value json.RawMessage
		if err := decoder.Decode(&value); err != nil {
			return nil, nil, err
		}
		entries = append(entries, legacyField{name, value})
		fields[name] = value
	}
	if _, err := decoder.Token(); err != nil {
		return nil, nil, err
	}
	if err := decoder.Decode(new(any)); err != io.EOF {
		return nil, nil, fmt.Errorf("unexpected data after object")
	}
	return entries, fields, nil
}

func legacyRejectDuplicateFields(entries []legacyField, known map[string]bool) error {
	seen := make(map[string]bool)
	for _, entry := range entries {
		if !known[entry.name] {
			continue
		}
		if seen[entry.name] {
			return fmt.Errorf("duplicate field %q", entry.name)
		}
		seen[entry.name] = true
	}
	return nil
}

// legacyInner and legacyMix have no decoders of their own, like the structs
// other packages hand to DecodeObject.
type legacyInner struct {
	A        string         `json:"a"`
	B        *int           `json:"b,omitempty"`
	C        []string       `json:"c"`
	D        map[string]any `json:"d"`
	Untagged int
	E        json.RawMessage `json:"e"`
	F        *legacyInner    `json:"f"`
}

type legacyMix struct {
	S      string                     `json:"s"`
	PS     *string                    `json:"ps"`
	I      int32                      `json:"i"`
	U      uint64                     `json:"u"`
	U8     uint8                      `json:"u8"`
	F      float64                    `json:"f"`
	B      bool                       `json:"b"`
	PB     *bool                      `json:"pb"`
	T      time.Time                  `json:"t"`
	Any    any                        `json:"any"`
	Raw    json.RawMessage            `json:"raw"`
	PRaw   *json.RawMessage           `json:"praw"`
	Num    json.Number                `json:"num"`
	Bytes  []byte                     `json:"bytes"`
	Arr    [2]int                     `json:"arr"`
	Sl     []int                      `json:"sl"`
	SlP    []*string                  `json:"slp"`
	M      map[string]string          `json:"m"`
	MI     map[int]string             `json:"mi"`
	MRaw   map[string]json.RawMessage `json:"mraw"`
	MP     map[string]*legacyInner    `json:"mp"`
	In     legacyInner                `json:"in"`
	PIn    *legacyInner               `json:"pin"`
	SlIn   []legacyInner              `json:"slin"`
	ID     SessionID                  `json:"id"`
	PID    *SessionID                 `json:"pid"`
	KeyID  map[SessionID]string       `json:"keyid"`
	Effort Effort                     `json:"effort"`
	Field  *ToolDisplayField          `json:"field"`
	Part   ConversationPart           `json:"part"`
	Parts  []ConversationPart         `json:"parts"`
	Skip   string                     `json:"-"`
	Omit   string                     `json:"omit,omitempty"`
	Bare   string
	hidden int
}

// legacyDocuments pairs each document with the decoder it suits: the mixed
// struct, the inner struct, or one of legacyShapes.
var legacyDocuments = []struct {
	mode int
	text string
}{
	{legacyMixed, `{"s":"x","ps":"y","i":-3,"u":18446744073709551615,"u8":255,"f":1.5,"b":true,"pb":false,"t":"2026-11-11T08:00:00Z"}`},
	{legacyMixed, `{"any":{"a":[1,"<b>",null]},"raw":{"x": "<y>" , "z":[ 1 , 2 ]},"praw":12345678901234567890,"num":-0}`},
	{legacyMixed, `{"bytes":"aGVsbG8=","arr":[1,2],"sl":[1,2,3],"slp":["a",null],"m":{"k":"v"},"mi":{"1":"a"},"mraw":{"k":{"n": null}}}`},
	{legacyMixed, `{"mp":{"k":{"a":"x","Untagged":1}},"in":{"a":"x","b":2,"c":["y"],"d":{"k":1},"e":[ ]},"pin":{"a":"z","f":{"a":"deep"}},"slin":[{"a":"1"},{"a":"2","untagged":3}]}`},
	{legacyMixed, `{"id":"28a5d3df-dae3-4ab0-8297-3e0afae820d1","pid":"a8f643e1-2060-4fc0-9e88-4a80e48c2892","keyid":{"28a5d3df-dae3-4ab0-8297-3e0afae820d1":"x"}}`},
	{legacyMixed, `{"effort":"default","field":{"path":"/title","names":{"a":"b"}},"part":{"kind":"text","data":"partial"},"parts":[{"data":"first","kind":"text"}]}`},
	{legacyMixed, `{"omit":"o","Bare":"bare","bare":"lower","-":"dash","Skip":"skip","hidden":1,"zz":{"unknown":[true]}}`},
	{legacyMixed, `{"s":"\u00e9\ud83d\ude00\u2028","ps":"a\\b\"c","raw":"\u003c\/tag\u003e","any":"\ud83d","m":{"\u006b":"escaped name"}}`},
	{legacyMixed, "{\"s\":\"bad \xff utf8\",\"raw\":\"\xc0\xaf\",\"any\":[\"\xed\xa0\x80\"]}"},
	{legacyMixed, `{"f":1e400,"i":2147483648,"u":-1,"u8":256,"num":"1"}`},
	{legacyMixed, ` { "s" : "spaced" , "sl" : [ ] , "m" : { } , "pin" : null , "ps" : null , "pb" : null } `},
	{legacyPlain, `{"a":"x","b":1,"c":["y"],"d":{"k":[1]},"Untagged":2,"e":{"r": 1},"f":{"a":"deep","f":null}}`},
	{legacyPlain, `{"a":"only"}`},
	{1, `{"kind":"text","data":"partial","s":"x"}`},
	{1, `{"data":{"a":"x","b":1},"kind":"inner"}`},
	{3, `{"type":"inner","a":"x","c":["y"],"Untagged":4}`},
	{3, `{"a":"x","type":"inner","e":{"raw": 1},"f":{"a":"nested","e":"<"}}`},
	{3, `{"type":"field","path":"/a","code":true}`},
	{3, `{"type":"part","kind":"text","data":"partial"}`},
	{0, `{"inner":{"a":"x"}}`},
	{0, `{"text":"x"}`},
	{0, `{"count":3}`},
	{0, `{"list":["a","b"]}`},
	{0, `{"raw":{"a": [1 ,2]}}`},
	{0, `{"any":null}`},
	{0, `{"unit":null}`},
	{0, `"unit"`},
	{2, `{"kind":"unit"}`},
	{3, `{"type":"unit"}`},
	{1, `{"kind":"unit","data":null}`},
	{1, `{"kind":null,"data":"empty kind"}`},
	{0, `{"time":"2026-11-11T08:00:00+08:00"}`},
	{0, `{"map":{"a":1,"a":2}}`},
	{0, `{"ptr":{"a":"x"}}`},
	{0, `{"ptr":null}`},
	{0, `{"mix":{"s":"x","in":{"a":"y"}}}`},
	{1, `{"kind":"mix","data":{"s":"x","raw":{"k": "<"}}}`},
	{3, `{"type":"mix","s":"x","raw":{"k": "<"},"part":{"kind":"text","data":"p"}}`},
	{4, `{"a":"inner","c":["tag named like a member"]}`},
}

const (
	legacyMixed = -1
	legacyPlain = -2
)

var legacyShapes = [][2]string{{"", ""}, {"kind", "data"}, {"kind", ""}, {"type", ""}, {"a", ""}}

type legacyNode struct {
	kind  byte
	text  []byte
	names [][]byte
	kids  []*legacyNode
}

func legacyParse(decoder *jsontext.Decoder) *legacyNode {
	kind := decoder.PeekKind()
	if kind != '{' && kind != '[' {
		value, err := decoder.ReadValue()
		if err != nil {
			return nil
		}
		return &legacyNode{kind: byte(kind), text: bytes.Clone(value)}
	}
	node := &legacyNode{kind: byte(kind)}
	decoder.ReadToken()
	for decoder.PeekKind() != kind+2 {
		if kind == '{' {
			name, err := decoder.ReadValue()
			if err != nil {
				return nil
			}
			node.names = append(node.names, bytes.Clone(name))
		}
		kid := legacyParse(decoder)
		if kid == nil {
			return nil
		}
		node.kids = append(node.kids, kid)
	}
	decoder.ReadToken()
	return node
}

func (node *legacyNode) write(out *bytes.Buffer) {
	if node.kind != '{' && node.kind != '[' {
		out.Write(node.text)
		return
	}
	out.WriteByte(node.kind)
	for index, kid := range node.kids {
		if index > 0 {
			out.WriteByte(',')
		}
		if node.kind == '{' {
			out.Write(node.names[index])
			out.WriteByte(':')
		}
		kid.write(out)
	}
	out.WriteByte(node.kind + 2)
}

func (node *legacyNode) all(into *[]*legacyNode) {
	*into = append(*into, node)
	for _, kid := range node.kids {
		kid.all(into)
	}
}

// legacyMutate returns document with one or two random edits: a member
// dropped, repeated, renamed, reordered or added, a value replaced or nested,
// or the bytes cut or disturbed.
func legacyMutate(r *rand.Rand, document string) []byte {
	root := legacyParse(jsontext.NewDecoder(strings.NewReader(document), jsontext.AllowDuplicateNames(true), jsontext.AllowInvalidUTF8(true)))
	if root == nil {
		return []byte(document)
	}
	values := []string{"null", "true", `""`, `"x"`, "0", "-0", "1.5", "1e2", "9007199254740993", "18446744073709551616", "1e400", "[]", "{}", "[null]",
		`{"a":null}`, `"<&>"`, "\"\xff\"", `"\ud83d"`, `"\u2028"`, `{"kind":"text","data":"p"}`, `[[[[[[[[1]]]]]]]]`, `"default"`, `"unit"`}
	for count := 1 + r.IntN(2); count > 0; count-- {
		var nodes []*legacyNode
		root.all(&nodes)
		node := nodes[r.IntN(len(nodes))]
		replacement := func() *legacyNode {
			return legacyParse(jsontext.NewDecoder(strings.NewReader(values[r.IntN(len(values))]), jsontext.AllowInvalidUTF8(true)))
		}
		switch choice := r.IntN(12); {
		case node.kind == '{' && len(node.kids) > 0 && choice < 8:
			index := r.IntN(len(node.kids))
			switch choice {
			case 0:
				node.names = append(node.names[:index:index], node.names[index+1:]...)
				node.kids = append(node.kids[:index:index], node.kids[index+1:]...)
			case 1:
				node.names, node.kids = append(node.names, node.names[index]), append(node.kids, node.kids[index])
			case 2:
				node.names, node.kids = append([][]byte{node.names[index]}, node.names...), append([]*legacyNode{replacement()}, node.kids...)
			case 3:
				node.names, node.kids = append(node.names, []byte(`"zz"`)), append(node.kids, replacement())
			case 4:
				other := r.IntN(len(node.kids))
				node.names[index], node.names[other] = node.names[other], node.names[index]
				node.kids[index], node.kids[other] = node.kids[other], node.kids[index]
			case 5:
				node.names[index] = bytes.ToUpper(node.names[index])
			case 6:
				name := node.names[index]
				if len(name) > 2 && name[1] < 0x80 && name[1] != '\\' {
					node.names[index] = append([]byte(fmt.Sprintf(`"\u%04x`, name[1])), name[2:]...)
				}
			default:
				node.names[index] = node.names[r.IntN(len(node.names))]
			}
		case choice < 10:
			*node = *replacement()
		case choice == 10:
			inner := *node
			*node = legacyNode{kind: '[', kids: []*legacyNode{&inner}}
		default:
			if node.kind == '"' {
				node.text = bytes.ToUpper(node.text)
			} else {
				*node = legacyNode{kind: 'n', text: []byte("null")}
			}
		}
	}
	var out bytes.Buffer
	root.write(&out)
	data := out.Bytes()
	switch r.IntN(12) {
	case 0:
		data = data[:r.IntN(len(data)+1)]
	case 1:
		index := r.IntN(len(data) + 1)
		insert := []string{" ", ",", ":", `"`, "{", "}", "\xff", "\n", "0", "x"}[r.IntN(10)]
		data = append(append(bytes.Clone(data[:index]), insert...), data[index:]...)
	case 2:
		data = append(data, []string{" ", "x", "{}", "null"}[r.IntN(4)]...)
	}
	return data
}

// legacyEquivalence checks that the streaming helpers accept the same inputs
// as the reflective ones and produce the same values and the same bytes when
// those values are encoded again.
func legacyEquivalence(t *testing.T) {
	text, flag := "preset", true
	preset := func(value *legacyMix) {
		value.S, value.PS, value.B, value.PB = "preset", &text, true, &flag
		value.Sl, value.M, value.MRaw, value.SlIn = []int{}, map[string]string{}, map[string]json.RawMessage{}, []legacyInner{}
		value.Raw, value.Any, value.PIn, value.Effort = json.RawMessage("null"), "preset", &legacyInner{A: "preset"}, Effort{Kind: "default"}
	}
	variants := map[string]func() any{
		"text": func() any { return new(string) }, "count": func() any { return new(uint64) }, "list": func() any { return new([]string) },
		"raw": func() any { return new(json.RawMessage) }, "any": func() any { return new(any) }, "inner": func() any { return new(legacyInner) },
		"ptr": func() any { return new(*legacyInner) }, "map": func() any { return new(map[string]int) }, "time": func() any { return new(time.Time) },
		"mix": func() any { return new(legacyMix) }, "field": func() any { return new(ToolDisplayField) }, "part": func() any { return new(ConversationPart) },
		"": func() any { return new(string) }, "unit": nil,
	}
	requirements := [][]string{nil, nil, nil, {"s"}, {"ps", "pb"}, {"ghost"}, {"a"}, {"a", "b", "f"}}
	r := rand.New(rand.NewPCG(1, 2))
	cases := 40000
	if testing.Short() {
		cases = 4000
	}
	accepted := 0
	for index := range cases {
		document := legacyDocuments[r.IntN(len(legacyDocuments))]
		data := []byte(document.text)
		if index < 8*len(legacyDocuments) {
			document = legacyDocuments[index%len(legacyDocuments)]
			data = []byte(document.text)
		} else {
			data = legacyMutate(r, document.text)
		}
		mode := document.mode
		if r.IntN(10) == 0 {
			mode = r.IntN(len(legacyShapes)+2) - 2
		}
		required, deny := requirements[r.IntN(len(requirements))], r.IntN(2) == 0
		var want, got any
		var wantErr, gotErr error
		switch mode {
		case legacyMixed:
			old, new := new(legacyMix), new(legacyMix)
			if r.IntN(2) == 0 {
				preset(old)
				preset(new)
			}
			wantErr, gotErr = legacyDecodeObject(data, old, required, deny), DecodeObject(data, new, required, deny)
			want, got = old, new
		case legacyPlain:
			old, new := new(legacyInner), new(legacyInner)
			wantErr, gotErr = legacyDecodeObject(data, old, required, deny), DecodeObject(data, new, required, deny)
			want, got = old, new
		default:
			shape := legacyShapes[mode]
			var oldKind, newKind string
			oldKind, want, wantErr = legacyUnmarshalUnion(data, shape[0], shape[1], variants, deny)
			newKind, got, gotErr = UnmarshalUnion(data, shape[0], shape[1], variants, deny)
			if oldKind != newKind {
				t.Fatalf("kind %q, want %q for %q", newKind, oldKind, data)
			}
		}
		if (wantErr == nil) != (gotErr == nil) {
			t.Fatalf("error %v, want %v for %q", gotErr, wantErr, data)
		}
		if json.Valid(data) && isNormal(data) {
			if encoded, err := json.Marshal(json.RawMessage(data)); err != nil || !bytes.Equal(encoded, data) {
				t.Fatalf("isNormal accepted %q, which encodes as %q", data, encoded)
			}
		}
		if wantErr != nil {
			continue
		}
		accepted++
		if !reflect.DeepEqual(want, got) {
			t.Fatalf("decoded %#v, want %#v for %q", got, want, data)
		}
		wantBytes, wantErr := json.Marshal(want)
		gotBytes, gotErr := json.Marshal(got)
		if (wantErr == nil) != (gotErr == nil) || !bytes.Equal(wantBytes, gotBytes) {
			t.Fatalf("encoded %s, want %s for %q", gotBytes, wantBytes, data)
		}
	}
	if accepted < cases/10 {
		t.Fatalf("only %d of %d cases were accepted", accepted, cases)
	}
}

func BenchmarkDecodeObject(b *testing.B) {
	parts := make([]string, 200)
	for index := range parts {
		parts[index] = `{"kind":"text","data":"part ` + strconv.Itoa(index) + `"}`
	}
	data := []byte(`{"s":"x","ps":"y","i":-3,"u":7,"f":1.5,"b":true,"t":"2026-11-11T08:00:00Z","raw":{"a":[1,2,3]},"sl":[1,2,3],"m":{"k":"v"},` +
		`"in":{"a":"x","c":["y"]},"id":"28a5d3df-dae3-4ab0-8297-3e0afae820d1","effort":"default","field":{"path":"/title"},` +
		`"part":{"kind":"text","data":"partial"},"parts":[` + strings.Join(parts, ",") + `]}`)
	for name, decode := range map[string]func([]byte, any, []string, bool) error{"streaming": DecodeObject, "reflective": legacyDecodeObject} {
		b.Run(name, func(b *testing.B) {
			b.SetBytes(int64(len(data)))
			b.ReportAllocs()
			for b.Loop() {
				if err := decode(data, new(legacyMix), []string{"s", "id"}, false); err != nil {
					b.Fatal(err)
				}
			}
		})
	}
}
