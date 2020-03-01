use rental::rental;
use std::sync::Arc;
use tokio::sync::{Mutex, MutexGuard};

pub use self::rent_mod::*;

rental! {
    pub mod rent_mod {
        use tokio::sync::{Mutex, MutexGuard};
        use std::sync::Arc;

        #[rental(debug, clone, deref_mut_suffix, covariant)]
        pub struct MutexAndGuard<T: 'static> {
            mutex: Arc<Mutex<T>>,
            guard: MutexGuard<'mutex, T>
        }
    }
}

impl<T: 'static> MutexAndGuard<T> {
    pub async fn async_new(mutex: Arc<Mutex<T>>) -> MutexAndGuard<T> {
        // I hope this is sound :)
        // moving mutex (Arc<Mutex<T>>) does nothing because it's StableDeref
        // by looking at the source of what MutexAndGuard::new gets expanded to, this is (hopefully)
        // extremely similar
        let guard_real_lifetime = mutex.lock().await;
        let guard_static: MutexGuard<'static, T> = unsafe { extend_lifetime(guard_real_lifetime) };
        MutexAndGuard::new(mutex, |_| guard_static)
    }
}

unsafe fn extend_lifetime<'s, T: 'static>(r: MutexGuard<'s, T>) -> MutexGuard<'static, T> {
    std::mem::transmute(r)
}

// TODO extension trait: .lock_owned() or something and OwnedMutexGuard<T>
