package speech_test

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"errors"
	"math"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"testing"

	"github.com/sailry/sailry-harness/internal/localmodels/assets"
	"github.com/sailry/sailry-harness/internal/protocol"
	"github.com/sailry/sailry-harness/internal/speech"
	"github.com/sailry/sailry-harness/internal/speech/models"
)

func TestSpeech(t *testing.T) {
	t.Run("resampler", checkResampler)
	t.Run("audio bounds", checkAudioBounds)
	t.Run("language", checkLanguage)
	t.Run("runtime absent", checkRuntimeAbsent)
	t.Run("model index", checkModelIndex)
	t.Run("model download", checkModelDownload)
	t.Run("model leftovers", checkModelLeftovers)
}

func checkResampler(t *testing.T) {
	for _, rate := range []uint32{8_000, 16_000, 22_050, 44_100, 48_000, 96_000} {
		input := make([]float32, rate)
		for index := range input {
			input[index] = 0.25
		}
		audio, err := speech.Normalize(input, rate)
		if err != nil || len(audio) != speech.SampleRate {
			t.Fatalf("%d Hz: %d samples, %v", rate, len(audio), err)
		}
		for index, sample := range audio {
			if math.Abs(float64(sample)-0.25) > 1e-6 {
				t.Fatalf("%d Hz: sample %d = %v", rate, index, sample)
			}
		}
	}
	audio, err := speech.Normalize([]float32{0.5, 0.5, 0.5, -0.25, -0.25, -0.25}, 48_000)
	if err != nil || len(audio) != 2 || audio[0] != 0.5 || audio[1] != -0.25 {
		t.Fatalf("48 kHz averages = %v, %v", audio, err)
	}
	// Each output averages the inputs of its period; upsampling repeats.
	var down, up []float32
	sampler := speech.NewSampler(32_000)
	for _, sample := range []float32{1, 0, 0.5, -0.5, 4, float32(math.NaN())} {
		sampler.Push(sample, func(value float32) { down = append(down, value) })
	}
	if len(down) != 3 || down[0] != 0.5 || down[1] != 0 || down[2] != 0.5 {
		t.Fatalf("32 kHz = %v", down)
	}
	sampler = speech.NewSampler(8_000)
	for _, sample := range []float32{0.25, -2} {
		sampler.Push(sample, func(value float32) { up = append(up, value) })
	}
	if len(up) != 4 || up[0] != 0.25 || up[1] != 0.25 || up[2] != -1 || up[3] != -1 {
		t.Fatalf("8 kHz = %v", up)
	}
}

func checkAudioBounds(t *testing.T) {
	second := make([]float32, speech.SampleRate)
	for _, invalid := range []struct {
		name    string
		samples []float32
		rate    uint32
	}{
		{"no rate", second, 0},
		{"low rate", second, 7_999},
		{"high rate", second, 192_001},
		{"not a number", []float32{0, float32(math.NaN())}, speech.SampleRate},
		{"infinite", []float32{float32(math.Inf(-1))}, 48_000},
		{"too long", make([]float32, 8_000*300+1), 8_000},
	} {
		if _, err := speech.Normalize(invalid.samples, invalid.rate); !errors.Is(err, speech.ErrInvalidAudio) {
			t.Fatalf("%s: %v", invalid.name, err)
		}
	}
	if audio, err := speech.Normalize(make([]float32, 8_000*300), 8_000); err != nil || len(audio) != speech.MaxSamples {
		t.Fatalf("five minutes = %d samples, %v", len(audio), err)
	}
	if !speech.Silent(nil) || !speech.Silent(second) || !speech.Silent(make([]float32, speech.SampleRate/5-1)) {
		t.Fatal("silence is taken for speech")
	}
	second[len(second)-1] = -0.01
	if speech.Silent(second) {
		t.Fatal("a sample above the noise floor is silence")
	}
	if short := []float32{0.5, 0.5}; !speech.Silent(short) {
		t.Fatal("audio shorter than a fifth of a second is decoded")
	}
}

func checkLanguage(t *testing.T) {
	for preference, expected := range map[string]struct {
		language speech.Language
		code     string
	}{
		"auto": {speech.Auto, "auto"}, "chinese": {speech.Chinese, "zh"}, "english": {speech.English, "en"},
		"": {speech.Auto, "auto"}, "zh": {speech.Auto, "auto"}, "English": {speech.Auto, "auto"},
	} {
		if language := speech.ParseLanguage(preference); language != expected.language || language.Code() != expected.code {
			t.Fatalf("%q = %v (%s)", preference, language, language.Code())
		}
	}
	if speech.Languages != [...]string{"auto", "chinese", "english"} {
		t.Fatalf("languages = %v", speech.Languages)
	}
}

// A directory without the pinned libraries leaves the runtime unavailable
// and nothing is opened.
func checkRuntimeAbsent(t *testing.T) {
	directory := t.TempDir()
	if err := speech.Load(directory); !errors.Is(err, speech.ErrUnavailable) {
		t.Fatalf("empty directory: %v", err)
	}
	if _, err := speech.NewRecognizer(filepath.Join(directory, "model.onnx"), filepath.Join(directory, "tokens.txt"), speech.Auto); !errors.Is(err, speech.ErrUnavailable) {
		t.Fatalf("recognizer without a runtime: %v", err)
	}
}

const revision = "0123456789abcdef0123456789abcdef01234567"

func definition(repository string, graph, tokens []byte) protocol.ModelsDefinition {
	source := protocol.ModelsSource{Hub: protocol.ModelsHubHuggingFace, Repository: repository, Revision: revision}
	asset := func(name string, content []byte) protocol.ModelsAsset {
		sum := sha256.Sum256(content)
		return protocol.ModelsAsset{Source: source, Path: name, Name: name, Bytes: uint64(len(content)), Sha256: hex.EncodeToString(sum[:])}
	}
	return protocol.ModelsDefinition{
		Source: source, Name: "SenseVoice", License: "apache-2.0",
		Format: protocol.ModelsFormat{Kind: "sense_voice", Data: &protocol.ModelsFormatSenseVoicePayload{}},
		Assets: []protocol.ModelsAsset{asset("model.onnx", graph), asset("tokens.txt", tokens)},
	}
}

func checkModelIndex(t *testing.T) {
	directory := t.TempDir()
	manager, err := models.Open(t.Context(), models.Options{Directory: directory})
	if err != nil || len(manager.Entries()) != 0 {
		t.Fatalf("empty index: %v, %v", manager, err)
	}
	if _, known := manager.RecommendedState(); known {
		t.Fatal("the recommended model has a state before it was observed")
	}
	first := definition("org/voice", []byte("graph"), []byte("tokens"))
	id, err := manager.Register(first)
	if err != nil {
		t.Fatal(err)
	}
	if again, err := manager.Register(first); err != nil || again != id {
		t.Fatalf("registering twice = %v, %v", again, err)
	}
	if _, err := manager.Register(definition("org/voice", []byte("other"), []byte("tokens"))); !errors.Is(err, models.ErrConflict) {
		t.Fatalf("same source, other files: %v", err)
	}
	embedding := first
	embedding.Format = protocol.ModelsFormat{Kind: "decision"}
	if _, err := manager.Register(embedding); !errors.Is(err, speech.ErrInvalidModel) {
		t.Fatalf("another model kind: %v", err)
	}
	unpinned := definition("org/unpinned", []byte("graph"), []byte("tokens"))
	unpinned.Assets[0].Sha256 = "abc"
	if _, err := manager.Register(unpinned); !errors.Is(err, speech.ErrInvalidModel) {
		t.Fatalf("unpinned asset: %v", err)
	}
	second, err := manager.Register(definition("org/second", []byte("graph"), []byte("tokens")))
	if err != nil || second == id {
		t.Fatalf("second model = %v, %v", second, err)
	}

	// The index is models/speech.json and survives a restart; a model is
	// not installed until its files were verified.
	if _, err := os.Stat(filepath.Join(directory, "models", "speech.json")); err != nil {
		t.Fatal(err)
	}
	manager, err = models.Open(t.Context(), models.Options{Directory: directory})
	if err != nil {
		t.Fatal(err)
	}
	entries := manager.Entries()
	if len(entries) != 2 || entries[0].Record.ID != id || entries[1].Record.ID != second || entries[0].Record.Installed || entries[0].State.Status != models.Missing {
		t.Fatalf("reopened index = %+v", entries)
	}
	if _, err := manager.Capture(&id); !errors.Is(err, models.ErrMissing) {
		t.Fatalf("leasing a model that is not installed: %v", err)
	}
	unknown := protocol.NewModelID()
	if err := manager.Start(&unknown, nil); !errors.Is(err, models.ErrMissing) {
		t.Fatalf("downloading an unregistered model: %v", err)
	}
	if err := manager.Remove(unknown); !errors.Is(err, models.ErrMissing) {
		t.Fatalf("removing an unregistered model: %v", err)
	}
	if err := manager.Remove(second); err != nil {
		t.Fatal(err)
	}
	manager, err = models.Open(t.Context(), models.Options{Directory: directory})
	if err != nil || len(manager.Entries()) != 1 {
		t.Fatalf("index after a removal: %v", err)
	}

	// The recommended model is not in the index: its lease is given
	// before its files are known and reports them missing.
	lease, err := manager.Capture(nil)
	if err != nil || lease.Ready() {
		t.Fatalf("recommended lease: %v", err)
	}
	graph, tokens := lease.Paths()
	root := filepath.Join(directory, "models", "sensevoice-int8")
	if graph != filepath.Join(root, "model.int8.onnx") || tokens != filepath.Join(root, "tokens.txt") {
		t.Fatalf("recommended paths = %s, %s", graph, tokens)
	}
	if err := manager.Start(nil, nil); !errors.Is(err, models.ErrBusy) {
		t.Fatalf("downloading a model in use: %v", err)
	}
	lease.Release()
	lease.Release()
	manager.Observe(nil, true)
	if state, known := manager.RecommendedState(); !known || state.Status != models.Installed {
		t.Fatalf("observed state = %+v, %v", state, known)
	}
	manager.Observe(nil, false)
	if state, _ := manager.RecommendedState(); state.Status != models.Missing {
		t.Fatalf("observed state = %+v", state)
	}

	for name, content := range map[string]string{"corrupt": "{", "foreign": `[{"id":"` + id.String() + `","definition":{},"installed":true}]`} {
		broken := t.TempDir()
		if err := os.MkdirAll(filepath.Join(broken, "models"), 0o700); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(filepath.Join(broken, "models", "speech.json"), []byte(content), 0o600); err != nil {
			t.Fatal(err)
		}
		if _, err := models.Open(t.Context(), models.Options{Directory: broken}); !errors.Is(err, models.ErrStorage) {
			t.Fatalf("%s index: %v", name, err)
		}
	}
}

func checkModelDownload(t *testing.T) {
	graph, tokens := []byte(strings.Repeat("graph", 4096)), []byte("<unk> 0\n")
	var (
		mu      sync.Mutex
		corrupt bool
		hold    chan struct{}
		held    = make(chan struct{}, 4)
	)
	server := httptest.NewTLSServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		mu.Lock()
		damaged, wait := corrupt, hold
		mu.Unlock()
		if wait != nil {
			held <- struct{}{}
			select {
			case <-wait:
			case <-r.Context().Done():
				return
			}
		}
		content := map[string][]byte{
			"/org/voice/resolve/" + revision + "/model.onnx": graph,
			"/org/voice/resolve/" + revision + "/tokens.txt": tokens,
		}[r.URL.Path]
		if content == nil {
			http.NotFound(w, r)
			return
		}
		if damaged {
			content = append([]byte("x"), content[1:]...)
		}
		_, _ = w.Write(content)
	}))
	defer server.Close()

	directory := t.TempDir()
	changes := make(chan struct{}, 1024)
	var validated []string
	options := models.Options{
		Directory: directory, Transport: server.Client().Transport,
		Validate: func(graph, tokens string) error {
			validated = append(validated, graph, tokens)
			return nil
		},
		Changed: func() {
			select {
			case changes <- struct{}{}:
			default:
			}
		},
	}
	manager, err := models.Open(t.Context(), options)
	if err != nil {
		t.Fatal(err)
	}
	id, err := manager.Register(definition("org/voice", graph, tokens))
	if err != nil {
		t.Fatal(err)
	}
	state := func() models.State { return manager.Entries()[0].State }
	root := filepath.Join(directory, "models", "speech", id.String())

	// A file that does not match its pinned hash is not installed.
	mu.Lock()
	corrupt = true
	mu.Unlock()
	if err := manager.Start(&id, &server.URL); err != nil {
		t.Fatal(err)
	}
	manager.Wait()
	if got := state(); got.Status != models.Failed || !errors.Is(got.Err, assets.ErrInvalidAsset) || manager.Entries()[0].Record.Installed || validated != nil {
		t.Fatalf("corrupt download = %+v", got)
	}

	// A cancelled download is missing again and can be started again.
	mu.Lock()
	corrupt, hold = false, make(chan struct{})
	mu.Unlock()
	if err := manager.Start(&id, &server.URL); err != nil {
		t.Fatal(err)
	}
	if err := manager.Start(&id, &server.URL); err != nil {
		t.Fatalf("starting a running download: %v", err)
	}
	<-held
	if got := state(); got.Status != models.Downloading || manager.Cancelling(&id) {
		t.Fatalf("running download = %+v", got)
	}
	if err := manager.Remove(id); !errors.Is(err, models.ErrBusy) {
		t.Fatalf("removing a downloading model: %v", err)
	}
	if _, err := manager.Capture(&id); !errors.Is(err, models.ErrMissing) {
		t.Fatalf("leasing a downloading model: %v", err)
	}
	manager.Cancel(&id)
	if !manager.Cancelling(&id) {
		t.Fatal("the cancellation is not reported")
	}
	manager.Wait()
	if got := state(); got.Status != models.Missing || got.Err != nil || manager.Cancelling(&id) || validated != nil {
		t.Fatalf("cancelled download = %+v", got)
	}

	// The verified download is validated, installed and indexed.
	mu.Lock()
	hold = nil
	mu.Unlock()
	if err := manager.Start(&id, &server.URL); err != nil {
		t.Fatal(err)
	}
	manager.Wait()
	if got := state(); got.Status != models.Installed || !manager.Entries()[0].Record.Installed {
		t.Fatalf("download = %+v", got)
	}
	if len(validated) != 2 || validated[0] != filepath.Join(root, "model.onnx") || validated[1] != filepath.Join(root, "tokens.txt") {
		t.Fatalf("validated %v", validated)
	}
	if stored, err := os.ReadFile(validated[0]); err != nil || string(stored) != string(graph) {
		t.Fatalf("stored graph: %v", err)
	}
	if len(changes) == 0 {
		t.Fatal("no change was announced")
	}

	// The installation left the receipt that spares a dictation the hash
	// of the whole model.
	for _, name := range []string{"validated", ".verified"} {
		if _, err := os.Stat(filepath.Join(root, name)); err != nil {
			t.Fatalf("no receipt after the installation: %v", err)
		}
	}

	// A restart finds it installed; a lease verifies the files again and
	// keeps the model from being removed.
	manager, err = models.Open(t.Context(), options)
	if err != nil || manager.Entries()[0].State.Status != models.Installed {
		t.Fatalf("reopened: %v", err)
	}
	lease, err := manager.Capture(&id)
	if err != nil || !lease.Ready() {
		t.Fatalf("lease: %v", err)
	}
	if err := manager.Remove(id); !errors.Is(err, models.ErrBusy) {
		t.Fatalf("removing a model in use: %v", err)
	}
	if err := os.WriteFile(filepath.Join(root, "tokens.txt"), []byte("<unk> 1\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	if lease.Ready() {
		t.Fatal("changed files are ready")
	}
	// The receipt covered the files as they were: restored files are hashed
	// again, and files without a receipt are hashed and get one.
	if err := os.WriteFile(filepath.Join(root, "tokens.txt"), tokens, 0o600); err != nil {
		t.Fatal(err)
	}
	if !lease.Ready() {
		t.Fatal("restored files are not ready")
	}
	if err := os.Remove(filepath.Join(root, ".verified")); err != nil {
		t.Fatal(err)
	}
	if !lease.Ready() {
		t.Fatal("files without a receipt are not ready")
	}
	if _, err := os.Stat(filepath.Join(root, ".verified")); err != nil {
		t.Fatalf("the hash left no receipt: %v", err)
	}
	lease.Release()
	if err := manager.Remove(id); err != nil {
		t.Fatal(err)
	}
	if _, err := os.Stat(root); !errors.Is(err, os.ErrNotExist) || len(manager.Entries()) != 0 {
		t.Fatalf("removed model remains: %v", err)
	}

	// A model that fails validation is not installed, and a manager whose
	// context ended stops its download.
	options.Validate = func(string, string) error { return speech.ErrInvalidModel }
	ctx, cancel := context.WithCancel(t.Context())
	manager, err = models.Open(ctx, options)
	if err != nil {
		t.Fatal(err)
	}
	if id, err = manager.Register(definition("org/voice", graph, tokens)); err != nil {
		t.Fatal(err)
	}
	if err := manager.Start(&id, &server.URL); err != nil {
		t.Fatal(err)
	}
	manager.Wait()
	if got := state(); got.Status != models.Failed || !errors.Is(got.Err, speech.ErrInvalidModel) || manager.Entries()[0].Record.Installed {
		t.Fatalf("invalid model = %+v", got)
	}
	cancel()
	if err := manager.Start(&id, &server.URL); err != nil {
		t.Fatal(err)
	}
	manager.Wait()
	if got := state(); got.Status != models.Missing {
		t.Fatalf("download after shutdown = %+v", got)
	}
}

// checkModelLeftovers covers what a crash leaves under the models directory
// and the next Open clears.
func checkModelLeftovers(t *testing.T) {
	directory := t.TempDir()
	manager, err := models.Open(t.Context(), models.Options{Directory: directory})
	if err != nil {
		t.Fatal(err)
	}
	id, err := manager.Register(definition("org/voice", []byte("graph"), []byte("tokens")))
	if err != nil {
		t.Fatal(err)
	}
	root := filepath.Join(directory, "models")
	write := func(path string) {
		t.Helper()
		if err := os.MkdirAll(filepath.Dir(path), 0o700); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(path, []byte("left"), 0o600); err != nil {
			t.Fatal(err)
		}
	}
	// A removal parked the model's files and stopped before the index
	// without the model was saved.
	interrupted := filepath.Join(root, "speech", ".speech-delete-"+id.String())
	write(filepath.Join(interrupted, "model", "tokens.txt"))
	// A removal saved the index and stopped before the files were deleted.
	finished := filepath.Join(root, "speech", ".speech-delete-"+protocol.NewModelID().String())
	write(filepath.Join(finished, "model", "tokens.txt"))
	draft := filepath.Join(root, ".speech-123456.json")
	write(draft)
	stray := filepath.Join(root, "sensevoice-int8", ".other.bin-4-"+strings.Repeat("ab", 32)+".part")
	write(stray)
	temporary := filepath.Join(root, "speech", id.String()+"-unrelated")
	write(filepath.Join(temporary, "kept.txt"))

	manager, err = models.Open(t.Context(), models.Options{Directory: directory})
	if err != nil || len(manager.Entries()) != 1 {
		t.Fatalf("reopened index: %v", err)
	}
	if _, err := os.Stat(filepath.Join(root, "speech", id.String(), "tokens.txt")); err != nil {
		t.Fatalf("the listed model's files were not put back: %v", err)
	}
	for _, path := range []string{interrupted, finished, draft, stray} {
		if _, err := os.Lstat(path); !errors.Is(err, os.ErrNotExist) {
			t.Fatalf("%s stayed: %v", path, err)
		}
	}
	if _, err := os.Stat(filepath.Join(temporary, "kept.txt")); err != nil {
		t.Fatalf("an unrelated directory was touched: %v", err)
	}
	// A removal leaves no holder behind.
	if err := manager.Remove(id); err != nil {
		t.Fatal(err)
	}
	entries, err := os.ReadDir(filepath.Join(root, "speech"))
	if err != nil || len(entries) != 1 || entries[0].Name() != filepath.Base(temporary) {
		t.Fatalf("after the removal: %v, %v", entries, err)
	}
}
