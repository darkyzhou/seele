use std::{collections::HashMap, hash::Hash};

use futures_util::{
    future::{BoxFuture, Shared},
    FutureExt,
};
use tokio::sync::Mutex;

type Task<R> = Shared<BoxFuture<'static, R>>;
type TaskFactoryFn<K, R> = Box<dyn Fn(&K) -> BoxFuture<'static, R> + Send + Sync>;

pub struct CondGroup<K, R> {
    tasks: Mutex<HashMap<K, Task<R>>>,
    task_fn: TaskFactoryFn<K, R>,
}

impl<K, R> CondGroup<K, R>
where
    K: Eq + Hash + Clone,
    R: Clone,
{
    pub fn new(task_fn: impl Fn(&K) -> BoxFuture<'static, R> + Send + Sync + 'static) -> Self {
        Self { tasks: Default::default(), task_fn: Box::new(task_fn) }
    }

    pub async fn run(&self, key: &K) -> R {
        let mut tasks = self.tasks.lock().await;
        match tasks.get(key) {
            None => {
                let task = (self.task_fn)(key).shared();
                tasks.insert(key.clone(), task.clone());
                drop(tasks);

                task.await
            }
            Some(task) => {
                let task = task.clone();
                drop(tasks);

                task.await
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use futures_util::FutureExt;
    use rand::Rng;

    use super::CondGroup;

    #[tokio::test]
    async fn test_run_single() {
        let group: CondGroup<(), ()> =
            CondGroup::new(|_| tokio::time::sleep(Duration::from_millis(100)).boxed());

        group.run(&()).await;
    }

    #[tokio::test]
    async fn test_run_multiple() {
        let group: CondGroup<i32, (i32, i32)> = CondGroup::new(|num: &i32| {
            let num = *num;
            tokio::time::sleep(Duration::from_millis(100))
                .map(move |_| (num, rand::thread_rng().gen_range(0..1000)))
                .boxed()
        });

        let (a, b) = futures_util::join!(group.run(&114), group.run(&114));

        assert_eq!(a, b);
    }

    #[tokio::test]
    async fn test_run_complex() {
        let group: CondGroup<i32, (i32, i32)> = CondGroup::new(|num: &i32| {
            let num = *num;
            tokio::time::sleep(Duration::from_millis(100))
                .map(move |_| (num, rand::thread_rng().gen_range(0..1000)))
                .boxed()
        });

        let (a, b, c, d, e) = futures_util::join!(
            group.run(&114),
            group.run(&114),
            group.run(&514),
            group.run(&514),
            group.run(&1919),
        );

        assert_eq!(a, b);
        assert_eq!(c, d);
        assert_eq!(e.0, 1919);
    }
}
