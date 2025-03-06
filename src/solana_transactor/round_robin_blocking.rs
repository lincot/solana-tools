use std::sync::{Arc, Mutex};

pub struct RoundRobinBlocking<T> {
    pool: Arc<Vec<T>>,
    current_index: Arc<Mutex<usize>>,
}

impl<T> RoundRobinBlocking<T> {
    pub fn new(pool: Vec<T>) -> Self {
        Self {
            pool: Arc::new(pool),
            current_index: Arc::new(Mutex::new(0)),
        }
    }

    pub fn pull_by_max<'a, F>(&'a self, func: F) -> Option<(&T, u64)>
    where F: Fn(&'a T) -> u64 {
        let mut current_max = 0;
        let mut current_index = 0;
        for i in 0..self.pool.len() {
            let x = func(self.pool.get(i)?);
            if x > current_max {
                current_index = i;
                current_max = x;
            }
        }
        let mut current_index_handle =
            self.current_index.lock().expect("Failed to lock current_index");
        *current_index_handle = (current_index + 1) % self.pool.len();
        Some((self.pool.get(current_index)?, current_max))
    }

    pub fn len(&self) -> usize {
        self.pool.len()
    }
}

impl<T> Clone for RoundRobinBlocking<T> {
    fn clone(&self) -> Self {
        Self {
            pool: Arc::clone(&self.pool),
            current_index: Arc::clone(&self.current_index),
        }
    }
}
