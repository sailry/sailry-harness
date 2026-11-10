package decision

import (
	"unsafe"

	"github.com/ebitengine/purego"
)

// Build is the pinned llama.cpp release
// (d80619397c2ba5cf1366c29f7d43699ca3a159b9) whose public C symbols and
// by-value layouts are declared below.
const Build = "b11756"

// modelParams is llama_model_params (80 bytes).
type modelParams struct {
	devices             unsafe.Pointer
	tensorBuftOverrides unsafe.Pointer
	gpuLayers           int32
	splitMode           uint32
	loadMode            int32
	lazyMode            uint32
	mainGPU             int32
	tensorSplit         unsafe.Pointer
	progressCallback    uintptr
	progressUserData    unsafe.Pointer
	kvOverrides         unsafe.Pointer
	vocabOnly           uint8
	checkTensors        uint8
	useExtraBufts       uint8
	noHost              uint8
	noAlloc             uint8
	loadMTP             uint8
}

// contextParams is llama_context_params (168 bytes).
type contextParams struct {
	ctx, batch, ubatch, seqMax, rsSeq, outputsMax, outputsMaxPerSeq uint32
	threads, threadsBatch                                           int32
	contextType                                                     uint32
	ropeScaling, pooling, attention, flashAttention                 int32
	ropeFreqBase, ropeFreqScale                                     float32
	yarnExt, yarnAttn, yarnBetaFast, yarnBetaSlow                   float32
	yarnOrigCtx                                                     uint32
	defragThreshold                                                 float32
	evalCallback                                                    uintptr
	evalUserData                                                    unsafe.Pointer
	typeK, typeV                                                    uint32
	moeCacheSize                                                    uintptr
	abortCallback                                                   uintptr
	abortData                                                       unsafe.Pointer
	embeddings, offloadKQV, noPerf, opOffload, swaFull, kvUnified   uint8
	samplers                                                        unsafe.Pointer
	samplerCount                                                    uintptr
	other                                                           unsafe.Pointer
}

// batch is llama_batch (56 bytes).
type batch struct {
	count    int32
	token    *int32
	embd     *float32
	pos      *int32
	seqCount *int32
	seqID    **int32
	logits   *int8
}

const (
	splitModeNone       = 0 // LLAMA_SPLIT_MODE_NONE
	loadModeMmap        = 1 // LLAMA_LOAD_MODE_MMAP
	lazyModeOff         = 0 // LLAMA_LAZY_MODE_OFF
	poolingNone         = 0 // LLAMA_POOLING_TYPE_NONE
	attentionCausal     = 0 // LLAMA_ATTENTION_TYPE_CAUSAL
	attentionNonCausal  = 1 // LLAMA_ATTENTION_TYPE_NON_CAUSAL
	deviceCPU           = 0 // GGML_BACKEND_DEVICE_TYPE_CPU
	attributeUnknown    = 1 // LLAMA_TOKEN_ATTR_UNKNOWN
	attributeControl    = 8 // LLAMA_TOKEN_ATTR_CONTROL
	attributeLeftStrip  = 128
	attributeRightStrip = 256
)

// api is the subset of the llama.cpp and ggml C API the engine calls.
type api struct {
	modelDefaultParams   func() modelParams
	contextDefaultParams func() contextParams
	backendInit          func()
	modelLoadFromFile    func(path string, params modelParams) uintptr
	modelFree            func(model uintptr)
	initFromModel        func(model uintptr, params contextParams) uintptr
	free                 func(context uintptr)
	nCtx                 func(context uintptr) uint32
	getMemory            func(context uintptr) uintptr
	modelGetVocab        func(model uintptr) uintptr
	modelNCtxTrain       func(model uintptr) int32
	modelNEmbdOut        func(model uintptr) int32
	vocabNTokens         func(vocab uintptr) int32
	modelHasEncoder      func(model uintptr) bool
	memoryClear          func(memory uintptr, data bool)
	batchInit            func(tokens, embd, seqMax int32) batch
	batchFree            func(value batch)
	encode               func(context uintptr, value batch) int32
	decode               func(context uintptr, value batch) int32
	setAbortCallback     func(context uintptr, callback uintptr, data uintptr)
	synchronize          func(context uintptr)
	getLogitsIth         func(context uintptr, index int32) *float32
	getEmbeddingsIth     func(context uintptr, index int32) *float32
	vocabGetAttr         func(vocab uintptr, token int32) uint32
	vocabSep             func(vocab uintptr) int32
	vocabMask            func(vocab uintptr) int32
	tokenize             func(vocab uintptr, text *byte, length int32, tokens *int32, capacity int32, addSpecial, parseSpecial bool) int32
	tokenToPiece         func(vocab uintptr, token int32, buffer *byte, length int32, lstrip int32, special bool) int32
	llamaLogSet          func(callback uintptr, data uintptr)
	ggmlLogSet           func(callback uintptr, data uintptr)
	backendDevByType     func(kind uint32) uintptr
	backendLoad          func(path string) uintptr
}

// bind fills the function table from the base, registry and entry images.
func (a *api) bind(base, registry, llama uintptr) error {
	if unsafe.Sizeof(modelParams{}) != 80 || unsafe.Sizeof(contextParams{}) != 168 || unsafe.Sizeof(batch{}) != 56 {
		return unavailable("by-value layouts do not match " + Build)
	}
	var failure error
	register := func(handle uintptr, target any, name string) {
		symbol, err := librarySymbol(handle, name)
		if err != nil {
			if failure == nil {
				failure = &Error{Kind: Library, Message: err.Error()}
			}
			return
		}
		purego.RegisterFunc(target, symbol)
	}
	register(base, &a.ggmlLogSet, "ggml_log_set")
	register(registry, &a.backendDevByType, "ggml_backend_dev_by_type")
	register(registry, &a.backendLoad, "ggml_backend_load")
	register(llama, &a.modelDefaultParams, "llama_model_default_params")
	register(llama, &a.contextDefaultParams, "llama_context_default_params")
	register(llama, &a.backendInit, "llama_backend_init")
	register(llama, &a.modelLoadFromFile, "llama_model_load_from_file")
	register(llama, &a.modelFree, "llama_model_free")
	register(llama, &a.initFromModel, "llama_init_from_model")
	register(llama, &a.free, "llama_free")
	register(llama, &a.nCtx, "llama_n_ctx")
	register(llama, &a.getMemory, "llama_get_memory")
	register(llama, &a.modelGetVocab, "llama_model_get_vocab")
	register(llama, &a.modelNCtxTrain, "llama_model_n_ctx_train")
	register(llama, &a.modelNEmbdOut, "llama_model_n_embd_out")
	register(llama, &a.vocabNTokens, "llama_vocab_n_tokens")
	register(llama, &a.modelHasEncoder, "llama_model_has_encoder")
	register(llama, &a.memoryClear, "llama_memory_clear")
	register(llama, &a.batchInit, "llama_batch_init")
	register(llama, &a.batchFree, "llama_batch_free")
	register(llama, &a.encode, "llama_encode")
	register(llama, &a.decode, "llama_decode")
	register(llama, &a.setAbortCallback, "llama_set_abort_callback")
	register(llama, &a.synchronize, "llama_synchronize")
	register(llama, &a.getLogitsIth, "llama_get_logits_ith")
	register(llama, &a.getEmbeddingsIth, "llama_get_embeddings_ith")
	register(llama, &a.vocabGetAttr, "llama_vocab_get_attr")
	register(llama, &a.vocabSep, "llama_vocab_sep")
	register(llama, &a.vocabMask, "llama_vocab_mask")
	register(llama, &a.tokenize, "llama_tokenize")
	register(llama, &a.tokenToPiece, "llama_token_to_piece")
	register(llama, &a.llamaLogSet, "llama_log_set")
	return failure
}
