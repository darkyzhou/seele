use crate::{
    entity::{
        SequenceTasks, Submission, SubmissionConfig, TaskConfig, TaskExtraConfig, TaskNode,
        TaskNodeExtra,
    },
    shared,
};
use anyhow::{bail, Context};
use std::{collections::HashMap, sync::Arc};

pub fn resolve_submission(config: SubmissionConfig) -> anyhow::Result<Submission> {
    let root_id = config.id.clone();
    let root = Arc::new(TaskNode {
        parent_id: None,
        id: root_id.clone(),
        when: None,
        needs: None,
        children: vec![],
        extra: TaskNodeExtra::Schedule(
            resolve_sequence(Some(root_id.clone()), &config.tasks)
                .context("Error resolving root sequence tasks")?,
        ),
    });

    Ok(Submission {
        id: root_id,
        id_to_config_map: get_id_to_config_map(&config),
        id_to_node_map: get_id_to_node_map(root.clone()),
        config,
        root,
    })
}

fn resolve_sequence(
    parent_id: Option<String>,
    tasks: &SequenceTasks,
) -> anyhow::Result<Vec<Arc<TaskNode>>> {
    if tasks.is_empty() {
        bail!("Empty steps provided");
    }

    // TODO: check duplicate name in `steps`

    let mut id_to_node_map: HashMap<String, TaskNode> = HashMap::default();
    let mut id_to_children_map: HashMap<String, Vec<String>> = HashMap::default();
    let mut root_ids: Vec<String> = vec![];
    for (i, (id, task)) in tasks.iter().enumerate() {
        let node = resolve_task(parent_id.clone(), task)?;

        match (&task.needs, root_ids.last()) {
            (None, Some(prev_id)) => {
                id_to_children_map.entry(prev_id.clone()).or_default().push(node.id.clone())
            }
            (Some(needs), _) => match tasks[0..i].iter().find(|(name, _)| name == needs) {
                None => bail!("Unknown task specified by the `needs` field: {}", needs),
                Some(_) => {
                    id_to_children_map.entry(needs.clone()).or_default().push(node.id.clone())
                }
            },
            _ => {}
        };

        if task.needs.is_none() {
            root_ids.push(node.id.clone());
        }

        id_to_node_map.insert(id.clone(), node);
    }

    Ok(root_ids
        .into_iter()
        .map(|id| {
            append_children(
                &id_to_node_map,
                &id_to_children_map,
                id_to_node_map.get(&id).unwrap().clone(),
            )
        })
        .map(Arc::new)
        .collect())
}

fn resolve_task(parent_id: Option<String>, task: &TaskConfig) -> anyhow::Result<TaskNode> {
    let id = match &parent_id {
        Some(parent_id) => format!("{}_{}", parent_id, shared::random_task_id()),
        None => shared::random_task_id(),
    };
    Ok(match &task.extra {
        TaskExtraConfig::Sequence(config) => TaskNode {
            parent_id,
            id,
            when: task.when.clone(),
            needs: task.needs.clone(),
            children: vec![],
            extra: TaskNodeExtra::Schedule(
                resolve_sequence(Some(task.id.clone()), &config.tasks)
                    .context("Error resolving sequence tasks")?,
            ),
        },
        TaskExtraConfig::Parallel(config) => TaskNode {
            parent_id,
            id,
            when: task.when.clone(),
            needs: task.needs.clone(),
            children: vec![],
            extra: TaskNodeExtra::Schedule(
                config
                    .tasks
                    .iter()
                    .map(|task| resolve_task(Some(task.id.clone()), &task.clone()).map(Arc::new))
                    .collect::<anyhow::Result<_>>()
                    .context("Error resolving parallel tasks")?,
            ),
        },
        TaskExtraConfig::Action(config) => TaskNode {
            parent_id,
            id,
            when: task.when.clone(),
            needs: task.needs.clone(),
            children: vec![],
            extra: TaskNodeExtra::Action(Arc::new(config.clone())),
        },
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

fn get_id_to_config_map(config: &SubmissionConfig) -> HashMap<String, Arc<TaskConfig>> {
    let mut result = HashMap::default();
    let mut queue = config.tasks.iter().map(|(_, config)| config.clone()).collect::<Vec<_>>();

    while !queue.is_empty() {
        let mut next_queue = vec![];

        for config in &queue {
            result.insert(config.id.clone(), config.clone());

            match &config.extra {
                TaskExtraConfig::Sequence(config) => {
                    next_queue.extend(config.tasks.iter().map(|(_, config)| config.clone()))
                }
                TaskExtraConfig::Parallel(config) => {
                    next_queue.extend(config.tasks.iter().cloned())
                }
                _ => {}
            }
        }

        queue = next_queue;
    }

    result
}

fn get_id_to_node_map(root: Arc<TaskNode>) -> HashMap<String, Arc<TaskNode>> {
    let mut result = HashMap::default();
    let mut queue = vec![root];

    while !queue.is_empty() {
        let mut next_queue = vec![];

        for node in queue {
            result.insert(node.id.clone(), node.clone());

            next_queue.extend(node.children.iter().cloned());
            if let TaskNodeExtra::Schedule(tasks) = &node.extra {
                next_queue.extend(tasks.iter().cloned());
            }
        }

        queue = next_queue;
    }

    result
}
