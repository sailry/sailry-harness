//! Retain Goose 14741e9's incomplete-tool semantics at Rig's Messages boundary.
use rig_core::{
    completion::CompletionRequest,
    error::{EncodeError, ProviderError},
    operation::Completion,
    providers::anthropic::{
        streaming::{MessagesDecoder, MessagesEvent},
        wire::Messages as NativeMessages,
    },
    wire::{Decoder as Decode, Descriptor, Encoded, Flow, Mode, Out, Wire, WireEvent, WireFrame},
};
use std::collections::BTreeSet;

pub(super) const INCOMPLETE: &str = "sailry_incomplete";

#[derive(Clone)]
pub(super) struct Messages(pub NativeMessages);

impl Wire for Messages {
    type Op = Completion;
    type Payload = Encoded;
    type Frame = WireFrame;
    type Event = MessagesEvent;
    type Decoder<'id> = Decoder;
    type Reassembler = <NativeMessages as Wire>::Reassembler;

    fn describe(&self) -> Descriptor<'_> {
        self.0.describe()
    }

    fn encode(&self, request: CompletionRequest, mode: Mode) -> Result<Encoded, EncodeError> {
        self.0.encode(request, mode)
    }

    fn decoder<'id>(&self) -> Self::Decoder<'id> {
        Decoder {
            inner: self.0.decoder(),
            calls: BTreeSet::new(),
        }
    }
}

pub(super) struct Decoder {
    inner: MessagesDecoder,
    calls: BTreeSet<usize>,
}

impl<'id> Decode<'id, Completion> for Decoder {
    type Event = MessagesEvent;

    fn classify(&self, frame: WireFrame) -> WireEvent<Self::Event> {
        self.inner.classify(frame)
    }

    fn decode(
        &mut self,
        event: MessagesEvent,
        mut out: Out<'id, Completion>,
    ) -> Result<Flow, ProviderError> {
        let index = event.fields["index"]
            .as_u64()
            .and_then(|index| usize::try_from(index).ok());
        match event.fields["type"].as_str() {
            Some("content_block_start") if event.fields["content_block"]["type"] == "tool_use" => {
                if let Some(index) = index {
                    self.calls.insert(index);
                }
            }
            Some("content_block_stop") => {
                if let Some(index) = index {
                    self.calls.remove(&index);
                }
            }
            Some("message_delta") if event.fields["delta"]["stop_reason"].is_string() => {
                for index in std::mem::take(&mut self.calls) {
                    out.edit(index, |item| {
                        if let Some(item) = item.as_object_mut() {
                            item.insert(INCOMPLETE.into(), true.into());
                        }
                    })?;
                }
            }
            _ => {}
        }
        self.inner.decode(event, out)
    }

    fn eof(&mut self, out: Out<'id, Completion>) -> Result<Flow, ProviderError> {
        self.inner.eof(out)
    }
}
