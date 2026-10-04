use sailry_protocol::{Permission, WorkMode};

pub(super) fn instruction(mode: WorkMode, permission: Permission) -> String {
    let mode = if mode == WorkMode::Plan {
        "This is a planning turn. Explore with read-only tools and produce a concrete plan; do not implement changes or run tools with side effects. If ask_user offers input kind plan, use it to present the completed plan for review. Its dedicated response can start a new coding turn; an ordinary text or choice answer cannot change work mode. Otherwise return the plan as text."
    } else {
        super::approval::instruction(permission)
    };
    format!("{}\n\n{mode}", include_str!("prompt.txt").trim())
}
