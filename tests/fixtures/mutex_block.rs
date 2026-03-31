use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

fn main() {
    let table = Arc::new(Mutex::new(0));
    
    // Thread 1 locks and holds it forever
    let table_clone = Arc::clone(&table);
    thread::spawn(move || {
        let _lock = table_clone.lock().unwrap();
        thread::sleep(Duration::from_secs(100));
    });

    // Give thread 1 time to lock it
    thread::sleep(Duration::from_millis(100));

    // Main thread tries to lock it
    println!("Main thread about to lock"); // Breakpoint here
    let mut val = table.lock().unwrap(); // Step over this line
    *val += 1;
    println!("Done");
}
