extern crate async;

use async::run_server;

fn main() {
    run_server("127.0.0.1:8080".to_string());
}


