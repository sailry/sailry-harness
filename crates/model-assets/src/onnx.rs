//! ONNX 1.22.0 metadata projection for single-file models.
//! Fields follow https://github.com/onnx/onnx/blob/v1.22.0/onnx/onnx.proto,
//! the schema pinned by ONNX Runtime 1.28.2. Weight arrays are unknown fields
//! and are skipped by Prost rather than copied into this projection.

use prost::Message;
use std::{fs::File, io, path::Path};

#[cfg(test)]
#[path = "onnx/tests.rs"]
mod tests;

/// Checks a verified, immutable asset owned by the execution Node.
/// The owner must not modify the file in place while this function reads it.
pub fn validate(path: &Path) -> io::Result<()> {
    let file = File::open(path)?;
    // SAFETY: the model owner verifies private, immutable assets before loading;
    // publication replaces files atomically and never modifies mapped bytes.
    let mapping = unsafe { memmap2::MmapOptions::new().map(&file) }?;
    check(&mapping)
}

fn check(bytes: &[u8]) -> io::Result<()> {
    let model =
        Model::decode(bytes).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    if model.graphs.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "ONNX model graph is missing",
        ));
    }
    if model.external() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "ONNX model requires external tensor data",
        ));
    }
    Ok(())
}

#[derive(Message)]
struct Model {
    #[prost(message, repeated, tag = "7")]
    graphs: Vec<Graph>,
    #[prost(message, repeated, tag = "20")]
    training: Vec<Training>,
    #[prost(message, repeated, tag = "25")]
    functions: Vec<Function>,
}

impl Model {
    fn external(&self) -> bool {
        self.graphs.iter().any(Graph::external)
            || self.training.iter().any(Training::external)
            || self.functions.iter().any(Function::external)
    }
}

#[derive(Message)]
struct Training {
    #[prost(message, repeated, tag = "1")]
    initialization: Vec<Graph>,
    #[prost(message, repeated, tag = "2")]
    algorithm: Vec<Graph>,
}

impl Training {
    fn external(&self) -> bool {
        self.initialization
            .iter()
            .chain(&self.algorithm)
            .any(Graph::external)
    }
}

#[derive(Message)]
struct Graph {
    #[prost(message, repeated, tag = "1")]
    nodes: Vec<Node>,
    #[prost(message, repeated, tag = "5")]
    initializers: Vec<Tensor>,
    #[prost(message, repeated, tag = "15")]
    sparse: Vec<Sparse>,
}

impl Graph {
    fn external(&self) -> bool {
        self.nodes.iter().any(Node::external)
            || self.initializers.iter().any(Tensor::external)
            || self.sparse.iter().any(Sparse::external)
    }
}

#[derive(Message)]
struct Node {
    #[prost(message, repeated, tag = "5")]
    attributes: Vec<Attribute>,
}

impl Node {
    fn external(&self) -> bool {
        self.attributes.iter().any(Attribute::external)
    }
}

#[derive(Message)]
struct Function {
    #[prost(message, repeated, tag = "7")]
    nodes: Vec<Node>,
    #[prost(message, repeated, tag = "11")]
    attributes: Vec<Attribute>,
}

impl Function {
    fn external(&self) -> bool {
        self.nodes.iter().any(Node::external) || self.attributes.iter().any(Attribute::external)
    }
}

#[derive(Message)]
struct Attribute {
    #[prost(message, repeated, tag = "5")]
    tensor: Vec<Tensor>,
    #[prost(message, repeated, tag = "10")]
    tensors: Vec<Tensor>,
    #[prost(message, repeated, tag = "6")]
    graph: Vec<Graph>,
    #[prost(message, repeated, tag = "11")]
    graphs: Vec<Graph>,
    #[prost(message, repeated, tag = "22")]
    sparse: Vec<Sparse>,
    #[prost(message, repeated, tag = "23")]
    sparse_list: Vec<Sparse>,
}

impl Attribute {
    fn external(&self) -> bool {
        self.tensor
            .iter()
            .chain(&self.tensors)
            .any(Tensor::external)
            || self.graph.iter().chain(&self.graphs).any(Graph::external)
            || self
                .sparse
                .iter()
                .chain(&self.sparse_list)
                .any(Sparse::external)
    }
}

#[derive(Message)]
struct Sparse {
    #[prost(message, repeated, tag = "1")]
    values: Vec<Tensor>,
    #[prost(message, repeated, tag = "2")]
    indices: Vec<Tensor>,
}

impl Sparse {
    fn external(&self) -> bool {
        self.values
            .iter()
            .chain(&self.indices)
            .any(Tensor::external)
    }
}

#[derive(Message)]
struct Tensor {
    #[prost(message, repeated, tag = "13")]
    external_data: Vec<Marker>,
    // Keep every enum occurrence so a later DEFAULT cannot hide EXTERNAL.
    #[prost(uint32, repeated, packed = "false", tag = "14")]
    locations: Vec<u32>,
}

impl Tensor {
    fn external(&self) -> bool {
        !self.external_data.is_empty() || self.locations.contains(&1)
    }
}

#[derive(Message)]
struct Marker {}
