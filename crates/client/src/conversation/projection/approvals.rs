use sailry_protocol::{
    Fault,
    conversation::{Approval, ApprovalState},
};

pub(in crate::conversation) fn check_transition(
    previous: &Approval,
    incoming: &Approval,
    allow_stale: bool,
) -> Result<(), Fault> {
    let terminal = previous.state != ApprovalState::Pending;
    let stale = allow_stale && incoming.state == ApprovalState::Pending;
    if previous.id != incoming.id
        || previous.session != incoming.session
        || previous.turn != incoming.turn
        || previous.entry != incoming.entry
        || previous.index != incoming.index
        || previous.source != incoming.source
        || previous.prefix != incoming.prefix
        || previous.can_remember != incoming.can_remember
        || terminal
            && !stale
            && (previous.state != incoming.state
                || previous.choice != incoming.choice
                || previous.reason != incoming.reason)
    {
        return Err(Fault::invalid(
            "approval identity or terminal decision changed",
        ));
    }
    Ok(())
}
