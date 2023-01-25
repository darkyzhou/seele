use crate::{
    entities::{
        RootTaskNode, SequenceTasks, Submission, SubmissionConfig, TaskConfig, TaskExtraConfig,
        TaskNode, TaskNodeExtra,
    },
    shared,
};
use anyhow::{bail, Context};
use std::{collections::HashMap, sync::Arc};

pub fn resolve_submission(config: Arc<SubmissionConfig>) -> anyhow::Result<Submission> {
    let root_id = config.id.clone();
    let root = Arc::new(RootTaskNode {
        id: root_id.clone(),
        tasks: vec![resolve_sequence(&config.tasks).context("Error resolving root sequence tasks")?],
    });
    let id_to_node_map = get_id_to_node_map(root.clone());
    Ok(Submission { id: root_id, config, id_to_node_map, root })
}

fn resolve_sequence(tasks: &SequenceTasks) -> anyhow::Result<Arc<TaskNode>> {
    if tasks.is_empty() {
        bail!("Empty steps provided");
    }

    // TODO: check duplicate name in `steps`

    let mut id_to_node_map: HashMap<String, TaskNode> = HashMap::default();
    let mut id_to_children_map: HashMap<String, Vec<String>> = HashMap::default();

    let root_node = {
        let (_, root_task) = tasks.first().unwrap();
        resolve_task(root_task.clone())?
    };
    id_to_node_map.insert(root_node.id.clone(), root_node.clone());

    let mut prev_seq_node_id = root_node.id.clone();
    for (i, (name, task)) in tasks.iter().enumerate().skip(1) {
        let node = resolve_task(task.clone())?;

        match &task.needs {
            None => {
                id_to_children_map
                    .entry(prev_seq_node_id.clone())
                    .or_default()
                    .push(node.id.clone());

                prev_seq_node_id = node.id.clone();
            }
            Some(needs) => match tasks
                .iter()
                .take(i)
                .find(|(task_name, _)| *task_name == needs && *task_name != name)
            {
                None => bail!("Unknown task specified by the `needs` field: {}", needs),
                Some(_) => {
                    id_to_children_map.entry(needs.clone()).or_default().push(node.id.clone());
                }
            },
        };

        id_to_node_map.insert(node.id.clone(), node);
    }

    Ok(Arc::new(append_children(&id_to_node_map, &id_to_children_map, root_node)))
}

fn resolve_task(config: Arc<TaskConfig>) -> anyhow::Result<TaskNode> {
    let id = shared::random_task_id();
    Ok(match &config.extra {
        TaskExtraConfig::Sequence(extra) => {
            let extra = TaskNodeExtra::Schedule({
                vec![resolve_sequence(&extra.tasks).context("Error resolving sequence tasks")?]
            });
            TaskNode { config, id, children: vec![], extra }
        }
        TaskExtraConfig::Parallel(extra) => {
            let extra = TaskNodeExtra::Schedule(
                extra
                    .tasks
                    .iter()
                    .map(|task| resolve_task(task.clone()).map(Arc::new))
                    .collect::<anyhow::Result<_>>()
                    .context("Error resolving parallel tasks")?,
            );
            TaskNode { config, id, children: vec![], extra }
        }
        TaskExtraConfig::Action(extra) => {
            let extra = TaskNodeExtra::Action(Arc::new(extra.clone()));
            TaskNode { config, id, children: vec![], extra }
        }
    })
}

fn append_children(
    id_to_node_map: &HashMap<String, TaskNode>,
    id_to_children_map: &HashMap<String, Vec<String>>,
    mut node: TaskNode,
) -> TaskNode {
    if let Some(children_ids) = id_to_children_map.get(&node.id) {
        node.children = children_ids
            .iter()
            .map(|id| {
                let node = id_to_node_map.get(id).unwrap().clone();
                append_children(id_to_node_map, id_to_children_map, node)
            })
            .map(Arc::new)
            .collect();
    }
    node
}

fn get_id_to_node_map(root: Arc<RootTaskNode>) -> HashMap<String, Arc<TaskNode>> {
    let mut result = HashMap::default();
    let mut queue = root.tasks.clone();

    while !queue.is_empty() {
        let mut next_queue = vec![];

        for node in queue {
            result.insert(node.id.clone(), node.clone());

            if let TaskNodeExtra::Schedule(tasks) = &node.extra {
                next_queue.extend(tasks.iter().cloned());
            }

            next_queue.extend(node.children.iter().cloned());
        }

        queue = next_queue;
    }

    result
}

#[cfg(test)]
mod tests {
    use insta::glob;
    use std::fs;

    #[test]
    fn test_resolve_submission() {
        glob!("stubs/*.yaml", |path| {
            let submission = super::resolve_submission(
                serde_yaml::from_str(&fs::read_to_string(path).unwrap()).unwrap(),
            )
            .expect("Error resolving the submission");
            insta::assert_ron_snapshot!(submission, {
                ".**.id" => "[id]"
            });
        });
    }
}
