package decision

import (
	"context"
	"maps"
	"path/filepath"
	"runtime"
	"slices"
	"strings"
	"sync"
	"sync/atomic"
	"unicode/utf8"
	"unsafe"

	"github.com/ebitengine/purego"

	"github.com/sailry/sailry-harness/internal/protocol"
)

// Engine owns one loaded decision model. Calls are serialized.
type Engine struct {
	mutex     sync.Mutex
	api       *api
	model     uintptr
	vocab     uintptr
	metadata  *Metadata
	templates *templates
	marker    int32
	separator int32
	labels    []int32
}

// check is the cancellation state of one running request.
type check struct {
	stopped func() bool
	aborted atomic.Bool
}

// aborts routes the one native abort callback of the process to running
// requests. Callbacks are never freed, so none is created per request.
var aborts struct {
	sync.Mutex
	next   uintptr
	checks map[uintptr]*check
}

var abort = sync.OnceValue(func() uintptr {
	return purego.NewCallback(func(data uintptr) uintptr {
		aborts.Lock()
		running := aborts.checks[data]
		aborts.Unlock()
		if running == nil || !running.stopped() {
			return 0
		}
		running.aborted.Store(true)
		return 1
	})
})

type prompt struct {
	query   *query
	tokens  []int32
	markers []int
}

// Load validates and loads the single model.gguf of an installed model.
// Request contexts are sized to the input. Installation sets validating to
// scan all tensor data before publication.
func Load(definition *protocol.ModelsDefinition, directory string, validating bool) (*Engine, error) {
	if len(definition.Assets) != 1 || definition.Assets[0].Name != "model.gguf" {
		return nil, invalid("decision model requires model.gguf")
	}
	return open(filepath.Join(directory, "model.gguf"), validating, initialize)
}

func open(path string, validating bool, start func() (*backend, error)) (*Engine, error) {
	metadata, err := Validate(path)
	if err != nil {
		return nil, err
	}
	templates, err := newTemplates(metadata.template)
	if err != nil {
		return nil, err
	}
	loaded, err := start()
	if err != nil {
		return nil, err
	}
	if strings.ContainsRune(path, 0) {
		return nil, invalid("model path contains a nul byte")
	}
	api := &loaded.api
	params := api.modelDefaultParams()
	// This list selects offload backends, not CPU execution. An explicit
	// empty list matches --device none and keeps the native CPU order.
	devices := new(uintptr)
	var pinner runtime.Pinner
	pinner.Pin(devices)
	defer pinner.Unpin()
	params.devices = unsafe.Pointer(devices)
	params.gpuLayers = 0
	params.splitMode = splitModeNone
	params.loadMode = loadModeMmap
	params.lazyMode = lazyModeOff
	params.useExtraBufts = 0
	params.checkTensors = 0
	if validating {
		params.checkTensors = 1
	}
	model := api.modelLoadFromFile(path, params)
	if model == 0 {
		return nil, native("model could not be loaded")
	}
	engine := &Engine{api: api, model: model, metadata: metadata, templates: templates, marker: -1, separator: -1}
	if err := engine.describe(); err != nil {
		engine.Close()
		return nil, err
	}
	return engine, nil
}

// describe reads the vocabulary properties the layout depends on.
func (e *Engine) describe() error {
	if trained := e.api.modelNCtxTrain(e.model); trained <= 0 || int(trained) != e.metadata.Context {
		return invalid("native context length differs from GGUF metadata")
	}
	if e.vocab = e.api.modelGetVocab(e.model); e.vocab == 0 {
		return native("model vocabulary is missing")
	}
	if e.metadata.Algorithm == Laya {
		e.marker, e.separator = e.api.vocabMask(e.vocab), e.api.vocabSep(e.vocab)
		if e.marker < 0 || e.separator < 0 {
			return invalid("laya model has no mask or separator token")
		}
		if e.api.modelNEmbdOut(e.model) < 3 {
			return invalid("laya decision head has fewer than three columns")
		}
		return nil
	}
	for _, label := range "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz" {
		tokens, err := e.tokenize(string(label), false)
		if err != nil {
			return err
		}
		if len(tokens) != 1 {
			return invalid("openjev label is not a single token")
		}
		e.labels = append(e.labels, tokens[0])
	}
	return nil
}

// Close frees the model. The runtime stays loaded for the process.
func (e *Engine) Close() {
	e.mutex.Lock()
	defer e.mutex.Unlock()
	if e.model != 0 {
		e.api.modelFree(e.model)
		e.model = 0
	}
}

// Decide runs one forward pass per question and generates no text.
// Cancelling ctx stops the request between and inside forward passes.
func (e *Engine) Decide(ctx context.Context, request *protocol.DecisionRequest) (*protocol.DecisionResponse, error) {
	if ValidateRequest(request) != nil {
		return nil, invalid("invalid decision request")
	}
	stopped := func() bool { return ctx.Err() != nil }
	if stopped() {
		return nil, cancelled()
	}
	e.mutex.Lock()
	defer e.mutex.Unlock()
	if e.model == 0 {
		return nil, native("decision model is closed")
	}
	prompts := make([]*prompt, 0, len(request.Questions))
	capacity := 0
	for _, name := range slices.Sorted(maps.Keys(request.Questions)) {
		prepared, err := e.prompt(request.State, name, request.Questions[name], stopped)
		if err != nil {
			return nil, err
		}
		prompts = append(prompts, prepared)
		capacity = max(capacity, len(prepared.tokens))
		if stopped() {
			return nil, cancelled()
		}
	}
	handle, err := e.context(capacity)
	if err != nil {
		return nil, err
	}
	defer e.api.free(handle)
	running, release := e.watch(handle, stopped)
	defer release()
	response := &protocol.DecisionResponse{Answers: make(map[string]protocol.DecisionAnswer, len(prompts))}
	for _, prepared := range prompts {
		if stopped() {
			return nil, cancelled()
		}
		scores, err := e.forward(handle, prepared, running)
		if err != nil {
			return nil, err
		}
		answer, err := prepared.query.answer(e.metadata, scores)
		if err != nil {
			return nil, err
		}
		response.Answers[prepared.query.name] = answer
		response.Usage.InputTokens += uint64(len(prepared.tokens))
	}
	return response, nil
}

// watch installs the abort callback for one context. The returned function
// removes it before its state and the context are released.
func (e *Engine) watch(handle uintptr, stopped func() bool) (*check, func()) {
	running := &check{stopped: stopped}
	aborts.Lock()
	aborts.next++
	identity := aborts.next
	if aborts.checks == nil {
		aborts.checks = map[uintptr]*check{}
	}
	aborts.checks[identity] = running
	aborts.Unlock()
	e.api.setAbortCallback(handle, abort(), identity)
	return running, func() {
		e.api.setAbortCallback(handle, 0, 0)
		aborts.Lock()
		delete(aborts.checks, identity)
		aborts.Unlock()
	}
}

func (e *Engine) prompt(state, name string, question protocol.DecisionQuestion, stopped func() bool) (*prompt, error) {
	prepared := &prompt{query: newQuery(name, question, e.metadata.Algorithm)}
	text, err := e.templates.render(state, prepared.query, stopped)
	if err != nil {
		return nil, err
	}
	if prepared.tokens, err = e.encode(text); err != nil {
		return nil, err
	}
	if e.metadata.Algorithm == Laya {
		prepared.tokens, prepared.markers, err = laya(prepared.tokens, len(prepared.query.options), e.marker, e.separator, e.metadata.head)
		if err != nil {
			return nil, err
		}
	}
	if len(prepared.tokens) == 0 || len(prepared.tokens) > e.metadata.Context {
		return nil, invalid("decision prompt exceeds the trained context")
	}
	return prepared, nil
}

// encode tokenizes template text with control tokens and request values
// without them.
func (e *Engine) encode(prompt *rendered) ([]int32, error) {
	type control struct {
		start, end int
		token      int32
		attributes uint32
	}
	var controls []control
	for _, trusted := range prompt.trusted() {
		start := trusted[0]
		found, err := e.tokenize(prompt.text[trusted[0]:trusted[1]], true)
		if err != nil {
			return nil, err
		}
		for _, token := range found {
			attributes := e.api.vocabGetAttr(e.vocab, token)
			if attributes&(attributeControl|attributeUnknown) == 0 {
				continue
			}
			piece, err := e.piece(token)
			if err != nil {
				return nil, err
			}
			offset := strings.Index(prompt.text[start:trusted[1]], piece)
			if offset < 0 {
				// Unknown text can produce the unknown token without
				// spelling the vocabulary's special token in the template.
				if attributes&attributeUnknown != 0 {
					continue
				}
				return nil, invalid("decision control token is missing")
			}
			position := start + offset
			start = position + len(piece)
			controls = append(controls, control{position, start, token, attributes})
		}
	}
	var tokens []int32
	start := 0
	// Ordinary template text and values merge across interpolation
	// boundaries; only native controls split the tokenizer's context.
	for _, item := range controls {
		from, to := prompt.strip(item.start, item.end, item.attributes&attributeLeftStrip != 0, item.attributes&attributeRightStrip != 0)
		if start < from {
			more, err := e.tokenize(prompt.text[start:from], false)
			if err != nil {
				return nil, err
			}
			tokens = append(tokens, more...)
		}
		tokens = append(tokens, item.token)
		start = to
	}
	if start < len(prompt.text) {
		more, err := e.tokenize(prompt.text[start:], false)
		if err != nil {
			return nil, err
		}
		tokens = append(tokens, more...)
	}
	return tokens, nil
}

func (e *Engine) tokenize(text string, special bool) ([]int32, error) {
	if len(text) > 1<<31-1 {
		return nil, invalid("decision prompt exceeds its limit")
	}
	if text == "" {
		return nil, nil
	}
	// A zero-capacity call reports the required length without writing.
	data := unsafe.StringData(text)
	count := e.api.tokenize(e.vocab, data, int32(len(text)), nil, 0, false, special)
	if count == 0 {
		return nil, nil
	}
	if count > 0 || count < -1_048_576 {
		return nil, invalid("decision prompt could not be tokenized")
	}
	tokens := make([]int32, -count)
	if e.api.tokenize(e.vocab, data, int32(len(text)), &tokens[0], -count, false, special) != -count {
		return nil, native("token count changed during tokenization")
	}
	return tokens, nil
}

func (e *Engine) piece(token int32) (string, error) {
	buffer := make([]byte, 256)
	written := e.api.tokenToPiece(e.vocab, token, &buffer[0], int32(len(buffer)), 0, true)
	if written <= 0 || int(written) > len(buffer) {
		return "", invalid("decision marker is invalid")
	}
	if !utf8.Valid(buffer[:written]) {
		return "", invalid("decision marker is not UTF-8")
	}
	return string(buffer[:written]), nil
}

// context creates a CPU context sized to the longest prompt.
func (e *Engine) context(capacity int) (uintptr, error) {
	laya := e.metadata.Algorithm == Laya
	params := e.api.contextDefaultParams()
	params.ctx = uint32(max(capacity, 32))
	params.batch = uint32(capacity)
	params.seqMax = 1
	params.ubatch, params.outputsMax = uint32(min(capacity, 512)), 1
	params.attention = attentionCausal
	params.embeddings = 0
	if laya {
		// The noncausal encode path reserves every token's output even
		// though only the marker positions are read.
		params.ubatch, params.outputsMax = uint32(capacity), uint32(capacity)
		params.attention = attentionNonCausal
		params.embeddings = 1
	}
	params.outputsMaxPerSeq = params.outputsMax
	threads := int32(min(max(runtime.NumCPU()/2, 1), 4))
	params.threads, params.threadsBatch = threads, threads
	params.pooling = poolingNone
	params.offloadKQV, params.opOffload, params.noPerf = 0, 0, 1
	handle := e.api.initFromModel(e.model, params)
	if handle == 0 {
		return 0, native("context could not be created")
	}
	// The allocated context may include native padding.
	if e.api.nCtx(handle) < uint32(capacity) {
		e.api.free(handle)
		return 0, native("allocated context is too short")
	}
	return handle, nil
}

func (e *Engine) forward(handle uintptr, prompt *prompt, running *check) ([]float32, error) {
	api := e.api
	// Encoder contexts can have no memory module to clear.
	if memory := api.getMemory(handle); memory != 0 {
		api.memoryClear(memory, true)
	}
	laya := len(prompt.markers) != 0
	encoder := api.modelHasEncoder(e.model)
	chunk := len(prompt.tokens)
	if !laya {
		chunk = min(chunk, 512)
	}
	value := api.batchInit(int32(chunk), 0, 1)
	defer api.batchFree(value)
	if value.token == nil || value.pos == nil || value.seqCount == nil || value.seqID == nil || value.logits == nil {
		return nil, native("batch could not be allocated")
	}
	tokens, positions := unsafe.Slice(value.token, chunk), unsafe.Slice(value.pos, chunk)
	counts, sequences := unsafe.Slice(value.seqCount, chunk), unsafe.Slice(value.seqID, chunk)
	outputs := unsafe.Slice(value.logits, chunk)
	for start := 0; start < len(prompt.tokens); start += chunk {
		part := prompt.tokens[start:min(start+chunk, len(prompt.tokens))]
		value.count = int32(len(part))
		for offset, token := range part {
			tokens[offset], positions[offset], counts[offset] = token, int32(start+offset), 1
			*sequences[offset] = 0
			outputs[offset] = 0
			if laya || start+offset+1 == len(prompt.tokens) {
				outputs[offset] = 1
			}
		}
		if running.stopped() {
			return nil, cancelled()
		}
		var status int32
		if encoder {
			status = api.encode(handle, value)
		} else {
			status = api.decode(handle, value)
		}
		// Finish any backend work while the batch is still alive.
		api.synchronize(handle)
		if status == 2 || running.aborted.Load() || running.stopped() {
			return nil, cancelled()
		}
		if status != 0 {
			return nil, native("forward pass failed")
		}
	}
	if laya {
		scores := make([]float32, len(prompt.markers))
		for index, marker := range prompt.markers {
			values := api.getEmbeddingsIth(handle, int32(marker))
			if values == nil {
				return nil, native("decision embedding is missing")
			}
			// The column was checked against the head width at load.
			scores[index] = unsafe.Slice(values, 3)[prompt.query.column()]
		}
		return scores, nil
	}
	// Only the final token requested logits; -1 selects that output.
	logits := api.getLogitsIth(handle, -1)
	if logits == nil {
		return nil, native("decision logits are missing")
	}
	vocabulary := api.vocabNTokens(e.vocab)
	labels := e.labels[:min(len(e.labels), len(prompt.query.options))]
	scores := make([]float32, len(labels))
	for index, label := range labels {
		if label < 0 || label >= vocabulary {
			return nil, invalid("decision label is outside the vocabulary")
		}
		scores[index] = unsafe.Slice(logits, vocabulary)[label]
	}
	return scores, nil
}
