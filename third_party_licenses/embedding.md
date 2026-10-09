# Local embedding sources and model licenses

Granite and BGE use CPU ONNX sessions through
[`ort` 2.0.0-rc.13](https://github.com/pykeio/ort/tree/v2.0.0-rc.13),
licensed under MIT or Apache-2.0. ONNX Runtime 1.28.2 is loaded from the packaged
native library; its MIT notice is in `third_party_licenses/onnxruntime-MIT.txt`.

The shared single-file check follows the
[ONNX 1.22.0 schema](https://github.com/onnx/onnx/blob/v1.22.0/onnx/onnx.proto)
used by ONNX Runtime 1.28.2. Its Rust metadata projection is decoded with Prost
0.14.4, using read-only maps from memmap2 0.9.11; weight arrays are skipped.

Model weights are optional downloads, not bundled source. The catalog records
their immutable revisions, file sizes, SHA-256 values, and upstream license IDs:

| Model | Upstream revision | License |
| --- | --- | --- |
| [Granite multilingual R2](https://huggingface.co/ibm-granite/granite-embedding-97m-multilingual-r2/tree/835ad14087e140460703cf0fae09f97d469d65c2) | `835ad14087e140460703cf0fae09f97d469d65c2` | Apache-2.0 |
| [BGE small Chinese v1.5](https://huggingface.co/BAAI/bge-small-zh-v1.5/tree/7999e1d3359715c523056ef9478215996d62a620) | `7999e1d3359715c523056ef9478215996d62a620` | MIT |
| [Potion multilingual 128M](https://huggingface.co/minishlab/potion-multilingual-128M/tree/73908c3438cf03b6a01bcb9611d62b23d0726f08) | `73908c3438cf03b6a01bcb9611d62b23d0726f08` | MIT |

Granite uses the upstream `onnx/model.onnx` from the revision listed above.
BGE uses the converted `onnx/model.onnx` from
[`Xenova/bge-small-zh-v1.5@75c43b069aac4d136ba6bc1122f995fedcfd2781`](https://huggingface.co/Xenova/bge-small-zh-v1.5/tree/75c43b069aac4d136ba6bc1122f995fedcfd2781),
while retaining BAAI's pinned configuration, tokenizer, license and reference
vectors. The conversion's model card identifies BAAI as its upstream model.

The Potion kernel implements row lookup, frequency-preserving mean pooling and
L2 normalization without copying Model2Vec source. Opt-in reference generation
uses official [Model2Vec commit `52cb9ffacd3df382cca0a67e3b8746fadec5cecc`](https://github.com/MinishLab/model2vec/blob/52cb9ffacd3df382cca0a67e3b8746fadec5cecc/model2vec/model.py),
whose declared unknown-token handling covers Unigram models, and Transformers
4.56.2 with PyTorch 2.8.0. Model2Vec 0.9.0's released implementation lacks that
Unigram handling and is not the reference for this kernel.
