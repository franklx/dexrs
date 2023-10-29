use dexrs::DesktopEntry;
use std::path::PathBuf;
use std::{env, fs};

fn main() {
    let path: PathBuf = env::args().nth(1).expect("Not enough arguments").into();
    let input = fs::read_to_string(&path).expect("Failed to read file");
    let de = DesktopEntry::decode(path.as_path(), &input).expect("Error decoding desktop entry");
    de.launch(&[]).expect("Failed to run desktop entry");
}
