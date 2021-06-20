/// Re-export of derived functions for [Args].
pub use gumdrop::Options;

use owo_colors::OwoColorize;
use sylt_common::error::{Error, RuntimeError};
use sylt_common::prog::Prog;
use sylt_common::rc::Rc;
use sylt_common::{RustFunction, Type, Value};
use std::borrow::Borrow;
use std::cell::RefCell;
use std::fmt::Debug;
use std::path::{Path, PathBuf};

pub mod vm;

// Lingon linking layer
#[cfg(feature = "lingon")]
pub mod lingon_sylt;
pub mod lib_sylt;

/// Generates the linking for the standard library, and lingon if it's active.
pub fn lib_bindings() -> Vec<(String, RustFunction)> {
    let mut lib = Vec::new();

    lib.append(&mut lib_sylt::_sylt_link());

    #[cfg(feature = "lingon")]
    lib.append(&mut lingon_sylt::_sylt_link());

    lib
}

pub fn compile(args: &Args, functions: Vec<(String, RustFunction)>) -> Result<Prog, Vec<Error>> {
    let path = match &args.file {
        Some(file) => file,
        None => {
            return Err(vec![Error::NoFileGiven]);
        }
    };
    let tree = sylt_parser::tree(&path)?;
    let prog = sylt_compiler::compile(tree, &functions)?;
    Ok(prog)
}

/// Compiles, links and runs the given file. The supplied functions are callable
/// external functions.
pub fn run_file(args: &Args, functions: Vec<(String, RustFunction)>) -> Result<(), Vec<Error>> {
    let prog = compile(args, functions)?;
    sylt_typechecker::typecheck(&prog, args.verbosity)?;
    run(&prog, &args)
}

pub fn run(prog: &Prog, args: &Args) -> Result<(), Vec<Error>> {
    let mut vm = vm::VM::new();
    vm.print_bytecode = args.verbosity >= 1;
    vm.print_exec = args.verbosity >= 2;
    vm.init(&prog);
    if let Err(e) = vm.run() {
        Err(vec![e])
    } else {
        Ok(())
    }
}

#[derive(Default, Debug, Options)]
pub struct Args {
    #[options(free)]
    pub file: Option<PathBuf>,

    #[options(short = "r", long = "run", help = "Runs a precompiled sylt binary")]
    pub is_binary: bool,

    #[options(short = "c", long = "compile", help = "Compile a sylt binary")]
    pub compile_target: Option<PathBuf>,

    #[options(short = "v", no_long, count, help = "Increase verbosity, up to max 2")]
    pub verbosity: u32,

    #[options(help = "Print this help")]
    pub help: bool,
}

impl Args {
    /// Wraps the function with the same name from [gumdrop] for convenience.
    pub fn parse_args_default_or_exit() -> Args {
        <Args as Options>::parse_args_default_or_exit()
    }
}

pub fn path_to_module(current_file: &Path, module: &str) -> PathBuf {
    let mut res = PathBuf::from(current_file);
    res.pop();
    res.push(format!("{}.sy", module));
    res
}

#[cfg(test)]
mod tests {
    #[macro_export]
    macro_rules! assert_errs {
        ($result:expr, $expect:pat) => {
            let errs = $result.err().unwrap_or(Vec::new());

            #[allow(unused_imports)]
            use ::sylt_common::error::Error;
            if !matches!(errs.as_slice(), $expect) {
                eprintln!("===== Got =====");
                for err in errs {
                    eprint!("{}", err);
                }
                eprintln!("===== Expect =====");
                eprint!("{}\n\n", stringify!($expect));
                assert!(false);
            }
        };
    }

    #[macro_export]
    macro_rules! test_file {
        ($fn:ident, $path:literal, $print:expr, $errs:pat) => {
            #[test]
            fn $fn() {
                #[allow(unused_imports)]
                use ::sylt_common::error::RuntimeError;
                #[allow(unused_imports)]
                use $crate::Type;

                let mut args = $crate::Args::default();
                args.file = Some(std::path::PathBuf::from(format!("../{}", $path)));
                args.verbosity = if $print { 1 } else { 0 };
                let res = $crate::run_file(
                    &args,
                    $crate::lib_sylt::_sylt_link(),
                );
                $crate::assert_errs!(res, $errs);
            }
        };
    }

    sylt_macro::find_tests!();
}
