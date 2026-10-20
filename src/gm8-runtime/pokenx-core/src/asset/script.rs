use crate::gml::{self, runtime::Instruction};
use serde::{Deserialize, Serialize};
use std::{cell::OnceCell, rc::Rc};

/// A GML script. Its instructions are compiled the first time the script runs, not at launch: compiling every one of
/// the title's ~5000 scripts took ~2 s on a PC and ~12 s on the Switch's Cortex-A57, and most are never called in a
/// session. The compiler must already know every script/constant name by then, which launch guarantees.
#[derive(Clone, Serialize, Deserialize)]
pub struct Script {
    pub name: gml::String,
    pub source: gml::String,
    #[serde(skip)]
    compiled: OnceCell<Rc<[Instruction]>>,
}

impl Script {
    pub fn new(name: gml::String, source: gml::String) -> Self {
        Self { name, source, compiled: OnceCell::new() }
    }

    /// The compiled instructions, compiling on first use.
    pub fn instructions(&self, compiler: &mut gml::Compiler) -> gml::Result<Rc<[Instruction]>> {
        if let Some(compiled) = self.compiled.get() {
            return Ok(compiled.clone());
        }
        let compiled = compiler
            .compile_game_script(self.name.as_ref(), self.source.as_ref())
            .map_err(|e| gml::Error::FunctionError(self.name.to_string().into(), format!("compiler error: {e}")))?;
        let _ = self.compiled.set(compiled.clone());
        Ok(compiled)
    }
}
