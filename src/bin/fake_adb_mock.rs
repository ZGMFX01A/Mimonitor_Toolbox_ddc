use std::env;
use std::thread::sleep;
use std::time::Duration;

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.iter().any(|a| a == "sleep") {
        sleep(Duration::from_secs(30));
    } else if args.iter().any(|a| a == "large_output") {
        for i in 1..=2000 {
            println!("[LINE_{}] abcdefghijklmnopqrstuvwxyz0123456789", i);
        }
    } else if args.iter().any(|a| a == "unauthorized") {
        eprintln!("adb: device unauthorized");
        std::process::exit(1);
    } else if args.iter().any(|a| a == "devices_unauthorized") {
        println!("List of devices attached\r\n192.168.31.250:5555\tunauthorized");
    } else if args.iter().any(|a| a == "devices_offline") {
        println!("List of devices attached\r\n192.168.31.250:5555\toffline");
    } else if args.iter().any(|a| a == "devices_normal") {
        println!("List of devices attached\r\n192.168.31.250:5555\tdevice");
    } else {
        println!("fake adb ok");
    }
}
