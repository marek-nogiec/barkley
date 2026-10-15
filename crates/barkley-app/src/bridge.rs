//! Runs a query off the main thread and hops the results back to it.
//! Each query gets a generation id; results for older generations are dropped.

use std::sync::Arc;

use barkley_core::Dispatcher;
use dispatch2::DispatchQueue;

pub fn search(dispatcher: Arc<Dispatcher>, text: String) {
    let generation = dispatcher.next_generation();
    std::thread::spawn(move || {
        if !dispatcher.is_current(generation) {
            return;
        }
        let items = dispatcher.search(&text);
        DispatchQueue::main().exec_async(move || {
            crate::controller::results_ready(&dispatcher, generation, items);
        });
    });
}
