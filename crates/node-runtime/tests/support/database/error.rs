//! Project fixture storage failures into shared Node faults.
pub fn fault(error: impl std::fmt::Display) -> sailry_protocol::Fault {
    sailry_protocol::Fault::new(sailry_protocol::ErrorCode::Internal, error.to_string())
}
