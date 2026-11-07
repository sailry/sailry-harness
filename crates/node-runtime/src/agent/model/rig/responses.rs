//! Preserve Goose 14741e9's streamed-text precedence over Responses snapshots.
use rig_core::{
    completion::CompletionRequest,
    error::{EncodeError, ProviderError},
    operation::Completion,
    providers::openai::responses_api::{
        streaming::{ResponsesDecoder, ResponsesEvent},
        wire::Responses as NativeResponses,
    },
    wire::{Decoder as Decode, Descriptor, Encoded, Flow, Mode, Out, Wire, WireEvent, WireFrame},
};

pub(super) const TEXT_DELTA: &str = "__sailry_responses_text_delta";

pub(super) mod framing;

#[derive(Clone)]
pub(super) struct Responses(pub NativeResponses);

impl Wire for Responses {
    type Op = Completion;
    type Payload = Encoded;
    type Frame = WireFrame;
    type Event = ResponsesEvent;
    type Decoder<'id> = Decoder;
    type Reassembler = <NativeResponses as Wire>::Reassembler;

    fn describe(&self) -> Descriptor<'_> {
        self.0.describe()
    }

    fn encode(&self, request: CompletionRequest, mode: Mode) -> Result<Encoded, EncodeError> {
        self.0.encode(request, mode)
    }

    fn decoder<'id>(&self) -> Self::Decoder<'id> {
        Decoder(self.0.decoder())
    }
}

pub(super) struct Decoder(ResponsesDecoder);

impl<'id> Decode<'id, Completion> for Decoder {
    type Event = ResponsesEvent;

    fn classify(&self, frame: WireFrame) -> WireEvent<Self::Event> {
        match self.0.classify(frame) {
            WireEvent::Known(ResponsesEvent::Whole(value)) => WireEvent::Unknown {
                event_type: "response".into(),
                value: value.into(),
            },
            event => event,
        }
    }

    fn decode(
        &mut self,
        event: ResponsesEvent,
        mut out: Out<'id, Completion>,
    ) -> Result<Flow, ProviderError> {
        let delta = matches!(
            &event,
            ResponsesEvent::Frame { kind, .. }
                if matches!(kind.as_str(), "response.output_text.delta" | "response.refusal.delta")
        );
        out.unknown(serde_json::json!({(TEXT_DELTA): delta}).into());
        self.0.decode(event, out)
    }

    fn eof(&mut self, out: Out<'id, Completion>) -> Result<Flow, ProviderError> {
        self.0.eof(out)
    }
}
