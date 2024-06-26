use std::{
    fmt,
    future::Future,
    panic::{RefUnwindSafe, UnwindSafe},
    rc::Rc,
    sync::Arc,
};

pub struct Shard<T> {
    inner: Arc<[Rc<T>]>,
}

impl<T> fmt::Debug for Shard<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Shard")
            .field(std::any::type_name::<T>(), &"...")
            .finish()
    }
}

impl<T> Clone for Shard<T> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

unsafe impl<T> Send for Shard<T> {}
unsafe impl<T> Sync for Shard<T> {}

impl<T> UnwindSafe for Shard<T> {}
impl<T> RefUnwindSafe for Shard<T> {}

impl<T> Shard<T> {
    pub fn new(f: impl Fn() -> T + Send) -> Self {
        Self {
            inner: Arc::from(
                (0..crate::worker_num())
                    .map(|_| Rc::new((f)()))
                    .collect::<Vec<_>>(),
            ),
        }
    }

    pub async fn with<F, G>(&self, to: usize, f: impl FnOnce(Rc<T>) -> F + Send + 'static) -> G
    where
        F: Future<Output = G>,
        G: Send + 'static,
        T: 'static,
    {
        if crate::try_get_current_worker_id()
            .map(|id| to == id)
            .unwrap_or(false)
        {
            (f)(self.inner[to].clone()).await
        } else {
            let this = self.clone();
            crate::spawn_to(to, move || {
                let this = this;
                async move { (f)(this.inner[to].clone()).await }
            })
            .await
        }
    }
}

#[cfg(test)]
mod tests {
    use unsend::lock::RwLock;

    use crate::Executor;

    #[test]
    fn shard_per_thread() {
        Executor::builder()
            .worker_num(2)
            .build()
            .unwrap()
            .block_on(async {
                let local = super::Shard::new(|| RwLock::new(42));
                let a = local
                    .with(0, |a| async move {
                        let mut a = a.write().await;
                        *a += 1;
                        *a
                    })
                    .await;
                let b = local.with(1, |b| async move { *b.read().await }).await;
                let a_ = local.with(0, |a| async move { *a.read().await }).await;
                assert_eq!(a, 43);
                assert_eq!(b, 42);
                assert_eq!(a_, 43);
            });
    }
}
