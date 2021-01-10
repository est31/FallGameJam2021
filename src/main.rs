use std::path::{Path, PathBuf};

mod tokenizer;
mod vm;
mod compiler;

fn main() {
    let file = file_from_args().unwrap_or_else(|| Path::new("tests/simple.tdy").to_owned());
    if let Err(err) = run_file(&file) {
        println!("{}", err);
    }
}

fn file_from_args() -> Option<PathBuf> {
    std::env::args().skip(1).map(|s| Path::new(&s).to_owned()).find(|p| p.is_file())
}

fn run_file(path: &Path) -> Result<(), vm::VMError> {
    let tokens = tokenizer::file_to_tokens(path);
    let block = compiler::compile("main", path, tokens);  // path -> str might fail
    vm::run_block(block)
}
    }
}
