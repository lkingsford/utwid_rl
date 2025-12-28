use std::sync::mpsc::{SendError, Sender};

pub trait MctsSender<T>: Send + Sync + 'static {
    fn send(&self, t: T) -> Result<(), SendError<T>>;
    fn clone_sender(&self) -> Box<dyn MctsSender<T>>;
}

#[derive(Clone, Debug)]
pub struct NoopSender<T>
where
    T: Clone,
{
    _phantom: std::marker::PhantomData<T>,
}

impl<T: Clone> Default for NoopSender<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Clone> NoopSender<T> {
    pub fn new() -> Self {
        NoopSender {
            _phantom: std::marker::PhantomData,
        }
    }

    pub fn send(&self, _t: T) -> Result<(), SendError<T>> {
        // Do nothing, and always succeed.
        Ok(())
    }
}

impl<T: Send + Sync + 'static> MctsSender<T> for Sender<T> {
    fn send(&self, t: T) -> Result<(), SendError<T>> {
        self.send(t)
    }
    fn clone_sender(&self) -> Box<dyn MctsSender<T>> {
        Box::new(self.clone())
    }
}

impl<T: Send + Sync + Clone + 'static> MctsSender<T> for NoopSender<T> {
    fn send(&self, _t: T) -> Result<(), SendError<T>> {
        // Do nothing, and always succeed.
        Ok(())
    }
    fn clone_sender(&self) -> Box<dyn MctsSender<T>> {
        Box::new((*self).clone())
    }
}
