package decision

import (
	"context"
	"crypto/sha256"
	"encoding/binary"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"math"
	"os"
	"path/filepath"
	"runtime"
	"slices"
	"strings"
	"testing"

	"github.com/sailry/sailry-harness/internal/protocol"
)

type entry struct {
	key   string
	kind  uint32
	value []byte
}

func le[T uint32 | uint64 | float32 | float64](value T) []byte {
	data, _ := binary.Append(nil, binary.LittleEndian, value)
	return data
}

func text(value string) []byte {
	return append(le(uint64(len(value))), value...)
}

func metadata(kind string) []entry {
	return []entry{
		{"general.architecture", 8, text("modern-bert")},
		{"modern-bert.context_length", 4, le(uint32(8192))},
		{"modern-bert.decision.type", 8, text(kind)},
		{"modern-bert.decision.max_head_tokens", 4, le(uint32(256))},
		{"tokenizer.chat_template.systemone", 8, text("{{ state }}")},
	}
}

// fixture builds a GGUF v3 file of one-dimensional tensors given as type,
// dimension and offset.
func fixture(values []entry, tensors ...[3]uint64) []byte {
	data := append([]byte("GGUF"), le(uint32(3))...)
	data = append(data, le(uint64(len(tensors)))...)
	data = append(data, le(uint64(len(values)))...)
	for _, value := range values {
		data = append(append(append(data, text(value.key)...), le(value.kind)...), value.value...)
	}
	for index, tensor := range tensors {
		data = append(append(data, text(fmt.Sprintf("weight-%d", index))...), le(uint32(1))...)
		data = append(append(append(data, le(tensor[1])...), le(uint32(tensor[0]))...), le(tensor[2])...)
	}
	return append(data, make([]byte, (len(data)+31)/32*32+512-len(data))...)
}

func read(t *testing.T, data []byte) (*Metadata, error) {
	t.Helper()
	path := filepath.Join(t.TempDir(), "model.gguf")
	if err := os.WriteFile(path, data, 0o600); err != nil {
		t.Fatal(err)
	}
	return Validate(path)
}

func kind(err error) Kind {
	var failure *Error
	if !errors.As(err, &failure) {
		return math.MaxUint8
	}
	return failure.Kind
}

func question(t *testing.T, source string) protocol.DecisionQuestion {
	t.Helper()
	var value protocol.DecisionQuestion
	if err := json.Unmarshal([]byte(source), &value); err != nil {
		t.Fatal(err)
	}
	return value
}

func running() bool { return false }

func TestDecisionUnavailable(t *testing.T) {
	identity, err := target(runtime.GOOS, runtime.GOARCH)
	supported := err == nil
	attempt := func(directory string, verified *cache[artifact]) error {
		_, err := new(cache[backend]).get(func() (*backend, error) { return locate(directory, verified) })
		return err
	}
	manifest := func(t *testing.T, directory string, content any) {
		t.Helper()
		data, err := json.Marshal(content)
		if err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(manifestPath(directory), data, 0o600); err != nil {
			t.Fatal(err)
		}
	}

	t.Run("empty runtime directory", func(t *testing.T) {
		directory := t.TempDir()
		pending := new(cache[backend])
		start := func() (*backend, error) {
			return pending.get(func() (*backend, error) { return locate(directory, new(cache[artifact])) })
		}
		_, first := start()
		if kind(first) != Unavailable || !strings.HasPrefix(first.Error(), "decision runtime is unavailable: ") {
			t.Fatalf("runtime error = %v", first)
		}
		if supported && runtime.GOOS != "windows" && !strings.HasSuffix(first.Error(), "no such file or directory") {
			t.Fatalf("runtime error = %v", first)
		}
		// A valid model neither loads nor falls back without the runtime.
		model := filepath.Join(t.TempDir(), "model.gguf")
		if err := os.WriteFile(model, fixture(metadata("laya"), [3]uint64{0, 4, 0}), 0o600); err != nil {
			t.Fatal(err)
		}
		engine, err := open(model, false, start)
		if engine != nil || kind(err) != Unavailable || err.Error() != first.Error() {
			t.Fatalf("open = %v, %v", engine, err)
		}
		if pending.value != nil {
			t.Fatal("a failed runtime was cached")
		}
		// The test executable has no packaged runtime beside it either,
		// unless the integration test already loaded one into the process.
		if err := Available(); current.value == nil && kind(err) != Unavailable {
			t.Fatalf("Available() = %v", err)
		}
		definition := &protocol.ModelsDefinition{Assets: []protocol.ModelsAsset{{Name: "weights.gguf"}}}
		if _, err := Load(definition, t.TempDir(), false); kind(err) != Invalid {
			t.Fatalf("Load = %v", err)
		}
	})

	t.Run("unsupported target", func(t *testing.T) {
		for _, platform := range [][2]string{{"freebsd", "amd64"}, {"linux", "riscv64"}, {"windows", "386"}, {"android", "arm64"}} {
			_, err := target(platform[0], platform[1])
			if kind(err) != Unavailable || err.Error() != "decision runtime is unavailable: platform has no supported prebuilt runtime" {
				t.Fatalf("target(%v) = %v", platform, err)
			}
		}
		var lock lockFile
		if err := json.Unmarshal(lockSource, &lock); err != nil || lock.Version != 1 || lock.Build != Build {
			t.Fatalf("lock = %v, %v", lock.Build, err)
		}
		for _, platform := range [][2]string{
			{"darwin", "arm64"}, {"darwin", "amd64"}, {"linux", "amd64"},
			{"linux", "arm64"}, {"windows", "amd64"}, {"windows", "arm64"},
		} {
			identity, err := target(platform[0], platform[1])
			if asset := lock.Targets[identity]; err != nil || !asset.Available || asset.Entry == nil || len(asset.Files) == 0 {
				t.Fatalf("target(%v) = %q, %v", platform, identity, err)
			}
		}
		if !supported {
			if err := attempt(t.TempDir(), new(cache[artifact])); kind(err) != Unavailable {
				t.Fatalf("unsupported host = %v", err)
			}
		}
	})

	t.Run("mismatching manifest", func(t *testing.T) {
		if !supported {
			t.Skip("host has no locked runtime")
		}
		directory := t.TempDir()
		first := attempt(directory, new(cache[artifact]))
		for content, message := range map[string]string{
			"invalid manifest": "native runtime manifest is invalid",
			`{"version":1}`:    "native runtime manifest is invalid",
			`{"version":1,"target":"` + identity + `","build":"different","files":[]}`:     "native runtime does not match its lock",
			`{"version":2,"target":"` + identity + `","build":"` + Build + `","files":[]}`: "native runtime does not match its lock",
			`{"version":1,"target":"other","build":"` + Build + `","files":[]}`:            "native runtime does not match its lock",
			strings.Repeat(" ", 128*1024+1):                                                "native runtime manifest exceeds its limit",
		} {
			if err := os.WriteFile(manifestPath(directory), []byte(content), 0o600); err != nil {
				t.Fatal(err)
			}
			err := attempt(directory, new(cache[artifact]))
			if kind(err) != Unavailable || err.Error() != "decision runtime is unavailable: "+message || err.Error() == first.Error() {
				t.Fatalf("manifest %.40q = %v", content, err)
			}
		}
	})

	t.Run("mismatching files", func(t *testing.T) {
		if !supported {
			t.Skip("host has no locked runtime")
		}
		var lock lockFile
		if err := json.Unmarshal(lockSource, &lock); err != nil {
			t.Fatal(err)
		}
		locked := lock.Targets[identity].Files
		stage := func(t *testing.T, change func(index int, file *pinned)) string {
			directory := t.TempDir()
			files := make([]pinned, len(locked))
			for index, file := range locked {
				body := "invalid native image " + file.Name
				if err := os.WriteFile(filepath.Join(directory, file.Name), []byte(body), 0o600); err != nil {
					t.Fatal(err)
				}
				digest := sha256.Sum256([]byte(body))
				files[index] = pinned{Name: file.Name, Size: uint64(len(body)), SHA256: hex.EncodeToString(digest[:])}
				change(index, &files[index])
			}
			manifest(t, directory, map[string]any{"version": 1, "target": identity, "build": Build, "files": files})
			return directory
		}
		// Outside macOS the manifest must repeat the locked size and digest.
		for name, change := range map[string]func(int, *pinned){
			"native runtime provenance is invalid":    func(index int, file *pinned) { file.Name += ".other" },
			"native runtime file size does not match": func(index int, file *pinned) { file.Size++ },
			"native runtime checksum does not match":  func(index int, file *pinned) { file.SHA256 = strings.Repeat("0", 64) },
		} {
			err := attempt(stage(t, change), new(cache[artifact]))
			if runtime.GOOS != "darwin" {
				name = "native runtime provenance is invalid"
			}
			if kind(err) != Unavailable || err.Error() != "decision runtime is unavailable: "+name {
				t.Fatalf("%s = %v", name, err)
			}
		}
		linked := stage(t, func(int, *pinned) {})
		path := filepath.Join(linked, locked[0].Name)
		if err := os.Rename(path, path+".source"); err != nil {
			t.Fatal(err)
		}
		if err := os.Symlink(path+".source", path); err == nil {
			if err := attempt(linked, new(cache[artifact])); kind(err) != Unavailable ||
				runtime.GOOS == "darwin" && !strings.HasSuffix(err.Error(), "native runtime must contain ordinary files") {
				t.Fatalf("linked file = %v", err)
			}
		}
		if runtime.GOOS != "darwin" {
			return
		}
		// Files that match the manifest are verified once; the image that
		// cannot load still reports Unavailable on every retry.
		directory, verified := stage(t, func(int, *pinned) {}), new(cache[artifact])
		first := attempt(directory, verified)
		if kind(first) != Unavailable || verified.value == nil {
			t.Fatalf("invalid image = %v, verified %v", first, verified.value)
		}
		if err := os.Remove(manifestPath(directory)); err != nil {
			t.Fatal(err)
		}
		if second := attempt(directory, verified); second.Error() != first.Error() {
			t.Fatalf("retry = %v, want %v", second, first)
		}
	})
}

func TestDecisionContracts(t *testing.T) {
	t.Run("GGUF metadata", func(t *testing.T) {
		values := append(metadata("laya"),
			entry{"modern-bert.decision.temperature.choice.3_5", 6, le(float32(0.75))},
			entry{"modern-bert.decision.score_bias.3.1", 12, le(0.25)},
		)
		parsed, err := read(t, fixture(values, [3]uint64{8, 32, 0}, [3]uint64{12, 256, 64}))
		if err != nil {
			t.Fatal(err)
		}
		if parsed.Architecture != "modern-bert" || parsed.Algorithm != Laya || parsed.Context != 8192 || parsed.head != 256 ||
			parsed.temperatures["choice.3_5"] != 0.75 || !slices.Equal(parsed.bias[3], []float32{0, 0.25, 0}) {
			t.Fatalf("metadata = %+v", parsed)
		}
		for _, missing := range []string{
			"general.architecture", "modern-bert.decision.type", "modern-bert.context_length",
			"tokenizer.chat_template.systemone", "modern-bert.decision.max_head_tokens",
		} {
			values := slices.DeleteFunc(metadata("laya"), func(value entry) bool { return value.key == missing })
			if _, err := read(t, fixture(values, [3]uint64{0, 4, 0})); kind(err) != Invalid {
				t.Fatalf("missing %s = %v", missing, err)
			}
		}
		for _, invalid := range []entry{
			{"split.count", 4, le(uint32(2))},
			{"general.alignment", 4, le(uint32(3))},
			{"general.alignment", 8, text("32")},
			{"unknown", 13, nil},
			{"boolean", 7, []byte{2}},
			{"array", 9, append(le(uint32(9)), le(uint64(1))...)},
			{"array", 9, append(le(uint32(0)), le(uint64(math.MaxUint64))...)},
			{"modern-bert.decision.temperature.choice", 6, le(float32(0))},
			{"modern-bert.decision.temperature.choice", 6, le(float32(math.NaN()))},
			{"modern-bert.decision.score_bias.2.2", 6, le(float32(1))},
			{"general.architecture", 8, text("bert")},
			{"oversized", 8, le(uint64(math.MaxUint64))},
		} {
			if _, err := read(t, fixture(append(metadata("laya"), invalid), [3]uint64{0, 4, 0})); kind(err) != Invalid {
				t.Fatalf("%s = %v", invalid.key, err)
			}
		}
	})

	t.Run("GGUF structure", func(t *testing.T) {
		data := fixture(metadata("laya"), [3]uint64{0, 4, 0})
		for _, length := range []int{0, 4, 12, 24, 32, len(data) - 512} {
			if _, err := read(t, data[:length]); kind(err) != Invalid {
				t.Fatalf("length %d = %v", length, err)
			}
		}
		for _, change := range []struct {
			start int
			value []byte
		}{{4, le(uint32(2))}, {8, le(uint64(maxEntries + 1))}, {16, le(uint64(maxEntries + 1))}, {8, le(uint64(0))}} {
			changed := slices.Clone(data)
			copy(changed[change.start:], change.value)
			if _, err := read(t, changed); kind(err) != Invalid {
				t.Fatalf("header at %d = %v", change.start, err)
			}
		}
		for _, tensors := range [][][3]uint64{
			{{0, 4, 1}}, {{0, 129, 0}}, {{0, math.MaxUint64, 0}}, {{8, 31, 0}}, {{4, 32, 0}},
			{{0, 4, 0}, {0, 4, 0}}, {{0, 4, math.MaxUint64 - 31}},
		} {
			if _, err := read(t, fixture(metadata("laya"), tensors...)); kind(err) != Invalid {
				t.Fatalf("tensors %v = %v", tensors, err)
			}
		}
		directory := t.TempDir()
		large := filepath.Join(directory, "large.gguf")
		file, err := os.Create(large)
		if err != nil {
			t.Fatal(err)
		}
		err = file.Truncate(maxFile + 1)
		file.Close()
		if err != nil {
			t.Skipf("sparse file: %v", err)
		}
		if _, err := Validate(large); kind(err) != Invalid {
			t.Fatalf("oversized file = %v", err)
		}
		source := filepath.Join(directory, "source")
		if err := os.WriteFile(source, data, 0o600); err != nil {
			t.Fatal(err)
		}
		if err := os.Symlink(source, filepath.Join(directory, "model.gguf")); err == nil {
			if _, err := Validate(filepath.Join(directory, "model.gguf")); kind(err) != Invalid {
				t.Fatalf("linked model = %v", err)
			}
		}
		if _, err := Validate(filepath.Join(directory, "missing.gguf")); kind(err) != IO {
			t.Fatalf("missing model = %v", err)
		}
	})

	t.Run("layout detection", func(t *testing.T) {
		for name, expected := range map[string]Algorithm{"laya": Laya, "openjev": OpenJev} {
			parsed, err := read(t, fixture(metadata(name), [3]uint64{0, 4, 0}))
			if err != nil || parsed.Algorithm != expected {
				t.Fatalf("%s = %+v, %v", name, parsed, err)
			}
		}
		if _, err := read(t, fixture(metadata("openjev-type"), [3]uint64{0, 4, 0})); kind(err) != Invalid {
			t.Fatalf("unknown layout = %v", err)
		}
		// Only laya needs the head limit.
		values := slices.DeleteFunc(metadata("openjev"), func(value entry) bool { return strings.HasSuffix(value.key, "max_head_tokens") })
		if _, err := read(t, fixture(values, [3]uint64{0, 4, 0})); err != nil {
			t.Fatal(err)
		}
		keys := func(query *query) (keys []string) {
			for _, option := range query.options {
				keys = append(keys, option.key)
			}
			return keys
		}
		binary := question(t, `{"type":"noul","instructions":"Question","criteria":{"true":"Yes"}}`)
		if query := newQuery("q", binary, Laya); !slices.Equal(keys(query), []string{"false", "true"}) ||
			query.options[0].description != nil || *query.options[1].description != "Yes" || query.column() != 2 {
			t.Fatalf("laya binary = %+v", query)
		}
		if query := newQuery("q", binary, OpenJev); !slices.Equal(keys(query), []string{"true", "false"}) {
			t.Fatalf("openjev binary = %+v", query)
		}
		choice := question(t, `{"type":"choice","instructions":"Question","criteria":{"zeta":null,"alpha":"First"}}`)
		if query := newQuery("q", choice, OpenJev); !slices.Equal(keys(query), []string{"alpha", "zeta"}) || query.column() != 0 {
			t.Fatalf("choice = %+v", query)
		}
		score := question(t, `{"type":"score","instructions":"Question","criteria":["Low","High"]}`)
		if query := newQuery("q", score, Laya); !slices.Equal(keys(query), []string{"0", "1"}) || query.column() != 1 {
			t.Fatalf("score = %+v", query)
		}

		tokens := []int32{1}
		tokens = append(tokens, slices.Repeat([]int32{3}, 40)...)
		tokens = append(tokens, 2, 99)
		tokens = append(tokens, slices.Repeat([]int32{4}, 100)...)
		tokens = append(tokens, 99)
		tokens = append(tokens, slices.Repeat([]int32{5}, 100)...)
		tokens = append(tokens, 2, 7, 8, 9, 2)
		trimmed, markers, err := laya(tokens, 2, 99, 2, 16)
		if err != nil || !slices.Equal(markers, []int{10, 14}) || !slices.Equal(trimmed[len(trimmed)-5:], []int32{2, 7, 8, 9, 2}) ||
			len(trimmed) != 23 || trimmed[10] != 99 || trimmed[14] != 99 {
			t.Fatalf("laya = %v, %v, %v", trimmed, markers, err)
		}
		// A roomy head keeps the question and at most 49 tokens per option.
		roomy, markers, err := laya(tokens, 2, 99, 2, 256)
		if err != nil || !slices.Equal(markers, []int{42, 91}) || len(roomy) != 42+49+49+5 {
			t.Fatalf("laya = %d tokens, %v, %v", len(roomy), markers, err)
		}
		for _, layout := range [][]int32{{1, 2, 3}, {1, 3, 2, 99, 4, 99, 5, 2}, {1, 3, 9, 99, 4, 99, 5, 2, 7, 2}, {1, 3, 2, 99, 4, 99, 5, 2, 7}} {
			if _, _, err := laya(layout, 2, 99, 2, 16); kind(err) != Invalid {
				t.Fatalf("layout %v = %v", layout, err)
			}
		}
	})

	t.Run("answer arithmetic", func(t *testing.T) {
		plain := &Metadata{}
		near := func(left float32, right float64) bool { return math.Abs(float64(left)-right) < 1e-6 }
		binary := question(t, `{"type":"noul","instructions":"Question"}`)
		for algorithm, scores := range map[Algorithm][]float32{
			Laya: {0, float32(math.Log(3))}, OpenJev: {float32(math.Log(3)), 0},
		} {
			answer, err := newQuery("question", binary, algorithm).answer(plain, scores)
			if payload, _ := answer.Data.(protocol.DecisionAnswerNoulPayload); err != nil || answer.Kind != "noul" || !near(payload.Noul, 0.75) {
				t.Fatalf("binary = %+v, %v", answer, err)
			}
		}
		choice := newQuery("question", question(t, `{"type":"choice","instructions":"Question","criteria":{"alpha":null,"zeta":null}}`), Laya)
		answer, err := choice.answer(plain, []float32{0, 0})
		if payload, _ := answer.Data.(protocol.DecisionAnswerChoicePayload); err != nil || payload.Choice != "alpha" ||
			payload.Confidence != 0 || payload.Probabilities["alpha"] != 0.5 || payload.Probabilities["zeta"] != 0.5 {
			t.Fatalf("tie = %+v, %v", answer, err)
		}
		answer, err = choice.answer(plain, []float32{0, float32(math.Log(3))})
		if payload, _ := answer.Data.(protocol.DecisionAnswerChoicePayload); err != nil || payload.Choice != "zeta" || !near(payload.Confidence, 0.5) {
			t.Fatalf("choice = %+v, %v", answer, err)
		}

		calibrated := &Metadata{
			temperatures: map[string]float32{"score": 10, "score.3_5": 2},
			bias:         map[int][]float32{3: {0, 0, 0.5}},
		}
		score := newQuery("score", question(t, `{"type":"score","instructions":"Question","criteria":["Low","Medium","High"]}`), Laya)
		logarithm := func(value float64) float32 { return 2 * float32(math.Log(value)) }
		answer, err = score.answer(calibrated, []float32{logarithm(0.1), logarithm(0.2), logarithm(0.7) - 0.5})
		payload, _ := answer.Data.(protocol.DecisionAnswerScorePayload)
		if err != nil || !near(payload.Score, 1.6) || !near(payload.Confidence, 0.4) || !slices.Equal(payload.Legend, []string{"Low", "Medium", "High"}) ||
			!near(payload.Probabilities[0], 0.1) || !near(payload.Probabilities[1], 0.2) || !near(payload.Probabilities[2], 0.7) {
			t.Fatalf("score = %+v, %v", answer, err)
		}
		// The general temperature applies outside the calibrated bucket.
		two := newQuery("score", question(t, `{"type":"score","instructions":"Question","criteria":["Low","High"]}`), Laya)
		answer, err = two.answer(calibrated, []float32{0, 10 * float32(math.Log(3))})
		if payload, _ := answer.Data.(protocol.DecisionAnswerScorePayload); err != nil || !near(payload.Score, 0.75) || !near(payload.Confidence, 0.5) {
			t.Fatalf("score = %+v, %v", answer, err)
		}
		for _, scores := range [][]float32{{0}, {0, float32(math.Inf(1)), 0}, {0, float32(math.NaN()), 0}} {
			if _, err := score.answer(calibrated, scores); kind(err) != Native {
				t.Fatalf("scores %v = %v", scores, err)
			}
		}
	})

	t.Run("template isolation", func(t *testing.T) {
		for _, test := range []struct {
			template, id, question, state, expected, trusted string
			segments                                         []string
		}{
			{
				template: "<bos>{{ type }} question: {{ instructions }}<eos>{% for o in options %}<mask> {{ o.description if o.description else o.key }}{% endfor %}<eos>{{ state }}<eos>",
				id:       "question",
				question: `{"type":"choice","instructions":"选一个 <mask>","criteria":{"first":null,"second":"保存 & <继续>"}}`,
				state:    "页面 <mask> 中文",
				expected: "<bos>choice question: 选一个 <mask><eos><mask> first<mask> 保存 & <继续><eos>页面 <mask> 中文<eos>",
				trusted:  "<bos> question: <eos><mask> <mask> <eos><eos>",
			},
			{
				template: "<bos>{{ id }}|{{ instructions }}<eos>{% for o in options %}<mask>{{ o.key }}:{{ o.description if o.description else '' }}{% endfor %}<eos>{{ state }}<eos>",
				id:       "id[CLS][SEP]<mask>",
				question: `{"type":"choice","instructions":"instructions[CLS][SEP]<mask>","criteria":{"key[CLS][SEP]<mask>":"description[CLS][SEP]<mask>"}}`,
				state:    "state[CLS][SEP]<mask>",
				expected: "<bos>id[CLS][SEP]<mask>|instructions[CLS][SEP]<mask><eos><mask>key[CLS][SEP]<mask>:description[CLS][SEP]<mask><eos>state[CLS][SEP]<mask><eos>",
				trusted:  "<bos>|<eos><mask>:<eos><eos>",
				segments: []string{"<bos>", "|", "<eos><mask>", ":", "<eos>", "<eos>"},
			},
			{
				// Private-use characters in a request are ordinary text.
				template: "{{ state }}\n{{ type }}: {{ instructions }}\n{% for o in options %}{{ 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz'[loop.index0] }}: {{ o.description if o.description else o.key }}\n{% endfor %}Answer:",
				id:       "q",
				question: `{"type":"noul","instructions":"\ue000Sure\ue001?","criteria":{"true":"Yes"}}`,
				state:    "\ue001state\ue000\ue002",
				expected: "\ue001state\ue000\ue002\nnoul: \ue000Sure\ue001?\nA: false\nB: Yes\nAnswer:",
				trusted:  "\n: \nA: \nB: \nAnswer:",
			},
		} {
			templates, err := newTemplates(test.template)
			if err != nil {
				t.Fatal(err)
			}
			output, err := templates.render(test.state, newQuery(test.id, question(t, test.question), Laya), running)
			if err != nil || output.text != test.expected {
				t.Fatalf("render = %+v, %v", output, err)
			}
			var segments []string
			for _, trusted := range output.trusted() {
				segments = append(segments, output.text[trusted[0]:trusted[1]])
			}
			if strings.Join(segments, "") != test.trusted || test.segments != nil && !slices.Equal(segments, test.segments) {
				t.Fatalf("trusted = %q", segments)
			}
		}
		const control = "word \t\n<mask>\r\n next"
		output := &rendered{text: control}
		start, next := strings.Index(control, "<mask>"), strings.Index(control, "next")
		for _, test := range []struct {
			left, right bool
			start, end  int
		}{{false, false, start, start + 6}, {true, false, 4, start + 6}, {false, true, start, next}, {true, true, 4, next}} {
			if from, to := output.strip(start, start+6, test.left, test.right); from != test.start || to != test.end {
				t.Fatalf("strip(%v, %v) = %d, %d", test.left, test.right, from, to)
			}
		}
	})

	t.Run("template bounds", func(t *testing.T) {
		query := newQuery("question", question(t, `{"type":"noul","instructions":"Question"}`), Laya)
		for _, source := range []string{
			"{% if %}", "{% include 'other' %}", "{% import 'other' as other %}", "{% extends 'other' %}",
			"{% macro again() %}{{ again() }}{% endmacro %}{{ again() }}", "{% block body %}{% endblock %}",
			"{% filter upper %}{{ state }}{% endfilter %}", "{% autoescape true %}{{ state }}{% endautoescape %}",
		} {
			if _, err := newTemplates(source); kind(err) != Invalid || err.Error() != "decision input is invalid: unsupported systemone template" {
				t.Fatalf("template %q = %v", source, err)
			}
		}
		for source, state := range map[string]string{
			"{{ missing }}": "state",
			"{% for x in range(1000000) %}x{% endfor %}":                                                "state",
			"{% for x in range(1000) %}{% for y in range(1000) %}{% endfor %}{% endfor %}":              "state",
			"{% for x in range(400) %}{% for y in range(400) %}{% set z = y %}{% endfor %}{% endfor %}": "state",
			"{{ lipsum() }}":           "state",
			"{{ state }}{{ state }}":   strings.Repeat("x", 200_000),
			"{{ state[:3] }}":          "state",
			"{{ options[0].missing }}": "state",
		} {
			templates, err := newTemplates(source)
			if err != nil {
				t.Fatalf("template %q = %v", source, err)
			}
			if _, err := templates.render(state, query, running); kind(err) != Invalid ||
				err.Error() != "decision input is invalid: systemone template could not be rendered" {
				t.Fatalf("render %q = %v", source, err)
			}
		}
		templates, err := newTemplates("{% for x in range(3) %}{{ x }}{% endfor %}{% for x in range(4, 0, -2) %}{{ x }}{% endfor %}{{ state }}")
		if err != nil {
			t.Fatal(err)
		}
		output, err := templates.render(strings.Repeat("x", maxPrompt-5), query, running)
		if err != nil || len(output.text) != maxPrompt || !strings.HasPrefix(output.text, "01242x") {
			t.Fatalf("render = %v", err)
		}
		if _, err := templates.render("state", query, func() bool { return true }); kind(err) != Cancelled {
			t.Fatalf("cancelled render = %v", err)
		}
	})

	t.Run("request and runtime rules", func(t *testing.T) {
		request := func(source string) error {
			var value protocol.DecisionRequest
			if err := json.Unmarshal([]byte(source), &value); err != nil {
				t.Fatal(err)
			}
			return ValidateRequest(&value)
		}
		if err := request(`{"state":"s","questions":{"a":{"type":"noul","instructions":"Q"},"b":{"type":"choice","instructions":"Q","criteria":{"x":null}},"c":{"type":"score","instructions":"Q","criteria":["l","h"]}}}`); err != nil {
			t.Fatal(err)
		}
		for source, message := range map[string]string{
			`{"state":"s","questions":{}}`:                                                                                  "invalid decision request",
			`{"state":"s","questions":{"":{"type":"noul","instructions":"Q"}}}`:                                             "invalid decision question identity",
			`{"state":"s","questions":{"a":{"type":"noul","instructions":" "}}}`:                                            "invalid decision instructions",
			`{"state":"s","questions":{"a":{"type":"noul","instructions":"Q","criteria":{"maybe":"x"}}}}`:                   "invalid binary decision criteria",
			`{"state":"s","questions":{"a":{"type":"choice","instructions":"Q","criteria":{}}}}`:                            "invalid decision choices",
			`{"state":"s","questions":{"a":{"type":"score","instructions":"Q","criteria":["only"]}}}`:                       "invalid decision score criteria",
			`{"state":"` + strings.Repeat("s", MaxStateBytes+1) + `","questions":{"a":{"type":"noul","instructions":"Q"}}}`: "invalid decision request",
		} {
			var fault *protocol.Fault
			if err := request(source); !errors.As(err, &fault) || fault.Code != protocol.ErrorCodeInvalidRequest || fault.Message != message {
				t.Fatalf("request %.60s = %v", source, err)
			}
		}
		if _, err := new(Engine).Decide(context.Background(), &protocol.DecisionRequest{}); kind(err) != Invalid {
			t.Fatalf("Decide = %v", err)
		}

		for _, version := range []string{"13.3", "13.3.1", "14.0"} {
			if !atLeast(version, [2]uint32{13, 3}) {
				t.Fatalf("atLeast(%s)", version)
			}
		}
		for _, version := range []string{"13.2", "12.7", "invalid", "13"} {
			if atLeast(version, [2]uint32{13, 3}) {
				t.Fatalf("atLeast(%s)", version)
			}
		}
		if !atLeast("2.34", [2]uint32{2, 34}) || atLeast("2.33", [2]uint32{2, 34}) || atLeast("2.34", [2]uint32{2, 38}) || !atLeast("2.39", [2]uint32{2, 38}) {
			t.Fatal("glibc baseline")
		}
		if runtime.GOOS == "darwin" && manifestPath("/app/Contents/MacOS") != "/app/Contents/Resources/llama-runtime.json" ||
			manifestPath(filepath.FromSlash("/bin")) != filepath.FromSlash("/bin/llama-runtime.json") {
			t.Fatal("manifest path")
		}
		if _, err := ordinary(t.TempDir()); kind(err) != Unavailable {
			t.Fatalf("ordinary(directory) = %v", err)
		}

		var variants []string
		for _, name := range []string{
			"libggml-cpu-x64.so", "libggml-cpu-haswell.so", "ggml-cpu-zen4.dll", "libggml-cuda.so", "llama.dll", "libggml-cpu.0.dylib",
		} {
			if variant(name) {
				variants = append(variants, name)
			}
		}
		scores := map[string]int32{"libggml-cpu-x64.so": 1, "libggml-cpu-haswell.so": 8, "ggml-cpu-zen4.dll": 0}
		selected, err := best(variants, func(name string) (int32, error) {
			score, locked := scores[name]
			if !locked {
				t.Fatalf("unlocked backend %s was scored", name)
			}
			return score, nil
		})
		if err != nil || selected != "libggml-cpu-haswell.so" {
			t.Fatalf("best = %q, %v", selected, err)
		}
		missing := unavailable("dependency is missing")
		score := func(name string) (int32, error) {
			switch name {
			case "unsupported":
				return 0, nil
			case "unloadable":
				return 0, missing
			}
			return 1, nil
		}
		if selected, err := best([]string{"unsupported", "unloadable", "portable"}, score); err != nil || selected != "portable" {
			t.Fatalf("best = %q, %v", selected, err)
		}
		if _, err := best([]string{"unsupported"}, score); kind(err) != Unavailable || !strings.HasSuffix(err.Error(), "no compatible CPU backend is available") {
			t.Fatalf("best = %v", err)
		}
		if _, err := best([]string{"unloadable"}, score); err != missing {
			t.Fatalf("best = %v", err)
		}
	})
}
