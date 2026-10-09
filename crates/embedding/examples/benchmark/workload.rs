use super::process;
use sailry_embedding::Engine;
use serde_json::{Value, json};
use std::{
    error::Error,
    time::{Duration, Instant},
};

const BATCH: usize = 4;
const PASSAGE_TOKENS: usize = 256;

const PASSAGES: [&str; BATCH] = [
    "A long-running execution service owns session history, protected credentials, and admitted work. Desktop and phone controllers connect to that service rather than launching independent agents for the same profile. A controller can lose a reply after a mutation was committed, so retries preserve the original request identifier. Completion is distinct from a receipt that confirms durable admission. On reconnect, ordered events and a fresh snapshot rebuild the visible projection. Uncertain external effects are reported honestly instead of being automatically repeated. Each task captures its effective configuration at submission, while a later preference update applies only to the behavior it explicitly governs. Local and paired paths use the same commands but need separate acceptance evidence.",
    "Document retrieval starts with explicitly registered sources and a captured ownership boundary. Directory scanning observes nested ignore rules, avoids symbolic links, and reports skipped entries. A source content digest identifies the text used to build chunks. Office parsers supply typed pages, slides, sections, and cell ranges for citations. Small bounded CPU batches produce normalized embeddings that are quantized for a rebuildable index. Authorization, path, tag, and time filters apply before candidate selection. Keyword and semantic ranks can be fused without adding their incompatible raw scores. The model identity remains part of the collection binding, and an unavailable model falls back to keyword search without claiming semantic execution.",
    "一个知识集合需要明确的来源和访问范围。目录扫描逐层处理忽略规则，跳过凭据、二进制与符号链接，并保留无法读取条目的原因。文档内容和来源修订决定索引是否仍然有效。后台工作分成较小批次，优先响应用户正在等待的查询。模型下载必须核对固定修订中的文件大小和校验值，成功之前只写私有临时文件。取消任务时不发布尚未验证的权重。查询按照授权集合、标签、路径和时间筛选，然后生成关键词与语义候选。结果包含原文引用和实际检索模式，不把排序分数描述为理解问题的概率。",
    "多个控制端可以连接同一个执行主机。读取设置时返回当前修订，保存时使用这次读取的修订号，防止旧副本覆盖另一端刚保存的内容。冲突发生后保留界面草稿，让用户重新加载并核对。凭据存储在主机的受保护目录，事件流只说明是否已经配置，不广播秘密值。命令取消需要结束属于自己的子进程，并在完成前排空最后一批输出。资料结构缺少字段时集中填入默认值，在私有备份与事务保护下保持会话、配置和凭据。测试分别验证本地与配对路径，不把静态检查误称为真实服务验收。",
];

pub(super) const QUERIES: [&str; BATCH] = [
    "How can a controller retry after losing a reply without repeating the mutation?",
    "Which checks preserve ownership before combining keyword and vector search?",
    "模型下载被取消时，如何确保缓存没有未验证的权重？",
    "两台控制端同时保存设置，如何保留草稿并避免覆盖另一端？",
];

fn passage(engine: &Engine, text: &str) -> Result<String, Box<dyn Error>> {
    let mut value = text.repeat(4);
    let offsets = engine.offsets(&value)?;
    let mut end = offsets
        .get(PASSAGE_TOKENS - 1)
        .ok_or("benchmark passage is too short")?
        .1;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value.truncate(end);
    if engine.offsets(&value)?.len() != PASSAGE_TOKENS {
        return Err("benchmark passage does not contain exactly 256 input tokens".into());
    }
    Ok(value)
}

pub(super) fn documents(engine: &Engine) -> Result<Vec<String>, Box<dyn Error>> {
    PASSAGES.iter().map(|text| passage(engine, text)).collect()
}

pub(super) fn measure(
    engine: &mut Engine,
    label: &str,
    inputs: &[&str],
    runs: usize,
) -> Result<Value, Box<dyn Error>> {
    let tokens = inputs
        .iter()
        .map(|text| engine.offsets(text).map(|offsets| offsets.len()))
        .collect::<Result<Vec<_>, _>>()?;
    let started = Instant::now();
    let vectors = engine.encode(inputs)?;
    let warmup_ms = started.elapsed().as_secs_f64() * 1000.;
    validate(&vectors, inputs.len(), engine.dimension())?;
    let warmup_process = process::sample()?;
    let mut samples = Vec::with_capacity(runs);
    let mut elapsed = Duration::ZERO;
    for _ in 0..runs {
        let started = Instant::now();
        let vectors = engine.encode(inputs)?;
        let duration = started.elapsed();
        elapsed += duration;
        samples.push(duration);
        validate(&vectors, inputs.len(), engine.dimension())?;
    }
    samples.sort_unstable();
    let p50 = samples[(runs * 50).div_ceil(100) - 1];
    let p95 = samples[(runs * 95).div_ceil(100) - 1];
    Ok(json!({
        "workload": label,
        "batch": inputs.len(),
        "input_tokens": tokens,
        "special_tokens_excluded": true,
        "warmup": {"elapsed_ms": warmup_ms, "process": warmup_process},
        "measured": {
            "runs": runs,
            "elapsed_ms": elapsed.as_secs_f64() * 1000.,
            "p50_batch_ms": p50.as_secs_f64() * 1000.,
            "p95_batch_ms": p95.as_secs_f64() * 1000.,
            "documents_per_second": (inputs.len() * runs) as f64 / elapsed.as_secs_f64(),
            "tokens_per_second": (tokens.iter().sum::<usize>() * runs) as f64 / elapsed.as_secs_f64(),
            "process": process::sample()?,
        },
    }))
}

fn validate(vectors: &[Vec<f32>], count: usize, dimension: usize) -> Result<(), Box<dyn Error>> {
    if vectors.len() != count {
        return Err("benchmark output count differs from its input".into());
    }
    for vector in vectors {
        let norm = vector
            .iter()
            .map(|&value| f64::from(value).powi(2))
            .sum::<f64>()
            .sqrt();
        if vector.len() != dimension || !norm.is_finite() || (norm - 1.).abs() > 1e-5 {
            return Err("benchmark output is not a normalized embedding".into());
        }
    }
    Ok(())
}
