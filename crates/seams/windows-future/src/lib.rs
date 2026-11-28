pub mod get {
    use windows_core::{Result, RuntimeType};

    use windows_future::{
        IAsyncAction, IAsyncActionWithProgress, IAsyncOperation, IAsyncOperationWithProgress,
    };

    pub trait Completion {
        type Output;

        fn value(self) -> Result<Self::Output>;
    }

    impl<T: RuntimeType> Completion for IAsyncOperation<T> {
        type Output = T;

        fn value(self) -> Result<T> {
            self.join()
        }
    }

    impl<T: RuntimeType, P: RuntimeType> Completion for IAsyncOperationWithProgress<T, P> {
        type Output = T;

        fn value(self) -> Result<T> {
            self.join()
        }
    }

    impl<P: RuntimeType> Completion for IAsyncActionWithProgress<P> {
        type Output = ();

        fn value(self) -> Result<()> {
            self.join()
        }
    }

    impl Completion for IAsyncAction {
        type Output = ();

        fn value(self) -> Result<()> {
            self.join()
        }
    }
}

pub use windows_future::{
    IAsyncAction, IAsyncActionWithProgress, IAsyncOperation, IAsyncOperationWithProgress,
};
