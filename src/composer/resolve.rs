use std::{collections::HashMap, path::PathBuf, sync::Arc};

use anyhow::{bail, Context, Result};
use tracing::instrument;

use crate::entities::{
    ParallelTasks, RootTaskNode, SequenceTasks, Submission, SubmissionConfig, TaskConfig,
    TaskConfigExt, TaskNode, TaskNodeExt,
};

#[instrument(skip_all)]
pub fn resolve_submission(
    config: Arc<SubmissionConfig>,
    root_directory: PathBuf,
) -> Result<Submission> {
    let root_node = Arc::new(RootTaskNode {
        tasks: vec![
            resolve_sequence(".", &config.tasks).context("Error resolving root sequence tasks")?,
        ],
    });
    Ok(Submission { id: config.id.clone(), root_directory, config, root_node })
}

fn resolve_sequence(name_prefix: &str, tasks: &SequenceTasks) -> Result<Arc<TaskNode>> {
    if tasks.is_empty() {
        bail!("Empty steps provided");
    }

    let mut nodes: HashMap<String, TaskNode> = HashMap::default();
    let mut children: HashMap<String, Vec<String>> = HashMap::default();

    let root_node = {
        let (name, root_task) = tasks.first().unwrap();
        resolve_task(format!("{name_prefix}{}", name), root_task.clone())?
    };
    nodes.insert(root_node.name.clone(), root_node.clone());

    let mut previous_node = root_node.clone();
    for (i, (name, task)) in tasks.iter().enumerate().skip(1) {
        let node = resolve_task(format!("{name_prefix}{}", name), task.clone())?;

        match &task.needs {
            None => {
                children.entry(previous_node.name).or_default().push(node.name.clone());
                previous_node = node.clone();
            }
            Some(needs) => {
                let exists = tasks
                    .iter()
                    .take(i)
                    .find(|(task_name, _)| *task_name == needs && *task_name != name)
                    .is_some();
                if !exists {
                    bail!("Unknown task specified by the `needs` field: {needs}")
                }

                let needs_name = format!("{name_prefix}{needs}");
                children.entry(needs_name).or_default().push(node.name.clone());
            }
        };

        nodes.insert(node.name.clone(), node);
    }

    Ok(Arc::new(append_children(root_node, &nodes, &children)))
}

fn resolve_task(name: String, config: Arc<TaskConfig>) -> Result<TaskNode> {
    Ok(match &config.ext {
        TaskConfigExt::Sequence(ext) => {
            let prefix = format!("{name}.");
            let ext = TaskNodeExt::Schedule({
                vec![
                    resolve_sequence(&prefix, &ext.tasks)
                        .context("Error resolving sequence tasks")?,
                ]
            });
            TaskNode { name, config, children: vec![], ext }
        }
        TaskConfigExt::Parallel(ext) => {
            let ext = TaskNodeExt::Schedule(match &ext.tasks {
                ParallelTasks::Anonymous(tasks) => tasks
                    .iter()
                    .enumerate()
                    .map(|(i, task)| {
                        resolve_task(format!("{name}.{i}"), task.clone()).map(Arc::new)
                    })
                    .collect::<Result<_>>()
                    .context("Error resolving anonymous parallel tasks")?,
                ParallelTasks::Named(tasks) => tasks
                    .iter()
                    .map(|(task_name, task)| {
                        resolve_task(format!("{name}.{task_name}"), task.clone()).map(Arc::new)
                    })
                    .collect::<Result<_>>()
                    .context("Error resolving named parallel tasks")?,
            });
            TaskNode { name, config, children: vec![], ext }
        }
        TaskConfigExt::Action(ext) => {
            let ext = TaskNodeExt::Action(Arc::new(ext.clone()));
            TaskNode { name, config, children: vec![], ext }
        }
    })
}

fn append_children(
    mut node: TaskNode,
    nodes: &HashMap<String, TaskNode>,
    children: &HashMap<String, Vec<String>>,
) -> TaskNode {
    if let Some(names) = children.get(&node.name) {
        node.children = names
            .iter()
            .map(|name| {
                let node = nodes.get(name).unwrap().clone();
                append_children(node, nodes, children)
            })
            .map(Arc::new)
            .collect();
    }
    node
}

#[cfg(test)]
mod tests {
    use std::fs;

    use insta::glob;

    #[test]
    fn test_resolve_submission() {
        glob!("tests/*.yaml", |path| {
            let submission = super::resolve_submission(
                serde_yaml::from_str(&fs::read_to_string(path).unwrap()).unwrap(),
                "test".into(),
            )
            .expect("Error resolving the submission");
            insta::with_settings!({snapshot_path => "tests/snapshots"}, {
                insta::assert_ron_snapshot!(submission);
            });
        });
    }
}
