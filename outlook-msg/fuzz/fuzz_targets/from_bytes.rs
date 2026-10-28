//! Reads arbitrary bytes as a `.msg` file.

#![no_main]

use libfuzzer_sys::fuzz_target;
use outlook_msg::Message;

fuzz_target!(|data: &[u8]| {
    if let Ok(message) = Message::from_bytes(data) {
        let _ = (message.subject(), message.sender(), message.message_class());
        let _ = message.body();
        for problem in message.problems() {
            let _ = problem.to_string();
        }
    }
});
