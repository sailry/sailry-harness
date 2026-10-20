use crate::{
    Algorithm, Error, Metadata, Result, ffi,
    inference::{self, Query, Templates},
    runtime::{self, Runtime},
};
use sailry_protocol::{
    decision::{Question, Request, Response, Usage},
    models::Definition,
};
use std::{
    collections::BTreeMap,
    ffi::{CString, c_void},
    path::Path,
    ptr::NonNull,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

pub struct Engine {
    model: Model,
    metadata: Metadata,
    templates: Templates,
    marker: i32,
    separator: i32,
    marker_text: String,
    labels: Vec<i32>,
}

struct Model {
    handle: NonNull<ffi::llama_model>,
    runtime: Arc<Runtime>,
}
// SAFETY: this CPU model has one Engine owner. Context creation and inference are
// serialized through &mut Engine; libllama permits CPU use from another thread.
unsafe impl Send for Model {}

impl Drop for Model {
    fn drop(&mut self) {
        // SAFETY: this is the unique owner; all borrowed contexts have been dropped.
        unsafe {
            self.runtime.api.llama_model_free(self.handle.as_ptr());
        }
    }
}

struct Context<'a> {
    handle: NonNull<ffi::llama_context>,
    api: &'a ffi::Llama,
}
impl Drop for Context<'_> {
    fn drop(&mut self) {
        // SAFETY: this is the unique context owner, after synchronous evaluation.
        unsafe {
            self.api.llama_free(self.handle.as_ptr());
        }
    }
}

struct Batch<'a> {
    value: ffi::llama_batch,
    api: &'a ffi::Llama,
}
impl Drop for Batch<'_> {
    fn drop(&mut self) {
        // SAFETY: llama_batch_init allocated each buffer; this owner frees them once.
        unsafe {
            self.api.llama_batch_free(self.value);
        }
    }
}

struct Check<F> {
    cancelled: F,
    panicked: AtomicBool,
    aborted: AtomicBool,
}
struct Abort<'a, F: Fn() -> bool + Sync> {
    context: &'a Context<'a>,
    check: Box<Check<F>>,
}
impl<F: Fn() -> bool + Sync> Drop for Abort<'_, F> {
    fn drop(&mut self) {
        // SAFETY: native forward calls have returned; remove the callback before
        // freeing its borrowed state or the context, including during unwinding.
        unsafe {
            self.context.api.llama_set_abort_callback(
                self.context.handle.as_ptr(),
                None,
                std::ptr::null_mut(),
            );
        }
    }
}
unsafe extern "C" fn abort<F: Fn() -> bool + Sync>(data: *mut c_void) -> bool {
    // SAFETY: Abort owns this exact boxed Check until all synchronous calls return.
    let check = unsafe { &*data.cast::<Check<F>>() };
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| (check.cancelled)())) {
        Ok(cancelled) => {
            if cancelled {
                check.aborted.store(true, Ordering::Relaxed);
            }
            cancelled
        }
        Err(_) => {
            check.panicked.store(true, Ordering::Relaxed);
            true
        }
    }
}

struct Prompt<'a> {
    query: Query<'a>,
    tokens: Vec<i32>,
    markers: Vec<usize>,
}

impl Engine {
    /// Loads verified model weights once. Request contexts are sized to the input.
    pub fn load(definition: &Definition, directory: &Path) -> Result<Self> {
        if definition.assets.len() != 1 || definition.assets[0].name != "model.gguf" {
            return Err(Error::Invalid("decision model requires model.gguf"));
        }
        Self::open(&directory.join("model.gguf"))
    }

    fn open(path: &Path) -> Result<Self> {
        let metadata = crate::validate(path)?;
        let templates = Templates::new(&metadata.template)?;
        let runtime = runtime::initialize()?;
        let path = CString::new(path.as_os_str().as_encoded_bytes())
            .map_err(|_| Error::Invalid("model path contains a nul byte"))?;
        // SAFETY: the by-value layout and function table come from the same locked tag.
        let mut params = unsafe { runtime.api.llama_model_default_params() };
        // This list selects offload backends, not CPU execution. An explicit
        // empty list matches --device none and retains the native BLAS/CPU order.
        let mut devices = [std::ptr::null_mut()];
        params.devices = devices.as_mut_ptr();
        params.n_gpu_layers = 0;
        params.split_mode = ffi::llama_split_mode_LLAMA_SPLIT_MODE_NONE;
        params.load_mode = ffi::llama_load_mode_LLAMA_LOAD_MODE_MMAP;
        params.lazy_mode = ffi::llama_lazy_mode_LLAMA_LAZY_MODE_OFF;
        params.use_extra_bufts = false;
        params.check_tensors = true;
        // SAFETY: all pointers outlive this synchronous load; the GGUF was bounded
        // and structurally validated before entering native parsing.
        let handle = unsafe {
            runtime
                .api
                .llama_model_load_from_file(path.as_ptr(), params)
        };
        let model = Model {
            handle: NonNull::new(handle).ok_or(Error::Native("model could not be loaded"))?,
            runtime,
        };
        // SAFETY: model remains owned until after all vocabulary reads and contexts.
        let trained = unsafe { model.api().llama_model_n_ctx_train(model.handle.as_ptr()) };
        if trained <= 0 || trained as usize != metadata.context {
            return Err(Error::Invalid(
                "native context length differs from GGUF metadata",
            ));
        }
        let mut engine = Self {
            model,
            metadata,
            templates,
            marker: -1,
            separator: -1,
            marker_text: String::new(),
            labels: Vec::new(),
        };
        match engine.metadata.algorithm {
            Algorithm::Laya => {
                // SAFETY: vocabulary belongs to the live model.
                unsafe {
                    engine.marker = engine.model.api().llama_vocab_mask(engine.model.vocab()?);
                    engine.separator = engine.model.api().llama_vocab_sep(engine.model.vocab()?);
                }
                if engine.marker < 0 || engine.separator < 0 {
                    return Err(Error::Invalid("laya model has no mask or separator token"));
                }
                engine.marker_text = engine.model.piece(engine.marker)?;
                // SAFETY: reads a scalar property of the owned model.
                if unsafe {
                    engine
                        .model
                        .api()
                        .llama_model_n_embd_out(engine.model.handle.as_ptr())
                } < 3
                {
                    return Err(Error::Invalid(
                        "laya decision head has fewer than three columns",
                    ));
                }
            }
            Algorithm::OpenJev => {
                for label in b'A'..=b'Z' {
                    engine.labels.push(engine.model.label(label)?);
                }
                for label in b'a'..=b'z' {
                    engine.labels.push(engine.model.label(label)?);
                }
            }
        }
        Ok(engine)
    }

    /// Runs one forward pass per question and produces no generated text tokens.
    pub fn decide<F: Fn() -> bool + Sync>(
        &mut self,
        request: &Request,
        cancelled: F,
    ) -> Result<Response> {
        request
            .validate()
            .map_err(|_| Error::Invalid("invalid decision request"))?;
        if cancelled() {
            return Err(Error::Cancelled);
        }
        let mut prompts = Vec::with_capacity(request.questions.len());
        for (name, question) in &request.questions {
            prompts.push(self.prompt(&request.state, name, question)?);
            if cancelled() {
                return Err(Error::Cancelled);
            }
        }
        let capacity = prompts
            .iter()
            .map(|prompt| prompt.tokens.len())
            .max()
            .expect("validated questions");
        let context = self.model.context(capacity, self.metadata.algorithm)?;
        let mut guard = Abort {
            context: &context,
            check: Box::new(Check {
                cancelled,
                panicked: AtomicBool::new(false),
                aborted: AtomicBool::new(false),
            }),
        };
        let data = std::ptr::from_mut(guard.check.as_mut()).cast();
        // SAFETY: guard retains the callback state through each synchronous call.
        unsafe {
            context
                .api
                .llama_set_abort_callback(context.handle.as_ptr(), Some(abort::<F>), data);
        }
        let mut answers = BTreeMap::new();
        let mut usage = Usage::default();
        for prompt in prompts {
            if (guard.check.cancelled)() {
                return Err(Error::Cancelled);
            }
            let scores = self
                .model
                .forward(&context, &prompt, &self.labels, &guard)?;
            answers.insert(
                prompt.query.name.to_owned(),
                prompt.query.answer(&self.metadata, &scores)?,
            );
            usage.input_tokens += prompt.tokens.len() as u64;
        }
        Ok(Response { answers, usage })
    }

    fn prompt<'a>(&self, state: &str, name: &'a str, question: &'a Question) -> Result<Prompt<'a>> {
        let query = Query::new(name, question, self.metadata.algorithm);
        let text = self.templates.render(state, &query, &self.marker_text)?;
        let mut tokens = self.model.tokenize(&text, true)?;
        let markers = if self.metadata.algorithm == Algorithm::Laya {
            inference::laya(
                &mut tokens,
                query.options.len(),
                self.marker,
                self.separator,
                self.metadata.head,
            )?
        } else {
            Vec::new()
        };
        if tokens.is_empty() || tokens.len() > self.metadata.context {
            return Err(Error::Invalid(
                "decision prompt exceeds the trained context",
            ));
        }
        Ok(Prompt {
            query,
            tokens,
            markers,
        })
    }
}

impl Model {
    fn api(&self) -> &ffi::Llama {
        &self.runtime.api
    }
    fn vocab(&self) -> Result<*const ffi::llama_vocab> {
        // SAFETY: vocabulary is borrowed from the owned live model.
        let vocab = unsafe { self.api().llama_model_get_vocab(self.handle.as_ptr()) };
        if vocab.is_null() {
            Err(Error::Native("model vocabulary is missing"))
        } else {
            Ok(vocab)
        }
    }
    fn tokenize(&self, text: &str, special: bool) -> Result<Vec<i32>> {
        let length = i32::try_from(text.len())
            .map_err(|_| Error::Invalid("decision prompt exceeds its limit"))?;
        let vocab = self.vocab()?;
        // SAFETY: the byte string is valid for its explicit length; a zero-capacity
        // call reports the required token buffer length without writing through null.
        let count = unsafe {
            self.api().llama_tokenize(
                vocab,
                text.as_ptr().cast(),
                length,
                std::ptr::null_mut(),
                0,
                false,
                special,
            )
        };
        let count = count
            .checked_neg()
            .filter(|count| *count > 0 && *count <= 1_048_576)
            .ok_or(Error::Invalid("decision prompt could not be tokenized"))?;
        let mut tokens = vec![0; count as usize];
        // SAFETY: tokens has exactly the requested writable capacity.
        let written = unsafe {
            self.api().llama_tokenize(
                vocab,
                text.as_ptr().cast(),
                length,
                tokens.as_mut_ptr(),
                count,
                false,
                special,
            )
        };
        if written != count {
            return Err(Error::Native("token count changed during tokenization"));
        }
        Ok(tokens)
    }
    fn label(&self, label: u8) -> Result<i32> {
        let tokens = self.tokenize(std::str::from_utf8(&[label]).expect("ASCII label"), false)?;
        if tokens.len() != 1 {
            return Err(Error::Invalid("openjev label is not a single token"));
        }
        Ok(tokens[0])
    }
    fn piece(&self, token: i32) -> Result<String> {
        let mut bytes = vec![0u8; 256];
        // SAFETY: the writable byte buffer is valid for its explicit capacity.
        let written = unsafe {
            self.api().llama_token_to_piece(
                self.vocab()?,
                token,
                bytes.as_mut_ptr().cast(),
                bytes.len() as i32,
                0,
                true,
            )
        };
        if written <= 0 || written as usize > bytes.len() {
            return Err(Error::Invalid("decision marker is invalid"));
        }
        bytes.truncate(written as usize);
        String::from_utf8(bytes).map_err(|_| Error::Invalid("decision marker is not UTF-8"))
    }
    fn context(&self, capacity: usize, algorithm: Algorithm) -> Result<Context<'_>> {
        // SAFETY: params is the exact pinned layout returned by the same runtime.
        let mut params = unsafe { self.api().llama_context_default_params() };
        params.n_ctx = capacity.max(32) as u32;
        params.n_batch = capacity as u32;
        params.n_ubatch = if algorithm == Algorithm::Laya {
            capacity
        } else {
            capacity.min(512)
        } as u32;
        params.n_seq_max = 1;
        // b11756's noncausal encode path reserves every token's output even when
        // only the marker positions are read by the decision head.
        params.n_outputs_max = if algorithm == Algorithm::Laya {
            capacity as u32
        } else {
            1
        };
        params.n_outputs_max_per_seq = params.n_outputs_max;
        let threads = sailry_model_assets::session::threads() as i32;
        params.n_threads = threads;
        params.n_threads_batch = threads;
        params.embeddings = algorithm == Algorithm::Laya;
        params.pooling_type = ffi::llama_pooling_type_LLAMA_POOLING_TYPE_NONE;
        params.attention_type = if algorithm == Algorithm::Laya {
            ffi::llama_attention_type_LLAMA_ATTENTION_TYPE_NON_CAUSAL
        } else {
            ffi::llama_attention_type_LLAMA_ATTENTION_TYPE_CAUSAL
        };
        params.offload_kqv = false;
        params.op_offload = false;
        params.no_perf = true;
        // SAFETY: model outlives this uniquely owned context.
        let handle = unsafe {
            self.api()
                .llama_init_from_model(self.handle.as_ptr(), params)
        };
        let context = Context {
            handle: NonNull::new(handle).ok_or(Error::Native("context could not be created"))?,
            api: self.api(),
        };
        // SAFETY: reads the actual allocated context, which may include native padding.
        if unsafe { self.api().llama_n_ctx(context.handle.as_ptr()) } < capacity as u32 {
            return Err(Error::Native("allocated context is too short"));
        }
        Ok(context)
    }
    fn forward<F: Fn() -> bool + Sync>(
        &self,
        context: &Context<'_>,
        prompt: &Prompt<'_>,
        labels: &[i32],
        guard: &Abort<'_, F>,
    ) -> Result<Vec<f32>> {
        let api = self.api();
        // SAFETY: the memory handle belongs to this live context; encoder contexts
        // can have no memory module and need no clearing between independent prompts.
        let memory = unsafe { api.llama_get_memory(context.handle.as_ptr()) };
        if !memory.is_null() {
            unsafe {
                api.llama_memory_clear(memory, true);
            }
        }
        let laya = !prompt.markers.is_empty();
        // SAFETY: queries scalar model/context capabilities.
        let encoder = unsafe { api.llama_model_has_encoder(self.handle.as_ptr()) };
        let chunk = if laya {
            prompt.tokens.len()
        } else {
            512.min(prompt.tokens.len())
        };
        // SAFETY: allocate one owned native batch for the bounded token count.
        let value = unsafe { api.llama_batch_init(chunk as i32, 0, 1) };
        let mut batch = Batch { value, api };
        if batch.value.token.is_null()
            || batch.value.pos.is_null()
            || batch.value.n_seq_id.is_null()
            || batch.value.seq_id.is_null()
            || batch.value.logits.is_null()
        {
            return Err(Error::Native("batch could not be allocated"));
        }
        for (index, tokens) in prompt.tokens.chunks(chunk).enumerate() {
            let start = index * chunk;
            batch.value.n_tokens = tokens.len() as i32;
            for (offset, token) in tokens.iter().enumerate() {
                // SAFETY: every array was allocated for chunk entries and one sequence.
                unsafe {
                    *batch.value.token.add(offset) = *token;
                    *batch.value.pos.add(offset) = (start + offset) as i32;
                    *batch.value.n_seq_id.add(offset) = 1;
                    *(*batch.value.seq_id.add(offset)) = 0;
                    *batch.value.logits.add(offset) = if laya {
                        1
                    } else {
                        (start + offset + 1 == prompt.tokens.len()) as i8
                    };
                }
            }
            if (guard.check.cancelled)() {
                return Err(Error::Cancelled);
            }
            // SAFETY: token/position/sequence/output buffers and callback state remain
            // valid until this synchronous public forward call returns.
            let status = unsafe {
                if encoder {
                    api.llama_encode(context.handle.as_ptr(), batch.value)
                } else {
                    api.llama_decode(context.handle.as_ptr(), batch.value)
                }
            };
            // SAFETY: finish any backend work while the batch and callback storage
            // are still alive. The C API permits asynchronous forward execution.
            unsafe {
                api.llama_synchronize(context.handle.as_ptr());
            }
            if guard.check.panicked.load(Ordering::Relaxed) {
                return Err(Error::Native("cancellation callback panicked"));
            }
            if status == 2
                || guard.check.aborted.load(Ordering::Relaxed)
                || (guard.check.cancelled)()
            {
                return Err(Error::Cancelled);
            }
            if status != 0 {
                return Err(Error::Native("forward pass failed"));
            }
        }
        if laya {
            prompt
                .markers
                .iter()
                .map(|index| {
                    // SAFETY: this exact batch entry requested an embedding output, and
                    // the selected column was checked against n_embd_out at model load.
                    let values = unsafe {
                        api.llama_get_embeddings_ith(context.handle.as_ptr(), *index as i32)
                    };
                    if values.is_null() {
                        return Err(Error::Native("decision embedding is missing"));
                    }
                    Ok(unsafe { *values.add(prompt.query.column()) })
                })
                .collect()
        } else {
            // SAFETY: only the final token requested logits; -1 selects that output.
            let logits = unsafe { api.llama_get_logits_ith(context.handle.as_ptr(), -1) };
            if logits.is_null() {
                return Err(Error::Native("decision logits are missing"));
            }
            // SAFETY: public tokenizer IDs must lie in the same live model vocabulary.
            let vocabulary = unsafe { api.llama_vocab_n_tokens(self.vocab()?) };
            labels
                .iter()
                .take(prompt.query.options.len())
                .map(|label| {
                    if *label < 0 || *label >= vocabulary {
                        return Err(Error::Invalid("decision label is outside the vocabulary"));
                    }
                    Ok(unsafe { *logits.add(*label as usize) })
                })
                .collect()
        }
    }
}

#[cfg(test)]
mod tests;
