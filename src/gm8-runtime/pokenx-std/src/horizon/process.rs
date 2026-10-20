//! `std::process`: `exit` only. `Command`/`Child` exist so code compiles; spawning is unsupported on Horizon.

use crate::horizon::io;
use alloc_crate::string::String;

extern "C" {
    #[link_name = "exit"]
    fn c_exit(code: i32) -> !;
}

pub fn exit(code: i32) -> ! {
    // SAFETY: newlib's `exit` runs the atexit handlers and returns control to the loader.
    unsafe { c_exit(code) }
}

#[derive(Debug)]
pub struct Command {
    #[allow(dead_code)]
    program: String,
}

#[derive(Debug)]
pub struct Child(());

#[derive(Clone, Copy, Debug)]
pub struct Stdio(());

impl Stdio {
    pub fn null() -> Stdio {
        Stdio(())
    }
    pub fn piped() -> Stdio {
        Stdio(())
    }
    pub fn inherit() -> Stdio {
        Stdio(())
    }
}

impl Command {
    pub fn new<S: AsRef<str>>(program: S) -> Command {
        Command { program: String::from(program.as_ref()) }
    }
    pub fn arg<S: AsRef<str>>(&mut self, _arg: S) -> &mut Command {
        self
    }
    pub fn args<I, S>(&mut self, _args: I) -> &mut Command
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        self
    }
    pub fn stdin(&mut self, _cfg: Stdio) -> &mut Command {
        self
    }
    pub fn stdout(&mut self, _cfg: Stdio) -> &mut Command {
        self
    }
    pub fn stderr(&mut self, _cfg: Stdio) -> &mut Command {
        self
    }
    pub fn spawn(&mut self) -> io::Result<Child> {
        Err(io::Error::new(io::ErrorKind::Unsupported, "spawning processes is not supported on Horizon"))
    }
}

impl Child {
    pub fn kill(&mut self) -> io::Result<()> {
        Err(io::Error::from(io::ErrorKind::Unsupported))
    }
    pub fn wait(&mut self) -> io::Result<()> {
        Err(io::Error::from(io::ErrorKind::Unsupported))
    }
}
