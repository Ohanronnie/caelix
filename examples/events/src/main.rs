use events::{block_on, emit_example_event};

fn main() {
    let entries = block_on(emit_example_event()).expect("event example failed");

    for entry in entries {
        println!("{entry}");
    }
}
