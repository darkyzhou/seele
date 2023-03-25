use std::{collections::HashMap, hash::Hash};

use futures_util::{
    future::{BoxFuture, Shared},
    FutureExt,
};
use tokio::sync::Mutex;
use triggered::Listener;

type Task<R> = Shared<BoxFuture<'static, R>>;
type TaskFactoryFn<K, R> = Box<dyn Fn(&K) -> BoxFuture<'static, R> + Send + Sync>;

pub struct CondGroup<K, R> {
    tasks: Mutex<HashMap<K, Task<Option<R>>>>,
    task_fn: TaskFactoryFn<K, R>,
}

impl<K, R> CondGroup<K, R>
where
    K: Clone + Eq + Hash,
    R: Clone + 'static,
{
    pub fn new(task_fn: impl Fn(&K) -> BoxFuture<'static, R> + Send + Sync + 'static) -> Self {
        Self { tasks: Default::default(), task_fn: Box::new(task_fn) }
    }

    pub async fn run(&self, key: K, handle: Listener) -> Option<R> {
        let mut tasks = self.tasks.lock().await;
        match tasks.get(&key) {
            None => {
                let task = {
                    let fut = (self.task_fn)(&key);
                    async move {
                        tokio::select! {
                            _ = handle => None,
                            result = fut => Some(result),
                        }
                    }
                    .boxed()
                    .shared()
                };
                tasks.insert(key.clone(), task.clone());
                drop(tasks);

                let result = task.await;

                {
                    let mut tasks = self.tasks.lock().await;
                    tasks.remove(&key);
                }

                result
            }
            Some(task) => {
                let task = task.clone();
                drop(tasks);

                tokio::select! {
                    _ = handle => None,
                    result = task => result,
                }
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

        let (_abort_tx, abort_handle) = triggered::trigger();
        group.run((), abort_handle).await;
    }

    #[tokio::test]
    async fn test_run_multiple() {
        let group: CondGroup<i32, (i32, i32)> = CondGroup::new(|num: &i32| {
            let num = *num;
            tokio::time::sleep(Duration::from_millis(100))
                .map(move |_| (num, rand::thread_rng().gen_range(0..1000)))
                .boxed()
        });

        let (_abort_tx, abort_handle) = triggered::trigger();
        let (a, b) =
            futures_util::join!(group.run(114, abort_handle.clone()), group.run(114, abort_handle));

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

        let (_abort_tx, abort_handle) = triggered::trigger();
        let (a, b, c, d, e) = futures_util::join!(
            group.run(114, abort_handle.clone()),
            group.run(114, abort_handle.clone()),
            group.run(514, abort_handle.clone()),
            group.run(514, abort_handle.clone()),
            group.run(1919, abort_handle.clone()),
        );

        assert_eq!(a, b);
        assert_eq!(c, d);
        assert!(matches!(e, Some((1919, _))));
    }
}
