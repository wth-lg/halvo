mod generated {
    #![allow(dead_code, unused_imports, clippy::all)]
    include!(concat!(env!("OUT_DIR"), "/generated.rs"));
}

const GENERATED: &str = include_str!(concat!(env!("OUT_DIR"), "/generated.rs"));

fn main() {
    print!("{GENERATED}");
}
