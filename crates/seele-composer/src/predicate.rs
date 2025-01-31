use crate::entities::{TaskNode, TaskStatus};

const TRUE: &str = "true";
const PREVIOUS_OK: &str = "previous.ok";

pub fn check_node_predicate(parent_node: &TaskNode, node: &TaskNode) -> bool {
    let predicate = match &node.config.when {
        Some(when) => when.as_str(),
        None => PREVIOUS_OK,
    };

    match predicate {
        TRUE => true,
        PREVIOUS_OK => {
            matches!(*parent_node.config.status.read().unwrap(), TaskStatus::Success { .. })
        }
        _ => false,
    }
}
