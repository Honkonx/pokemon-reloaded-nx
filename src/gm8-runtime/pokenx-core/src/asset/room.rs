use crate::{
    game::{Background, View},
    gml::{self, runtime::Instruction},
    tile::Tile,
    types::{Colour, ID},
};
use serde::{Deserialize, Serialize};
use std::{cell::OnceCell, rc::Rc};

#[derive(Clone, Serialize, Deserialize)]
pub struct Room {
    pub name: gml::String,
    pub caption: gml::String,
    pub width: u32,
    pub height: u32,
    pub speed: u32,
    pub persistent: bool,
    pub bg_colour: Colour,
    pub clear_screen: bool,
    pub creation_code: LazyCode,

    pub backgrounds: Vec<Background>,
    pub views_enabled: bool,
    pub views: Vec<View>,
    pub instances: Vec<Instance>,
    pub tiles: Vec<Tile>,
}

/// An instance stored in a Room
#[derive(Clone, Serialize, Deserialize)]
pub struct Instance {
    pub x: i32,
    pub y: i32,
    pub object: i32,
    pub id: ID,
    pub creation: LazyCode,
    pub xscale: f64,
    pub yscale: f64,
    pub blend: u32,
    pub angle: f64,
}

/// GML code stored in a room or one of its instances. Like a script, it is compiled the first time it runs (see
/// `Script`): the title has hundreds of rooms with thousands of instances, almost none of which a session enters, and
/// compiling them all made launch ~300 ms slower on a PC and several seconds on the Switch.
///
/// `load_room` clones the whole `Room` on every entry. The source and the compiled result live behind an `Rc` so every
/// clone shares them: compiled once, the code is reused on each later visit (before, the result was stored in the
/// throw-away clone and every visit compiled all of the room's code again).
#[derive(Clone, Serialize, Deserialize)]
pub struct LazyCode {
    inner: Rc<LazyInner>,
}

#[derive(Serialize, Deserialize)]
struct LazyInner {
    source: Box<[u8]>,
    #[serde(skip)]
    compiled: OnceCell<Result<Rc<[Instruction]>, String>>,
}

impl LazyCode {
    pub fn new(source: &[u8]) -> Self {
        Self { inner: Rc::new(LazyInner { source: source.into(), compiled: OnceCell::new() }) }
    }

    /// Code that is already compiled (rooms and instances made at run time).
    pub fn precompiled(code: Result<Rc<[Instruction]>, String>) -> Self {
        let compiled = OnceCell::new();
        let _ = compiled.set(code);
        Self { inner: Rc::new(LazyInner { source: Box::default(), compiled }) }
    }

    /// True once the code has been compiled (or was made compiled).
    pub fn is_compiled(&self) -> bool {
        self.inner.compiled.get().is_some()
    }

    /// True when there is nothing to run, so compiling it is pointless.
    pub fn is_empty(&self) -> bool {
        self.inner.source.is_empty() && self.inner.compiled.get().is_none()
    }

    /// The instructions, compiling on first use. `describe` turns the compiler's message into the error text.
    pub fn get(&self, compiler: &mut gml::Compiler, describe: impl FnOnce(&str) -> String) -> Result<Rc<[Instruction]>, String> {
        if let Some(done) = self.inner.compiled.get() {
            return done.clone();
        }
        let result = compiler.compile(&self.inner.source).map_err(|e| describe(&e.message));
        let _ = self.inner.compiled.set(result.clone());
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_clone_shares_the_compiled_code() {
        let original = LazyCode::new(b"x = 1;");
        let copy = original.clone();
        assert!(!original.is_compiled() && !copy.is_compiled());
        let mut compiler = gml::Compiler::new();
        let first = original.get(&mut compiler, |e| e.to_owned()).expect("compiles");
        // `load_room` clones the whole room on each entry: the copy must see what the original compiled.
        assert!(copy.is_compiled());
        let second = copy.get(&mut compiler, |e| e.to_owned()).expect("shared");
        assert!(Rc::ptr_eq(&first, &second));
    }

    #[test]
    fn empty_code_is_not_worth_compiling() {
        assert!(LazyCode::new(b"").is_empty());
        assert!(!LazyCode::new(b"x = 1;").is_empty());
        assert!(!LazyCode::precompiled(Ok(Rc::new([]))).is_empty());
    }
}
