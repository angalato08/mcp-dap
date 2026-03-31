extern crate libc;

struct Data {
    nr: i32,
}

struct Notif {
    data: Data,
}

fn main() {
    let notif = Notif { data: Data { nr: 42 } };
    let is_static = true;
    
    println!("Parent PID: {}", unsafe { libc::getpid() });
    let pid = unsafe { libc::fork() };
    if pid == 0 {
        // Child
        println!("Child process PID: {}", unsafe { libc::getpid() });
        let mut x = 42;
        x += 1; // Breakpoint here at line 19
        println!("Child x: {}, notif.nr: {}, is_static: {}", x, notif.data.nr, is_static);
    } else if pid > 0 {
        // Parent
        println!("Spawned child with PID: {}", pid);
        let mut status = 0;
        unsafe { libc::waitpid(pid, &mut status, 0) };
        println!("Child exited");
    } else {
        eprintln!("Fork failed");
    }
}
