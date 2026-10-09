import argparse
import hashlib
import importlib.metadata
import json
import os
from pathlib import Path

os.environ["HF_HUB_OFFLINE"] = "1"
os.environ["TRANSFORMERS_OFFLINE"] = "1"
os.environ["TOKENIZERS_PARALLELISM"] = "false"

parser = argparse.ArgumentParser(description="Create offline embedding reference vectors")
parser.add_argument("model", choices=["granite-multi", "bge-small-zh", "potion-multi"])
parser.add_argument("directory", type=Path)
arguments = parser.parse_args()
directory = arguments.directory.resolve(strict=True)
catalog = {
    "granite-multi": {
        "revision": "835ad14087e140460703cf0fae09f97d469d65c2",
        "weights": "f3ea88b230492811046145513710e76b4cc8c2ad49e8708da0e7247e548903be",
    },
    "bge-small-zh": {
        "revision": "7999e1d3359715c523056ef9478215996d62a620",
        "weights": "354763b9b1357bc9c44f62c6be2276321081ed2567773608c0d0785b61d5a026",
    },
    "potion-multi": {
        "revision": "73908c3438cf03b6a01bcb9611d62b23d0726f08",
        "weights": "14b5eb39cb4ce5666da8ad1f3dc6be4346e9b2d601c073302fa0a31bf7943397",
    },
}
descriptor = catalog[arguments.model]
with (directory / "model.safetensors").open("rb") as stream:
    assert hashlib.file_digest(stream, "sha256").hexdigest() == descriptor["weights"]

texts = [
    "An isolated reference sentence",
    "東京と北京の検索",
    "alpha alpha beta",
    "",
    "本地知识库可离线检索",
    "A short note about authentication and network access",
]
if arguments.model == "potion-multi":
    from model2vec import StaticModel
    for package, version in [("numpy", "2.4.4")]:
        assert importlib.metadata.version(package) == version
    revision = "52cb9ffacd3df382cca0a67e3b8746fadec5cecc"
    provenance = importlib.metadata.distribution("model2vec").read_text("direct_url.json")
    assert provenance is not None, "install the official Model2Vec repository at the pinned revision"
    assert json.loads(provenance)["vcs_info"]["commit_id"] == revision
    model = StaticModel.from_pretrained(directory, force_download=False)
    vectors = model.encode(texts, max_length=None, use_multiprocessing=False).tolist()
    implementation = f"model2vec@{revision};numpy=2.4.4"
else:
    import torch
    from tokenizers import Tokenizer
    from transformers import AutoModel
    for package, version in [("transformers", "4.56.2"), ("torch", "2.8.0")]:
        assert importlib.metadata.version(package).split("+")[0] == version
    torch.set_num_threads(1)
    tokenizer = Tokenizer.from_file(str(directory / "tokenizer.json"))
    tokenizer.no_truncation()
    tokenizer.no_padding()
    model = AutoModel.from_pretrained(directory, local_files_only=True, dtype=torch.float32, attn_implementation="eager")
    model.eval()
    vectors = []
    with torch.inference_mode():
        for text in texts:
            encoded = tokenizer.encode(text, add_special_tokens=True)
            parameters = {
                "input_ids": torch.tensor([encoded.ids]),
                "attention_mask": torch.tensor([encoded.attention_mask]),
            }
            if arguments.model == "bge-small-zh":
                parameters["token_type_ids"] = torch.tensor([encoded.type_ids])
            vector = model(**parameters).last_hidden_state[:, 0]
            vectors.append(torch.nn.functional.normalize(vector, p=2, dim=1)[0].tolist())
    implementation = "transformers=4.56.2;torch=2.8.0"

reference = {
    "model_revision": descriptor["revision"],
    "implementation": implementation,
    "cases": [{"text": text, "vector": list(vector)} for text, vector in zip(texts, vectors)],
}
temporary = directory / "reference.json.tmp"
temporary.write_text(json.dumps(reference), encoding="utf-8")
temporary.replace(directory / "reference.json")
